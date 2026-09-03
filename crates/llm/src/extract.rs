//! Reading a résumé into proposed profile fields.
//!
//! Nothing here writes anything. Every value the model returns is checked back
//! against the document by [`crate::verify`]. A value that does not anchor
//! there is not offered at all. It is not shown with a warning label. It is not
//! among the things a person can accept.
//!
//! The prompt asks the model to copy rather than paraphrase. That improves the
//! yield. It is not what makes the result safe. Verification is.

use crate::client::Client;
use crate::config::Model;
use crate::document::Document;
use crate::error::Result;
use crate::verify::{Anchor, Source};
use perch_core::{Position, Profile};
use serde_json::{json, Value};

/// Which profile field a proposal would write, if accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Name,
    Email,
    Phone,
    Location,
    Github,
    Website,
    Skills,
    /// Nth position, most recent first.
    Experience(usize),
}

impl Field {
    /// How it reads above the row.
    pub fn label(&self) -> String {
        match self {
            Field::Name => "Name".into(),
            Field::Email => "Email".into(),
            Field::Phone => "Phone".into(),
            Field::Location => "Location".into(),
            Field::Github => "GitHub".into(),
            Field::Website => "Website".into(),
            Field::Skills => "Skills".into(),
            Field::Experience(0) => "Employer: most recent".into(),
            Field::Experience(1) => "Employer: previous".into(),
            Field::Experience(n) => format!("Employer: {} back", n),
        }
    }

    /// The values one row writes, when it writes more than one. Editing such
    /// a row edits all of them, so an interface offering an edit has to say
    /// what they are.
    pub fn parts(&self) -> Option<&'static str> {
        matches!(self, Field::Experience(_)).then_some("company · title · dates")
    }

    fn current(&self, profile: &Profile) -> Option<String> {
        let value = match self {
            Field::Name => profile.name.clone(),
            Field::Email => profile.email.clone(),
            Field::Phone => profile.phone.clone(),
            Field::Location => profile.location.clone(),
            Field::Github => profile.links.github.clone(),
            Field::Website => profile.links.website.clone(),
            Field::Skills => profile.skills.join(", "),
            Field::Experience(n) => profile
                .experience
                .get(*n)
                .map(|p| format!("{} · {} · {}", p.company, p.title, p.dates))
                .unwrap_or_default(),
        };
        (!value.trim().is_empty()).then_some(value)
    }
}

/// One field the model proposed, with the evidence for it.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub field: Field,
    pub value: String,
    pub anchor: Anchor,
    /// What the profile holds today, if anything.
    pub current: Option<String>,
    /// The document line the value was found on, quoted whole.
    pub quote: Option<String>,
    /// The matched span, as an offset within `quote`.
    pub highlight: Option<(usize, usize)>,
    pub line: Option<usize>,
    /// Only a quotation arrives accepted. A rewrite waits for a person.
    pub accepted: bool,
    /// Carried for `Field::Experience`, which writes three values at once.
    position: Option<Position>,
}

impl Proposal {
    /// Whether the interface may offer an Accept control at all.
    pub fn offerable(&self) -> bool {
        self.anchor.offerable()
    }

    /// Why this one is not on offer.
    pub fn refusal(&self, document: &str) -> Option<String> {
        matches!(self.anchor, Anchor::None)
            .then(|| format!("No line in {document} says this, so Perch is not offering it."))
    }

    /// Replace the value with the person's own words.
    ///
    /// An edited value is no longer a quotation, and the interface has to say
    /// whose words it is. What an edit is not is a way around verification:
    /// the anchor is left exactly as it was, so a value Perch never offered
    /// cannot be typed into one it would write.
    pub fn edit(&mut self, value: &str) {
        let value = value.trim().to_string();
        let Some(mut position) = self.position.clone() else {
            self.value = value;
            return;
        };
        // The row reads "company · title · dates", so the edited line is read
        // back the same way. A part the person did not type keeps what it had:
        // leaving a separator out is not a way to empty two values without
        // anyone saying so. The row is then written back out in the shape it
        // was read in, so what it shows is what would be written.
        let mut typed = value.split('·').map(str::trim);
        for slot in [
            &mut position.company,
            &mut position.title,
            &mut position.dates,
        ] {
            let Some(part) = typed.next() else { break };
            *slot = part.to_string();
        }
        self.value = one_line(&position);
        self.position = Some(position);
    }

    /// Why this one needs a second look before it is accepted.
    pub fn caution(&self) -> Option<String> {
        match self.anchor {
            Anchor::Loose { fragment: true, .. } => Some(
                "These characters are in the document, but only inside something larger. \
                 The document does not state this on its own."
                    .to_string(),
            ),
            Anchor::Loose { .. } => Some(
                "This one is read rather than quoted. The document says something close \
                 to it. Worth a look before accepting."
                    .to_string(),
            ),
            _ => None,
        }
    }
}

fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "name":     { "type": "string" },
            "email":    { "type": "string" },
            "phone":    { "type": "string" },
            "location": { "type": "string" },
            "github":   { "type": "string" },
            "website":  { "type": "string" },
            "skills":   { "type": "string" },
            "experience": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "company": { "type": "string" },
                        "title":   { "type": "string" },
                        "dates":   { "type": "string" }
                    },
                    "required": ["company", "title", "dates"]
                }
            }
        },
        "required": ["name", "email", "phone", "location", "github", "website", "skills", "experience"]
    })
}

const SYSTEM: &str = "\
You read a résumé and copy facts out of it. You never summarise, rewrite, \
normalise or infer. Every value you return must appear in the document as \
written, character for character where possible. If the document does not state \
something, return an empty string for it rather than a guess. An empty field \
costs nothing and a wrong one wastes someone's time. Return JSON only.";

/// How a position reads on one row. The same shape is anchored, shown, and
/// read back when a person edits it.
fn one_line(position: &Position) -> String {
    [&position.company, &position.title, &position.dates]
        .iter()
        .filter(|part| !part.is_empty())
        .map(|part| part.as_str())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn text_of(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Turn one model reply into proposals, each carrying its evidence.
pub fn proposals_from(reply: &Value, document: &Document, profile: &Profile) -> Vec<Proposal> {
    let source = Source::new(&document.text);
    let mut out = Vec::new();

    let mut consider = |field: Field, value: String, position: Option<Position>| {
        if value.trim().is_empty() {
            return;
        }
        let anchor = source.anchor(&value);
        let (quote, highlight, line) = match anchor.span() {
            Some(span) => {
                let (from, to) = source.line_around(span);
                let quote = source.text()[from..to].to_string();
                (
                    Some(quote),
                    Some((span.0 - from, span.1 - from)),
                    Some(source.line_number(span)),
                )
            }
            None => (None, None, None),
        };
        out.push(Proposal {
            current: field.current(profile),
            accepted: anchor.pre_accepted(),
            field,
            value,
            anchor,
            quote,
            highlight,
            line,
            position,
        });
    };

    consider(Field::Name, text_of(reply, "name"), None);
    consider(Field::Email, text_of(reply, "email"), None);
    consider(Field::Phone, text_of(reply, "phone"), None);
    consider(Field::Location, text_of(reply, "location"), None);
    consider(Field::Github, text_of(reply, "github"), None);
    consider(Field::Website, text_of(reply, "website"), None);
    consider(Field::Skills, text_of(reply, "skills"), None);

    if let Some(roles) = reply.get("experience").and_then(Value::as_array) {
        for (n, role) in roles.iter().take(4).enumerate() {
            let position = Position {
                company: text_of(role, "company"),
                title: text_of(role, "title"),
                dates: text_of(role, "dates"),
            };
            if position.company.is_empty() {
                continue;
            }
            // Anchored as one line, in the order the résumé writes it, so the
            // evidence shown is the evidence checked.
            let value = one_line(&position);
            consider(Field::Experience(n), value, Some(position));
        }
    }

    out
}

/// Ask the model, then verify everything it said.
pub fn run(
    client: &Client,
    model: &Model,
    key: Option<&str>,
    document: &Document,
    profile: &Profile,
) -> Result<Vec<Proposal>> {
    let user = format!(
        "Copy the following facts out of this résumé: name, email, phone, \
         location, GitHub, personal website, a skills line, and up to four \
         positions with company, title and dates. Leave anything the document \
         does not state as an empty string.\n\n---\n{}\n---",
        document.text
    );
    let reply = client.ask_for_json(model, key, SYSTEM, &user, schema())?;
    Ok(proposals_from(&reply, document, profile))
}

/// Write the accepted proposals into the profile. Nothing else is touched, and
/// a proposal that was never offered cannot be here. [`Proposal::accepted`]
/// only becomes true through a person, or through an exact quotation.
pub fn apply(proposals: &[Proposal], profile: &mut Profile) -> usize {
    let mut written = 0;
    for proposal in proposals.iter().filter(|p| p.accepted && p.offerable()) {
        match (&proposal.field, &proposal.position) {
            (Field::Name, _) => profile.name = proposal.value.clone(),
            (Field::Email, _) => profile.email = proposal.value.clone(),
            (Field::Phone, _) => profile.phone = proposal.value.clone(),
            (Field::Location, _) => profile.location = proposal.value.clone(),
            (Field::Github, _) => profile.links.github = proposal.value.clone(),
            (Field::Website, _) => profile.links.website = proposal.value.clone(),
            (Field::Skills, _) => {
                profile.skills = proposal
                    .value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            (Field::Experience(n), Some(position)) => {
                // A slot that is not there yet is appended rather than padded
                // up to, so a row the person skipped leaves no empty position
                // in the file and the count returned is the count written.
                match profile.experience.get_mut(*n) {
                    Some(slot) => *slot = position.clone(),
                    None => profile.experience.push(position.clone()),
                }
            }
            (Field::Experience(_), None) => continue,
        }
        written += 1;
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Kind;

    const RESUME: &str = "\
DANA FERREIRA
dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira
SF Bay Area · open to remote

EXPERIENCE

Cloudflare — Senior Software Engineer, Storage
March 2023 – February 2026

Honeycomb — Software Engineer, Infrastructure
August 2020 – February 2023

SKILLS
Rust, Go, Tokio, gRPC, PostgreSQL";

    fn doc() -> Document {
        Document {
            text: RESUME.to_string(),
            kind: Kind::Text,
            name: "resume-systems.pdf".into(),
        }
    }

    fn find<'a>(proposals: &'a [Proposal], label: &str) -> &'a Proposal {
        proposals
            .iter()
            .find(|p| p.field.label() == label)
            .unwrap_or_else(|| panic!("no proposal for {label}"))
    }

    #[test]
    fn a_quoted_value_arrives_accepted_with_its_line() {
        let reply = json!({
            "name": "DANA FERREIRA",
            "email": "dana@dferreira.dev",
            "phone": "+1 (415) 555-0148",
            "location": "", "github": "", "website": "", "skills": "",
            "experience": []
        });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());
        let name = find(&proposals, "Name");
        assert!(matches!(name.anchor, Anchor::Exact { .. }));
        assert!(name.accepted);
        assert_eq!(name.quote.as_deref(), Some("DANA FERREIRA"));
        assert_eq!(name.line, Some(1));
        let (a, b) = name.highlight.unwrap();
        assert_eq!(&name.quote.as_ref().unwrap()[a..b], "DANA FERREIRA");
    }

    #[test]
    fn an_invented_value_is_not_offered_and_cannot_be_written() {
        // The exact failure this feature exists to prevent: a plausible summary
        // the document never states.
        let reply = json!({
            "name": "Dana Ferreira",
            "email": "dana@gmail.com",
            "phone": "", "location": "", "github": "", "website": "",
            "skills": "", "experience": [
                { "company": "Stripe", "title": "Staff Engineer", "dates": "2019 – 2021" }
            ]
        });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());

        let email = find(&proposals, "Email");
        assert_eq!(email.anchor, Anchor::None);
        assert!(!email.offerable());
        assert!(!email.accepted);
        assert!(email
            .refusal("resume-systems.pdf")
            .unwrap()
            .contains("not offering it"));

        let employer = find(&proposals, "Employer: most recent");
        assert_eq!(employer.anchor, Anchor::None);
        assert!(!employer.offerable());

        // And even if something set accepted, apply refuses an unoffered one.
        let mut forced = proposals.clone();
        for p in forced.iter_mut() {
            p.accepted = true;
        }
        let mut profile = Profile::default();
        apply(&forced, &mut profile);
        assert_eq!(profile.email, "", "an unanchored value reached the profile");
        assert!(profile.experience.is_empty());
        assert_eq!(
            profile.name, "Dana Ferreira",
            "the anchored one should write"
        );
    }

    #[test]
    fn a_rewrite_is_offered_but_waits_for_a_person() {
        let reply = json!({
            "name": "", "email": "", "phone": "",
            "location": "San Francisco Bay Area, open to remote",
            "github": "", "website": "", "skills": "", "experience": []
        });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());
        let location = find(&proposals, "Location");
        assert!(
            matches!(location.anchor, Anchor::Loose { .. }),
            "{:?}",
            location.anchor
        );
        assert!(location.offerable());
        assert!(!location.accepted, "a rewrite must not arrive accepted");
        assert!(location
            .caution()
            .unwrap()
            .contains("read rather than quoted"));
        assert!(location.quote.is_some());

        // It writes only once a person has said so.
        let mut profile = Profile::default();
        apply(&proposals, &mut profile);
        assert_eq!(profile.location, "");
        let mut accepted = proposals.clone();
        accepted[0].accepted = true;
        apply(&accepted, &mut profile);
        assert_eq!(profile.location, "San Francisco Bay Area, open to remote");
    }

    #[test]
    fn what_the_profile_already_holds_is_shown_beside_the_proposal() {
        let profile = Profile {
            email: "dana@hey.com".into(),
            ..Profile::default()
        };
        let reply = json!({
            "name": "", "email": "dana@dferreira.dev", "phone": "", "location": "",
            "github": "", "website": "", "skills": "", "experience": []
        });
        let proposals = proposals_from(&reply, &doc(), &profile);
        assert_eq!(
            find(&proposals, "Email").current.as_deref(),
            Some("dana@hey.com")
        );
    }

    #[test]
    fn positions_write_all_three_parts_together() {
        let reply = json!({
            "name": "", "email": "", "phone": "", "location": "",
            "github": "", "website": "", "skills": "",
            "experience": [
                { "company": "Cloudflare", "title": "Senior Software Engineer, Storage", "dates": "March 2023 – February 2026" },
                { "company": "Honeycomb", "title": "Software Engineer, Infrastructure", "dates": "August 2020 – February 2023" }
            ]
        });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());
        assert_eq!(proposals.len(), 2);
        assert!(
            proposals.iter().all(|p| p.accepted),
            "both are quoted in the document"
        );

        let mut profile = Profile::default();
        assert_eq!(apply(&proposals, &mut profile), 2);
        assert_eq!(profile.experience[0].company, "Cloudflare");
        assert_eq!(profile.experience[0].dates, "March 2023 – February 2026");
        assert_eq!(profile.experience[1].company, "Honeycomb");
    }

    #[test]
    fn a_skipped_employer_leaves_no_empty_record_behind() {
        let reply = json!({
            "name": "", "email": "", "phone": "", "location": "",
            "github": "", "website": "", "skills": "",
            "experience": [
                { "company": "Cloudflare", "title": "Senior Software Engineer, Storage", "dates": "March 2023 – February 2026" },
                { "company": "Honeycomb", "title": "Software Engineer, Infrastructure", "dates": "August 2020 – February 2023" }
            ]
        });
        let mut proposals = proposals_from(&reply, &doc(), &Profile::default());
        // The most recent employer is turned down and the one before it is
        // kept, which is the order the rows are read in.
        proposals[0].accepted = false;
        proposals[1].accepted = true;

        let mut profile = Profile::default();
        assert_eq!(apply(&proposals, &mut profile), 1);
        assert_eq!(
            profile.experience.len(),
            1,
            "the skipped row wrote a record"
        );
        assert_eq!(profile.experience[0].company, "Honeycomb");
    }

    #[test]
    fn an_edit_that_leaves_a_part_out_keeps_what_that_part_had() {
        let reply = json!({
            "name": "", "email": "", "phone": "", "location": "",
            "github": "", "website": "", "skills": "",
            "experience": [
                { "company": "Cloudflare", "title": "Senior Software Engineer, Storage", "dates": "March 2023 – February 2026" }
            ]
        });
        let mut proposals = proposals_from(&reply, &doc(), &Profile::default());
        // The company alone, with none of the separators the row is read by.
        proposals[0].edit("Cloudflare, Inc.");
        proposals[0].accepted = true;

        let mut profile = Profile::default();
        assert_eq!(apply(&proposals, &mut profile), 1);
        assert_eq!(profile.experience[0].company, "Cloudflare, Inc.");
        assert_eq!(
            profile.experience[0].title,
            "Senior Software Engineer, Storage"
        );
        assert_eq!(profile.experience[0].dates, "March 2023 – February 2026");
        assert_eq!(
            proposals[0].value,
            "Cloudflare, Inc. · Senior Software Engineer, Storage · March 2023 – February 2026",
            "the row shows the three values it would write"
        );
    }

    #[test]
    fn an_edited_value_writes_what_the_person_typed() {
        let reply = json!({
            "name": "DANA FERREIRA", "email": "", "phone": "", "location": "",
            "github": "", "website": "", "skills": "",
            "experience": [
                { "company": "Cloudflare", "title": "Senior Software Engineer, Storage", "dates": "March 2023 – February 2026" }
            ]
        });
        let mut proposals = proposals_from(&reply, &doc(), &Profile::default());
        for proposal in proposals.iter_mut() {
            match proposal.field {
                Field::Name => proposal.edit("  Dana Ferreira  "),
                Field::Experience(_) => {
                    proposal.edit("Cloudflare · Staff Engineer · March 2023 to February 2026")
                }
                _ => {}
            }
            proposal.accepted = true;
        }

        let mut profile = Profile::default();
        assert_eq!(apply(&proposals, &mut profile), 2);
        assert_eq!(profile.name, "Dana Ferreira");
        assert_eq!(profile.experience[0].company, "Cloudflare");
        assert_eq!(profile.experience[0].title, "Staff Engineer");
        assert_eq!(profile.experience[0].dates, "March 2023 to February 2026");
    }

    #[test]
    fn editing_does_not_make_an_unoffered_value_writable() {
        let reply = json!({
            "name": "", "email": "dana@gmail.com", "phone": "", "location": "",
            "github": "", "website": "", "skills": "", "experience": []
        });
        let mut proposals = proposals_from(&reply, &doc(), &Profile::default());
        proposals[0].edit("dana@gmail.com");
        proposals[0].accepted = true;
        assert!(
            !proposals[0].offerable(),
            "the anchor decides, not the edit"
        );

        let mut profile = Profile::default();
        assert_eq!(apply(&proposals, &mut profile), 0);
        assert_eq!(profile.email, "");
    }

    #[test]
    fn an_empty_field_is_not_a_proposal() {
        let reply = json!({
            "name": "", "email": "   ", "phone": "", "location": "",
            "github": "", "website": "", "skills": "", "experience": []
        });
        assert!(proposals_from(&reply, &doc(), &Profile::default()).is_empty());
    }

    #[test]
    fn a_reply_missing_half_its_keys_does_not_derail_the_import() {
        let reply = json!({ "name": "DANA FERREIRA" });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());
        assert_eq!(proposals.len(), 1);
        assert!(proposals[0].accepted);
    }

    #[test]
    fn skills_become_a_list_only_when_accepted() {
        let reply = json!({
            "name": "", "email": "", "phone": "", "location": "", "github": "",
            "website": "", "skills": "Rust, Go, Tokio, gRPC, PostgreSQL",
            "experience": []
        });
        let proposals = proposals_from(&reply, &doc(), &Profile::default());
        let mut profile = Profile::default();
        apply(&proposals, &mut profile);
        assert_eq!(
            profile.skills,
            ["Rust", "Go", "Tokio", "gRPC", "PostgreSQL"]
        );
    }
}
