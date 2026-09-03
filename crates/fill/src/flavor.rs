//! What each ATS's application form looks like.
//!
//! Adding an ATS is a `Flavor` and one line in [`for_ats`]. Monitoring and
//! filling stay separate concerns: a board Perch can watch is not necessarily
//! one it can fill, and the adapter says which in `AtsAdapter::fill_supported`.

use perch_core::Ats;
use url::Url;

/// What a form field is for, which decides what Perch does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    FullName,
    FirstName,
    LastName,
    Email,
    Phone,
    Location,
    Github,
    Website,
    Linkedin,
    /// A file the person already chose.
    Resume,
    /// Free text Perch will not write.
    CoverLetter,
    WhyCompany,
    /// Perch has no way to know these, and will not guess at them.
    StartDate,
    HowDidYouHear,
    /// Never filled, never stored.
    Demographic,
}

impl Kind {
    /// Free text whose whole point is that a person wrote it.
    pub fn is_left_to_you(self) -> bool {
        matches!(self, Kind::CoverLetter | Kind::WhyCompany)
    }

    /// Answerable, but only by the person.
    pub fn is_flagged(self) -> bool {
        matches!(self, Kind::StartDate | Kind::HowDidYouHear)
    }

    pub fn is_demographic(self) -> bool {
        matches!(self, Kind::Demographic)
    }
}

#[derive(Debug, Clone)]
pub struct FormField {
    /// What the form calls it, as it reads on the page.
    pub label: &'static str,
    /// Empty when the board gives Perch nothing stable to write down.
    pub selector: &'static str,
    /// The words printed beside the box, tried when the selector finds
    /// nothing. Ashby mints an id for each of its own questions per posting,
    /// so there is no selector to write in advance and the label is the only
    /// handle that holds still.
    pub labels: &'static [&'static str],
    pub kind: Kind,
}

pub struct Flavor {
    pub ats: Ats,
    pub fields: Vec<FormField>,
}

fn field(label: &'static str, selector: &'static str, kind: Kind) -> FormField {
    FormField {
        label,
        selector,
        labels: &[],
        kind,
    }
}

/// A box with no selector worth writing, found by the words beside it.
fn labelled(label: &'static str, labels: &'static [&'static str], kind: Kind) -> FormField {
    FormField {
        label,
        selector: "",
        labels,
        kind,
    }
}

/// A box whose selector holds on some postings and not others. The selector is
/// tried first, because a name the board chose is a surer thing than words a
/// company typed.
fn field_or_labelled(
    label: &'static str,
    selector: &'static str,
    labels: &'static [&'static str],
    kind: Kind,
) -> FormField {
    FormField {
        label,
        selector,
        labels,
        kind,
    }
}

/// The form Perch knows how to fill for one ATS.
///
/// Selectors are best-effort: a board can change its markup, and a field Perch
/// does not find is simply one the person fills themselves. Nothing here can
/// do harm by being wrong: a selector that matches nothing types nothing.
pub fn for_ats(ats: Ats) -> Option<Flavor> {
    let fields = match ats {
        Ats::Greenhouse => vec![
            field("First name", "input#first_name", Kind::FirstName),
            field("Last name", "input#last_name", Kind::LastName),
            field("Email", "input#email", Kind::Email),
            field("Phone", "input#phone", Kind::Phone),
            field("Résumé", "input#resume", Kind::Resume),
            field("Website", "input#job_application_website", Kind::Website),
            field(
                "Cover letter",
                "textarea#cover_letter_text",
                Kind::CoverLetter,
            ),
            field(
                "How did you hear about us",
                "input#source",
                Kind::HowDidYouHear,
            ),
            field("Gender", "select#gender", Kind::Demographic),
            field("Race", "select#race", Kind::Demographic),
            field("Veteran status", "select#veteran_status", Kind::Demographic),
            field(
                "Disability status",
                "select#disability_status",
                Kind::Demographic,
            ),
        ],
        Ats::Lever => vec![
            field("Full name", "input[name='name']", Kind::FullName),
            field("Email", "input[name='email']", Kind::Email),
            field("Phone", "input[name='phone']", Kind::Phone),
            field("Current location", "input[name='location']", Kind::Location),
            field("Résumé", "input[name='resume']", Kind::Resume),
            field("LinkedIn", "input[name='urls[LinkedIn]' i]", Kind::Linkedin),
            field("GitHub", "input[name='urls[GitHub]' i]", Kind::Github),
            field(
                "Portfolio",
                "input[name='urls[Portfolio]' i], input[name='urls[Other Website]' i], \
                 input[name='urls[Website]' i]",
                Kind::Website,
            ),
            field(
                "Additional information",
                "textarea[name='comments']",
                Kind::CoverLetter,
            ),
            field(
                "How did you hear about this job",
                "input[name='sources']",
                Kind::HowDidYouHear,
            ),
            field("Gender", "select[name='eeo[gender]']", Kind::Demographic),
            field("Race", "select[name='eeo[race]']", Kind::Demographic),
            field(
                "Veteran status",
                "select[name='eeo[veteran]']",
                Kind::Demographic,
            ),
            field(
                "Disability status",
                "select[name='eeo[disability]']",
                Kind::Demographic,
            ),
        ],
        // Ashby names the boxes it owns itself, and mints an id per posting
        // for every question a company adds. Those ids are in the page and
        // nowhere else, so the words beside the box are what Perch matches on.
        // Labels are the ones read off live Linear, Ramp and Notion forms.
        Ats::Ashby => vec![
            field_or_labelled(
                "Name",
                "input[name='_systemfield_name'], input#_systemfield_name",
                &["Name", "Full Name", "Legal Name"],
                Kind::FullName,
            ),
            field_or_labelled(
                "Email",
                "input[name='_systemfield_email'], input#_systemfield_email",
                &["Email", "Email Address"],
                Kind::Email,
            ),
            field_or_labelled(
                "Phone",
                "input[name='_systemfield_phone'], input#_systemfield_phone",
                &["Phone", "Phone Number"],
                Kind::Phone,
            ),
            field_or_labelled(
                "Location",
                "input[name='_systemfield_location'], input#_systemfield_location",
                &["Location", "Current Location"],
                Kind::Location,
            ),
            field(
                "Résumé",
                "input[name='_systemfield_resume'], input#_systemfield_resume",
                Kind::Resume,
            ),
            labelled(
                "GitHub",
                &["Github", "GitHub", "GitHub Profile"],
                Kind::Github,
            ),
            labelled(
                "Website",
                &["Website", "Personal Website", "Portfolio"],
                Kind::Website,
            ),
            labelled(
                "LinkedIn",
                &["LinkedIn", "LinkedIn Profile", "Linkedin"],
                Kind::Linkedin,
            ),
            labelled("Preferred start date", &["Start Date"], Kind::StartDate),
            labelled(
                "Why do you want to work here",
                &["Why do you want to work here?"],
                Kind::WhyCompany,
            ),
            labelled(
                "How did you hear about us",
                &["How did you hear about us?"],
                Kind::HowDidYouHear,
            ),
            // Named so the plan can say Perch refuses them. Ashby renders these
            // as radio groups, which the fill will not touch whatever they are
            // called, and the words are checked as well.
            labelled("Gender", &["Gender"], Kind::Demographic),
            labelled(
                "Race / ethnicity",
                &["Race", "Ethnicity"],
                Kind::Demographic,
            ),
            labelled("Veteran status", &["Veteran Status"], Kind::Demographic),
            labelled(
                "Disability status",
                &["Disability Status"],
                Kind::Demographic,
            ),
        ],
        // Monitored, but there is no form Perch knows how to fill. The role
        // opens in the browser and the person fills it themselves.
        Ats::JsonLd => return None,
    };
    Some(Flavor { ats, fields })
}

/// The hosts an application form for this ATS is actually served from.
///
/// The URL Perch opens comes out of a board's own JSON, so it is remote data.
/// Typing a profile into whatever that data points at would trust a stranger
/// with where a résumé goes, so the fill only ever runs on a page whose origin
/// is on this list.
pub fn expected_hosts(ats: Ats) -> &'static [&'static str] {
    match ats {
        Ats::Greenhouse => &[
            "boards.greenhouse.io",
            "job-boards.greenhouse.io",
            "my.greenhouse.io",
        ],
        Ats::Lever => &["jobs.lever.co", "jobs.eu.lever.co"],
        Ats::Ashby => &["jobs.ashbyhq.com"],
        Ats::JsonLd => &[],
    }
}

/// Is this a page Perch is willing to type into?
///
/// The address is parsed with the same WHATWG implementation the webview and
/// reqwest use, rather than by splitting the string here. Two parsers that
/// disagree by one character is the whole vulnerability: WebKit ends the
/// authority at a backslash and a hand-written check does not, so
/// `https://evil.example\@boards.greenhouse.io/x` reads as Greenhouse to the
/// check and resolves to the attacker in the window. Sharing a parser means
/// the host Perch approves is by construction the host that gets loaded.
///
/// https only. The host list claims the page really is the board's, and that
/// claim rests entirely on TLS. Over plain http anyone on the network can
/// answer to `boards.greenhouse.io`, and the list would be checking a name the
/// attacker picked. A subdomain match is likewise not allowed:
/// `jobs.lever.co.evil.example` is somebody else's computer.
pub fn may_fill(ats: Ats, url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    // An application form never carries credentials. One that does is an
    // address built to read as one host to a person and resolve as another.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    parsed
        .host_str()
        .is_some_and(|host| expected_hosts(ats).contains(&host))
}

/// Is the page that just loaded the one the person reviewed?
///
/// The host is checked against the ATS's own list rather than against the
/// address the board handed out, because boards redirect: every
/// `boards.greenhouse.io` posting 301s to `job-boards.greenhouse.io`, and
/// comparing hosts literally would silently skip the fill on all of them.
///
/// The path still has to match. A redirect that lands somewhere else on the
/// same host (a careers index, a "this role has closed" page) is not the form
/// the person read, and gets nothing typed into it.
pub fn is_the_reviewed_page(ats: Ats, loaded: &str, intended: &str) -> bool {
    may_fill(ats, loaded) && form_path(loaded) == form_path(intended)
}

/// The path, normalised by the same parser, so `/a/b`, `/a/b/` and `/a/x/../b`
/// are one page rather than three.
fn form_path(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    Some(parsed.path().trim_end_matches('/').to_string())
}

/// A second line of defence, applied to every field whatever it claims to be.
///
/// The `Kind::Demographic` marking is a list someone maintains, and lists go
/// stale. This reads the label the form itself uses, so a question Perch has
/// never seen before still cannot be answered on the person's behalf.
pub fn label_is_demographic(label: &str) -> bool {
    let label = label.to_lowercase();
    DEMOGRAPHIC_MARKERS.iter().any(|m| label.contains(m))
}

/// The words that mark a question as one Perch will not answer.
///
/// Public because the fill script carries this same list into the page. A
/// board that names its boxes per posting is matched on the words beside them,
/// so the check has to run against the label the page actually shows, not
/// against the table Perch wrote. One list, read in both places.
pub const DEMOGRAPHIC_MARKERS: [&str; 16] = [
    "gender",
    "race",
    "ethnic",
    "veteran",
    "disability",
    "disabled",
    "sexual orientation",
    "pronoun",
    "hispanic",
    "latino",
    "national origin",
    "protected veteran",
    "eeo",
    "equal employment",
    "self-identif",
    "transgender",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fillable_ats_has_a_flavor_and_json_ld_has_none() {
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let flavor = for_ats(ats).unwrap_or_else(|| panic!("{ats:?} has no flavor"));
            assert!(!flavor.fields.is_empty());
        }
        // A board Perch can read but not fill opens in the browser instead.
        assert!(for_ats(Ats::JsonLd).is_none());
    }

    #[test]
    fn no_selector_names_a_field_by_position() {
        // A positional selector like Greenhouse's
        // `answers_attributes[0][text_value]` addresses whatever custom
        // question a particular job happens to list first, which may be a
        // demographic one. Neither the Kind marking nor the label check can
        // see that, because both read Perch's own table rather than the page.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            for f in for_ats(ats).unwrap().fields {
                assert!(
                    !f.selector.contains("[0]") && !f.selector.contains("attributes]"),
                    "{ats:?} field {:?} is addressed by position: {}",
                    f.label,
                    f.selector
                );
            }
        }
    }

    #[test]
    fn perch_only_types_into_a_page_the_ats_actually_serves() {
        assert!(may_fill(
            Ats::Greenhouse,
            "https://boards.greenhouse.io/figma/jobs/123"
        ));
        assert!(may_fill(Ats::Lever, "https://jobs.lever.co/oxide/abc"));
        assert!(may_fill(Ats::Ashby, "https://jobs.ashbyhq.com/valtown/x"));

        for hostile in [
            "https://jobs.lever.co.evil.example/x",
            "https://evil.example/jobs.lever.co",
            "https://jobs.ashbyhq.com.attacker.test/x",
            "https://user@evil.example/x",
            // A real host smuggled into the userinfo of a hostile one.
            "https://jobs.lever.co@evil.example/x",
            // A backslash ends the authority for the parser that does the
            // navigating, so the host here is evil.example and everything
            // after the backslash is path. A check that splits on `/` alone
            // reads it as the board and hands over the résumé.
            r"https://evil.example\@boards.greenhouse.io/x",
            r"https://evil.example\@jobs.lever.co/x",
            r"https://evil.example\.jobs.lever.co/x",
            // Cleartext. The host list is a claim about who is answering, and
            // without TLS anyone on the path can answer to any name.
            "http://boards.greenhouse.io/figma/jobs/123",
            "http://jobs.lever.co/oxide/abc",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "",
            "boards.greenhouse.io/figma",
        ] {
            assert!(!may_fill(Ats::Greenhouse, hostile), "{hostile} was allowed");
            assert!(!may_fill(Ats::Lever, hostile), "{hostile} was allowed");
        }

        // A board Perch watches but cannot fill has no page it may type into.
        assert!(!may_fill(Ats::JsonLd, "https://www.recurse.com/jobs"));
        // And an ATS never fills another's form.
        assert!(!may_fill(
            Ats::Lever,
            "https://boards.greenhouse.io/figma/jobs/1"
        ));
    }

    #[test]
    fn the_host_perch_approves_is_the_host_that_would_load() {
        // The bug this guards against is not a bad host list, it is two
        // parsers disagreeing. So the property under test is agreement: for
        // any address, whatever `may_fill` says must be a statement about the
        // host the webview would actually navigate to.
        for candidate in [
            r"https://evil.example\@boards.greenhouse.io/x",
            r"https://boards.greenhouse.io\@evil.example/x",
            "https://boards.greenhouse.io@evil.example/x",
            "https://boards.greenhouse.io/figma/jobs/1",
            "https://boards.greenhouse.io:443/figma/jobs/1",
            r"https://boards.greenhouse.io\..\evil.example/x",
            "https://BOARDS.greenhouse.io/figma/jobs/1",
        ] {
            let navigates_to = Url::parse(candidate).ok().and_then(|u| {
                (u.scheme() == "https").then(|| u.host_str().unwrap_or_default().to_string())
            });
            let allowed = may_fill(Ats::Greenhouse, candidate);
            let really_greenhouse = navigates_to
                .as_deref()
                .is_some_and(|h| expected_hosts(Ats::Greenhouse).contains(&h));
            assert_eq!(
                allowed,
                really_greenhouse,
                "{candidate} was {} but it loads {navigates_to:?}",
                if allowed { "allowed" } else { "refused" }
            );
        }
    }

    #[test]
    fn a_redirect_within_the_board_still_fills_but_another_page_does_not() {
        let intended = "https://boards.greenhouse.io/figma/jobs/123";
        // Every Greenhouse posting redirects to the newer host. Refusing this
        // would silently disable the fill on the whole board.
        assert!(is_the_reviewed_page(
            Ats::Greenhouse,
            "https://job-boards.greenhouse.io/figma/jobs/123",
            intended
        ));
        // A trailing slash is the same page.
        assert!(is_the_reviewed_page(
            Ats::Greenhouse,
            "https://job-boards.greenhouse.io/figma/jobs/123/",
            intended
        ));
        for elsewhere in [
            // A closed posting bouncing to the careers index.
            "https://boards.greenhouse.io/figma",
            // A different role on the same board.
            "https://boards.greenhouse.io/figma/jobs/999",
            // Off the board entirely.
            r"https://evil.example\@boards.greenhouse.io/figma/jobs/123",
        ] {
            assert!(
                !is_the_reviewed_page(Ats::Greenhouse, elsewhere, intended),
                "{elsewhere} was treated as the reviewed page"
            );
        }
    }

    #[test]
    fn ashby_addresses_its_boxes_the_way_ashby_names_them() {
        // Ashby gives its résumé box an id and no name at all, so a selector
        // written only against `name` matched nothing and no résumé was ever
        // attached on Ashby. Checked against a live Ashby form.
        let resume = for_ats(Ats::Ashby)
            .unwrap()
            .fields
            .into_iter()
            .find(|f| f.kind == Kind::Resume)
            .expect("Ashby has a résumé field");
        assert!(
            resume.selector.contains("input#_systemfield_resume"),
            "Ashby's résumé box is addressed by id, not by name: {}",
            resume.selector
        );
    }

    #[test]
    fn levers_link_boxes_are_matched_whatever_case_the_company_used() {
        // Lever names a URL box after its label, and the casing belongs to the
        // company. A live board writes `urls[Github]`, which a case-sensitive
        // selector for `urls[GitHub]` silently missed.
        for kind in [Kind::Github, Kind::Linkedin, Kind::Website] {
            let field = for_ats(Ats::Lever)
                .unwrap()
                .fields
                .into_iter()
                .find(|f| f.kind == kind)
                .unwrap_or_else(|| panic!("Lever has a {kind:?} field"));
            assert!(
                field.selector.contains("' i]"),
                "{kind:?} is matched case-sensitively: {}",
                field.selector
            );
        }
    }

    #[test]
    fn every_flavor_knows_which_of_its_fields_are_demographic() {
        // If an ATS's form has EEO questions, the flavor must mark them, or
        // the plan would treat them as ordinary fields it simply cannot fill.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let flavor = for_ats(ats).unwrap();
            assert!(
                flavor.fields.iter().any(|f| f.kind.is_demographic()),
                "{ats:?} marks no demographic fields"
            );
        }
    }

    #[test]
    fn the_label_check_catches_questions_no_list_anticipated() {
        for label in [
            "Gender",
            "What is your race?",
            "Are you a protected veteran?",
            "Disability status",
            "Voluntary Self-Identification of Disability",
            "Hispanic or Latino?",
            "Preferred pronouns",
            "Sexual orientation",
            "EEO information",
            "Do you identify as transgender?",
        ] {
            assert!(label_is_demographic(label), "{label:?} was not caught");
        }
    }

    #[test]
    fn ordinary_questions_are_not_mistaken_for_demographic_ones() {
        for label in [
            "First name",
            "Email",
            "Preferred start date",
            "Why do you want to work here",
            "How did you hear about us",
            "LinkedIn",
            "Résumé",
            "Current location",
        ] {
            assert!(!label_is_demographic(label), "{label:?} was wrongly caught");
        }
    }

    #[test]
    fn every_declared_demographic_field_is_also_caught_by_its_label() {
        // The two defences have to agree, or one of them is decorative.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            for f in for_ats(ats)
                .unwrap()
                .fields
                .iter()
                .filter(|f| f.kind.is_demographic())
            {
                assert!(
                    label_is_demographic(f.label),
                    "{ats:?} field {:?} is marked demographic but its label would not be caught",
                    f.label
                );
            }
        }
    }
}
