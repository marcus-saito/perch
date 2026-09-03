//! Turning a profile and a form into a plan, and nothing more.

use crate::flavor::{self, Flavor, Kind};
use crate::Action;
use perch_core::{Ats, Profile};
use serde::Serialize;

/// Where a value came from, shown beside every filled field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "camelCase")]
pub enum Provenance {
    /// The profile file the person wrote or accepted into.
    Profile,
    /// A document they attached.
    Document { name: String },
}

impl Provenance {
    /// How it reads under the value.
    pub fn label(&self) -> String {
        match self {
            Provenance::Profile => "profile.toml".into(),
            Provenance::Document { name } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub label: String,
    #[serde(flatten)]
    pub action: Action,
    pub provenance: Provenance,
}

/// A field Perch could answer but will not, because it would be guessing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flagged {
    pub label: String,
    pub selector: String,
    pub why: String,
}

/// Free text Perch leaves alone on purpose.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftToYou {
    pub label: String,
    pub selector: String,
    pub why: String,
}

/// A question Perch will not answer at all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    pub label: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillPlan {
    pub ats: String,
    pub url: String,
    pub entries: Vec<Entry>,
    pub flagged: Vec<Flagged>,
    pub left_to_you: Vec<LeftToYou>,
    pub never: Vec<Refused>,
}

impl FillPlan {
    /// The sentence the interface shows before anything happens. It says what
    /// will occur and where it stops.
    pub fn what_happens_next(&self, company: &str) -> String {
        // No positional word like "below": the terminal prints the values
        // above this sentence and the sheet prints them under it.
        let typed = match self.entries.len() {
            0 => "types nothing into it, because your profile is empty".to_string(),
            1 => "types the one value into it".to_string(),
            n => format!("types the {} values into it", spell(n)),
        };
        format!(
            "Perch opens {company}'s {} form in a window and {typed}. It will not press submit. \
             There is no submit anywhere in this program. The form is left open and filled in, \
             for you to review and send.",
            self.ats
        )
    }

    /// The one thing Perch cannot promise.
    ///
    /// Setting a file on an `<input type=file>` works at the DOM level, and a
    /// board that reads the input at submit time gets the file. Several ATSes
    /// wrap the input in their own uploader that keeps its own state and never
    /// reads `input.files`. Greenhouse's does, verified against a live
    /// posting, so the box still reads "Attach" and no file is attached.
    ///
    /// Perch cannot tell the two apart from outside the page, so it says so.
    pub fn attachment_caveat(&self) -> Option<String> {
        self.entries
            .iter()
            .any(|e| matches!(e.action, Action::AttachFile { .. }))
            .then(|| {
                "The form uploads the file itself once Perch hands it over. Check the file \
                 box shows it before you send, because Perch cannot see from outside the page \
                 whether the upload landed."
                    .to_string()
            })
    }
}

/// Small numbers read as words in prose; larger ones are clearer as numerals.
fn spell(n: usize) -> String {
    const WORDS: [&str; 13] = [
        "no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    WORDS
        .get(n)
        .map(|w| w.to_string())
        .unwrap_or_else(|| n.to_string())
}

fn first_and_last(name: &str) -> (String, String) {
    let mut parts = name.split_whitespace();
    let first = parts.next().unwrap_or_default().to_string();
    let last = parts.collect::<Vec<_>>().join(" ");
    (first, last)
}

/// Build the plan. Nothing here touches a browser, a network or a file. It
/// decides what *would* be typed, so a person can read it first.
pub fn build(ats: Ats, url: &str, profile: &Profile, resume: Option<&str>) -> Option<FillPlan> {
    let Flavor { fields, .. } = flavor::for_ats(ats)?;
    let (first, last) = first_and_last(&profile.name);

    let mut plan = FillPlan {
        ats: ats.label().to_string(),
        url: url.to_string(),
        entries: Vec::new(),
        flagged: Vec::new(),
        left_to_you: Vec::new(),
        never: Vec::new(),
    };

    for field in fields {
        // Checked first, and by label as well as by kind. A question Perch has
        // never seen before still cannot be answered on someone's behalf.
        if field.kind.is_demographic() || flavor::label_is_demographic(field.label) {
            plan.never.push(Refused {
                label: field.label.to_string(),
                why:
                    "Perch does not fill demographic questions, and does not store answers to them."
                        .into(),
            });
            continue;
        }

        if field.kind.is_left_to_you() {
            plan.left_to_you.push(LeftToYou {
                label: field.label.to_string(),
                selector: field.selector.to_string(),
                why: match field.kind {
                    Kind::CoverLetter => "Perch does not write cover letters.".into(),
                    _ => "Perch does not answer this one for you.".to_string(),
                },
            });
            continue;
        }

        if field.kind.is_flagged() {
            plan.flagged.push(Flagged {
                label: field.label.to_string(),
                selector: field.selector.to_string(),
                why: match field.kind {
                    Kind::StartDate => {
                        "Perch does not know your start date. This is left blank.".into()
                    }
                    _ => "Perch does not know how you found this role. This is left blank."
                        .to_string(),
                },
            });
            continue;
        }

        let (value, provenance) = match field.kind {
            Kind::FullName => (profile.name.clone(), Provenance::Profile),
            Kind::FirstName => (first.clone(), Provenance::Profile),
            Kind::LastName => (last.clone(), Provenance::Profile),
            Kind::Email => (profile.email.clone(), Provenance::Profile),
            Kind::Phone => (profile.phone.clone(), Provenance::Profile),
            Kind::Location => (profile.location.clone(), Provenance::Profile),
            Kind::Github => (profile.links.github.clone(), Provenance::Profile),
            Kind::Website => (profile.links.website.clone(), Provenance::Profile),
            Kind::Linkedin => (profile.links.linkedin.clone(), Provenance::Profile),
            Kind::Resume => match resume {
                Some(path) => (
                    path.to_string(),
                    Provenance::Document {
                        name: std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path)
                            .to_string(),
                    },
                ),
                None => continue,
            },
            // Handled above; listed so a new kind cannot be forgotten here.
            Kind::CoverLetter
            | Kind::WhyCompany
            | Kind::StartDate
            | Kind::HowDidYouHear
            | Kind::Demographic => continue,
        };

        // A field the profile has nothing for is left alone rather than
        // cleared: an empty string typed into a form is still an edit.
        if value.trim().is_empty() {
            continue;
        }

        let action = if field.kind == Kind::Resume {
            Action::AttachFile {
                selector: field.selector.to_string(),
                path: value,
            }
        } else {
            Action::SetText {
                selector: field.selector.to_string(),
                labels: field.labels.iter().map(|l| l.to_string()).collect(),
                value,
            }
        };

        plan.entries.push(Entry {
            label: field.label.to_string(),
            action,
            provenance,
        });
    }

    Some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_core::profile::Links;

    fn profile() -> Profile {
        Profile {
            name: "Dana Ferreira".into(),
            email: "dana@dferreira.dev".into(),
            phone: "+1 415 555 0148".into(),
            location: "Oakland, California".into(),
            work_authorisation: "US citizen. I don't need sponsorship.".into(),
            links: Links {
                github: "github.com/dferreira".into(),
                website: "dferreira.dev".into(),
                linkedin: String::new(),
            },
            ..Profile::default()
        }
    }

    fn plan(ats: Ats) -> FillPlan {
        build(
            ats,
            "https://example.invalid/apply",
            &profile(),
            Some("/docs/resume-systems.pdf"),
        )
        .unwrap()
    }

    #[test]
    fn no_demographic_question_is_ever_filled_on_any_ats() {
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let plan = plan(ats);
            assert!(!plan.never.is_empty(), "{ats:?} refused nothing");
            for entry in &plan.entries {
                assert!(
                    !flavor::label_is_demographic(&entry.label),
                    "{ats:?} would fill {:?}",
                    entry.label
                );
            }
            // And they are not quietly dropped either. The person is told.
            for refused in &plan.never {
                assert!(refused.why.contains("does not store"));
            }
        }
    }

    #[test]
    fn a_field_perch_cannot_know_is_left_visibly_blank_with_a_reason() {
        let plan = plan(Ats::Ashby);
        let labels: Vec<&str> = plan.flagged.iter().map(|f| f.label.as_str()).collect();
        assert!(labels.contains(&"Preferred start date"), "{labels:?}");
        assert!(labels.contains(&"How did you hear about us"));
        for flagged in &plan.flagged {
            assert!(!flagged.why.is_empty());
            assert!(!plan.entries.iter().any(|e| e.label == flagged.label));
        }
    }

    #[test]
    fn free_text_is_left_empty_on_purpose_and_says_why() {
        let plan = plan(Ats::Ashby);
        let labels: Vec<&str> = plan.left_to_you.iter().map(|f| f.label.as_str()).collect();
        assert!(
            labels.contains(&"Why do you want to work here"),
            "{labels:?}"
        );
        assert!(plan.left_to_you.iter().any(|f| f.why.contains("you")));
        assert!(!plan.entries.iter().any(|e| e.label.contains("Why")));
    }

    #[test]
    fn every_filled_value_says_where_it_came_from() {
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            for entry in &plan(ats).entries {
                match &entry.provenance {
                    Provenance::Profile => assert_eq!(entry.provenance.label(), "profile.toml"),
                    Provenance::Document { name } => assert_eq!(name, "resume-systems.pdf"),
                }
            }
        }
    }

    #[test]
    fn a_name_is_split_only_where_the_form_asks_for_halves() {
        let greenhouse = plan(Ats::Greenhouse);
        let first = greenhouse
            .entries
            .iter()
            .find(|e| e.label == "First name")
            .unwrap();
        let last = greenhouse
            .entries
            .iter()
            .find(|e| e.label == "Last name")
            .unwrap();
        assert_eq!(first.action.shown_value(), "Dana");
        assert_eq!(last.action.shown_value(), "Ferreira");

        let lever = plan(Ats::Lever);
        let full = lever
            .entries
            .iter()
            .find(|e| e.label == "Full name")
            .unwrap();
        assert_eq!(full.action.shown_value(), "Dana Ferreira");
    }

    #[test]
    fn an_empty_profile_field_is_left_alone_rather_than_cleared() {
        // The profile has no LinkedIn. Typing "" into the box would still be
        // an edit, and would overwrite anything already there.
        let plan = plan(Ats::Lever);
        assert!(!plan.entries.iter().any(|e| e.label == "LinkedIn"));
        assert!(plan
            .entries
            .iter()
            .all(|e| !e.action.shown_value().trim().is_empty()));
    }

    #[test]
    fn with_no_resume_chosen_nothing_is_attached() {
        let plan = build(Ats::Lever, "https://x.invalid", &profile(), None).unwrap();
        assert!(!plan
            .entries
            .iter()
            .any(|e| matches!(e.action, Action::AttachFile { .. })));
    }

    #[test]
    fn an_empty_profile_produces_a_plan_that_does_nothing() {
        let plan = build(Ats::Ashby, "https://x.invalid", &Profile::default(), None).unwrap();
        assert!(
            plan.entries.is_empty(),
            "an empty profile filled something in"
        );
        // The refusals and the deliberate blanks are still described.
        assert!(!plan.never.is_empty());
        assert!(!plan.flagged.is_empty());
        assert!(!plan.left_to_you.is_empty());
    }

    #[test]
    fn a_board_perch_cannot_fill_has_no_plan_at_all() {
        assert!(build(Ats::JsonLd, "https://x.invalid", &profile(), None).is_none());
    }

    #[test]
    fn the_sentence_says_where_it_stops() {
        let plan = plan(Ats::Ashby);
        let said = plan.what_happens_next("Val Town");
        assert!(said.contains("will not press submit"));
        assert!(said.contains("no submit anywhere in this program"));
        assert!(said.contains("left open"));
    }

    #[test]
    fn the_one_thing_perch_cannot_promise_is_said_out_loud() {
        // Verified against a live Greenhouse posting: the file lands on the
        // input, and the board's own uploader ignores it. Someone must not be
        // left believing a résumé is attached when it is not.
        let with_file = plan(Ats::Greenhouse);
        let caveat = with_file
            .attachment_caveat()
            .expect("a plan with a file must warn");
        assert!(caveat.contains("Check the file box"));

        // No file chosen, nothing to warn about.
        let without = build(Ats::Greenhouse, "https://x.invalid", &profile(), None).unwrap();
        assert!(without.attachment_caveat().is_none());
    }

    #[test]
    fn the_sentence_counts_in_words_and_tells_the_truth_at_zero() {
        let empty = build(Ats::Ashby, "https://x.invalid", &Profile::default(), None).unwrap();
        let said = empty.what_happens_next("Val Town");
        assert!(!said.contains("the 0 values"), "{said}");
        assert!(said.contains("types nothing into it"), "{said}");
        // And it still says where it stops.
        assert!(said.contains("will not press submit"));

        let with_values = plan(Ats::Ashby);
        let full = with_values.what_happens_next("Val Town");
        let n = with_values.entries.len();
        assert!(
            !full.contains(&format!(" {n} values")),
            "counts should read as words: {full}"
        );
        assert!(
            full.contains(&format!("{} values into it", spell(n))),
            "{full}"
        );
        // No positional word: the terminal prints the values above this
        // sentence and the sheet prints them under it.
        assert!(!full.contains("below") && !full.contains("above"), "{full}");
    }
}
