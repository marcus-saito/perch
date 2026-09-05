//! Lever. The public postings API is enough to watch a board, and perch-fill
//! knows the Lever form, so this adapter declares both.

use super::AtsAdapter;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::model::{Ats, DetectedBoard, Listing, RemoteRole};
use time::OffsetDateTime;

pub struct Lever;

const API: &str = "https://api.lever.co/v0/postings";

impl Lever {
    /// Pull a board token out of whatever the person typed. Accepts a bare
    /// token, a jobs.lever.co board or posting URL, the apply URL under a
    /// posting, or a company name that happens to be the token.
    fn token_from(input: &str) -> Option<String> {
        let trimmed = input.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }

        if Self::on_the_eu_instance(trimmed) {
            return None;
        }

        if let Some(rest) = trimmed.split("lever.co/").nth(1) {
            // On the board and posting URLs the token is the first segment. On
            // the API URL two fixed segments come first.
            let token = rest.split(['/', '?', '#']).find(|segment| {
                !segment.is_empty() && *segment != "v0" && *segment != "postings"
            })?;
            return Some(token.to_ascii_lowercase());
        }

        // Not a Lever URL, and not something we can guess a token from. This
        // is what keeps a Greenhouse or Ashby address from being answered
        // here: the next adapter gets its turn.
        if trimmed.contains("://") || trimmed.contains('/') {
            return None;
        }

        // A plain name: "Val Town" and "valtown" are both worth trying.
        let token: String = trimmed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        (!token.is_empty()).then_some(token)
    }

    /// Whether the address names Lever's European instance.
    ///
    /// The two instances are separate namespaces: a token on one may be absent
    /// from the other, or name a different company's board. A board is stored
    /// as an ATS and a token with nowhere to keep which instance it came from,
    /// so reading an EU address off api.lever.co would watch a board nobody
    /// asked for. Catches api.eu.lever.co as well as jobs.eu.lever.co.
    fn on_the_eu_instance(input: &str) -> bool {
        input.to_ascii_lowercase().contains("eu.lever.co")
    }

    fn list_url(token: &str) -> String {
        format!("{API}/{token}?mode=json")
    }

    fn board_url(token: &str) -> String {
        format!("https://jobs.lever.co/{token}")
    }
}

impl AtsAdapter for Lever {
    fn ats(&self) -> Ats {
        Ats::Lever
    }

    fn fill_supported(&self) -> bool {
        true
    }

    fn detect(&self, input: &str, http: &Http) -> Result<Option<DetectedBoard>> {
        // Named rather than declined: declining ends as "no public board
        // found" for an address that names a real board.
        if Self::on_the_eu_instance(input) {
            return Err(Error::msg(
                "that is a board on Lever's European instance, which Perch cannot read yet",
            ));
        }
        let Some(token) = Self::token_from(input) else {
            return Ok(None);
        };
        let Some(body) = http.get_json(&Self::list_url(&token))? else {
            return Ok(None);
        };

        // Lever answers with a bare array. A board that exists but has nothing
        // open answers with an empty one, and a company with nothing open
        // right now is still worth watching.
        if !body.is_array() {
            return Ok(None);
        }

        Ok(Some(DetectedBoard {
            ats: Ats::Lever,
            token: token.clone(),
            url: Self::board_url(&token),
            // Lever's postings carry no company name field. The name the
            // person typed is kept when they typed one, and the token stands
            // in when they gave an address.
            company_name: super::name_as_typed(input, &token),
            fill_supported: self.fill_supported(),
        }))
    }

    fn fetch(&self, token: &str, http: &Http) -> Result<Listing> {
        // Lever sends every description with the listing and offers no leaner
        // mode, so they are kept. Dropping them would mean fetching the whole
        // board again to read one role.
        let url = Self::list_url(token);

        // A board that is not there has told us nothing, which is not the same
        // as a board saying it has nothing open. Only the latter may close
        // roles, so the former has to be an error.
        let Some(body) = http.get_json(&url)? else {
            return Err(Error::BoardUnreadable(token.to_string()));
        };
        listing_from(token, &body)
    }

    fn fetch_description(
        &self,
        token: &str,
        external_id: &str,
        http: &Http,
    ) -> Result<Option<String>> {
        let url = Self::description_url(token, external_id);
        let Some(body) = http.get_json(&url)? else {
            return Ok(None);
        };
        Ok(description(&body))
    }
}

impl Lever {
    fn description_url(token: &str, external_id: &str) -> String {
        format!("{API}/{token}/{external_id}")
    }
}

/// The board's answer turned into a listing. Kept out of
/// [`AtsAdapter::fetch`] so the empty board, which is the case that decides
/// whether roles get closed, can be tested without the network.
fn listing_from(token: &str, body: &serde_json::Value) -> Result<Listing> {
    // The postings arrive as a bare array, not under a key. Anything else is
    // not a board answering.
    let Some(postings) = body.as_array() else {
        return Err(Error::BoardUnreadable(token.to_string()));
    };

    let mut listing = Listing::default();
    for posting in postings {
        if let Some(id) = external_id(posting) {
            // Recorded even when the entry will not parse: the board is still
            // listing it, so it has not come down.
            listing.listed_ids.push(id);
        }
        if let Some(role) = parse_posting(posting) {
            listing.roles.push(role);
        }
    }
    Ok(listing)
}

fn external_id(posting: &serde_json::Value) -> Option<String> {
    let id = posting.get("id")?.as_str()?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Lever states its times as whole milliseconds since the epoch, where the
/// other boards send RFC3339 text.
fn parse_time(value: Option<&serde_json::Value>) -> Option<OffsetDateTime> {
    let millis = value?.as_i64()?;
    OffsetDateTime::from_unix_timestamp_nanos(millis as i128 * 1_000_000).ok()
}

/// The posting's own words, as HTML. descriptionPlain is the same words with
/// the markup stripped, and Perch renders the markup.
///
/// An empty description is left as None. Stored as Some(""), it would count as
/// the posting's words: the detail pane would show a blank where they go, with
/// nothing left to fetch them again. Both the listing and the single posting
/// read the field through here, so neither path can store one.
fn description(posting: &serde_json::Value) -> Option<String> {
    let html = posting.get("description")?.as_str()?;
    (!html.trim().is_empty()).then(|| html.to_string())
}

fn parse_posting(posting: &serde_json::Value) -> Option<RemoteRole> {
    let external_id = external_id(posting)?;
    // Lever names the title field "text".
    let title = posting.get("text")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let location = posting
        .get("categories")
        .and_then(|c| c.get("location"))
        .and_then(|l| l.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // applyUrl is the page carrying the application form, and this is the
    // address perch-fill types into. Greenhouse's absolute_url is the form
    // page too, so both adapters store the same kind of address. hostedUrl is
    // the posting on its own, used when a board sends no applyUrl.
    let url = posting
        .get("applyUrl")
        .and_then(|u| u.as_str())
        .or_else(|| posting.get("hostedUrl").and_then(|u| u.as_str()))
        .unwrap_or("")
        .to_string();

    Some(RemoteRole {
        external_id,
        title,
        location,
        url,
        posted_at: parse_time(posting.get("createdAt")),
        // Most Lever postings carry no updatedAt at all. The ones that do
        // state it the same way as createdAt.
        updated_at: parse_time(posting.get("updatedAt")),
        description: description(posting),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_out_of_whatever_someone_types() {
        let t = Lever::token_from;
        assert_eq!(t("oxide").as_deref(), Some("oxide"));
        assert_eq!(t("Val Town").as_deref(), Some("valtown"));
        assert_eq!(t("Fly.io").as_deref(), Some("flyio"));
        assert_eq!(t("https://jobs.lever.co/oxide").as_deref(), Some("oxide"));
        assert_eq!(t("jobs.lever.co/oxide/").as_deref(), Some("oxide"));
        assert_eq!(
            t("https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e").as_deref(),
            Some("leverdemo")
        );
        assert_eq!(
            t("https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e/a").as_deref(),
            Some("leverdemo")
        );
        assert_eq!(
            t("https://api.lever.co/v0/postings/oxide?mode=json").as_deref(),
            Some("oxide")
        );
    }

    #[test]
    fn a_url_from_another_ats_is_not_this_adapters_problem() {
        // Whichever adapter owns the address has to be the one that answers,
        // so this one has to decline rather than guess a token.
        assert_eq!(
            Lever::token_from("https://boards.greenhouse.io/figma"),
            None
        );
        assert_eq!(
            Lever::token_from("https://job-boards.greenhouse.io/figma/jobs/5364702004"),
            None
        );
        assert_eq!(Lever::token_from("https://jobs.ashbyhq.com/cursor"), None);
        assert_eq!(Lever::token_from("https://example.com/careers"), None);
        assert_eq!(Lever::token_from(""), None);
        assert_eq!(Lever::token_from("   "), None);
    }

    #[test]
    fn an_address_on_levers_european_instance_is_not_read_off_the_us_one() {
        // The two instances list different boards, so answering an EU address
        // from api.lever.co watches a company nobody asked for.
        let t = Lever::token_from;
        assert_eq!(t("https://jobs.eu.lever.co/ovoko"), None);
        assert_eq!(t("https://jobs.eu.lever.co/oxide/abc/apply"), None);
        assert_eq!(t("jobs.eu.lever.co/ovoko"), None);
        assert_eq!(
            t("https://api.eu.lever.co/v0/postings/ovoko?mode=json"),
            None
        );
        assert_eq!(
            t("https://jobs.lever.co/leverdemo").as_deref(),
            Some("leverdemo")
        );
    }

    #[test]
    fn a_european_board_is_named_as_one_rather_than_reported_missing() {
        // The address names a real board, so "no public board found" would be
        // untrue. Nothing is requested: the answer is settled by the host.
        let http = Http::new().unwrap();
        let err = Lever
            .detect("https://jobs.eu.lever.co/ovoko", &http)
            .expect_err("an EU address is refused, not answered from the US instance");
        assert!(
            err.to_string().contains("European instance"),
            "the message has to say which instance it is: {err}"
        );
    }

    fn a_real_posting() -> serde_json::Value {
        serde_json::from_str(
            r#"{
                "id": "33538a2f-d27d-4a96-8f05-fa4b0e4d940e",
                "text": "  AbelsonTaylor Writer  ",
                "categories": {
                    "commitment": "Regular Full Time (Salary)",
                    "department": "Customer Success",
                    "location": "Arlington, TX",
                    "team": "Professional Services",
                    "allLocations": ["Arlington, TX"]
                },
                "country": "US",
                "workplaceType": "hybrid",
                "createdAt": 1553186035299,
                "description": "<div>Welcome to the <b>Demo Job Listing</b></div>",
                "descriptionPlain": "Welcome to the Demo Job Listing",
                "lists": [],
                "additional": "",
                "hostedUrl": "https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e",
                "applyUrl": "https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e/apply"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_posting_parses_out_of_the_real_payload_shape() {
        let role = parse_posting(&a_real_posting()).unwrap();
        assert_eq!(role.external_id, "33538a2f-d27d-4a96-8f05-fa4b0e4d940e");
        assert_eq!(role.title, "AbelsonTaylor Writer");
        assert_eq!(role.location, "Arlington, TX");
        assert_eq!(
            role.url,
            "https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e/apply"
        );
        assert_eq!(
            role.description.as_deref(),
            Some("<div>Welcome to the <b>Demo Job Listing</b></div>")
        );
        assert!(role.updated_at.is_none());
    }

    #[test]
    fn a_created_at_in_epoch_milliseconds_lands_on_the_date_lever_means() {
        let role = parse_posting(&a_real_posting()).unwrap();
        let posted = role.posted_at.unwrap();
        assert_eq!(posted.year(), 2019);
        assert_eq!(posted.month(), time::Month::March);
        assert_eq!(posted.day(), 21);
        assert_eq!(posted.hour(), 16);
        assert_eq!(posted.minute(), 33);
        assert_eq!(posted.second(), 55);
        assert_eq!(posted.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn the_address_stored_is_the_one_carrying_the_form() {
        let mut posting = a_real_posting();
        posting.as_object_mut().unwrap().remove("applyUrl");
        let role = parse_posting(&posting).unwrap();
        assert_eq!(
            role.url,
            "https://jobs.lever.co/leverdemo/33538a2f-d27d-4a96-8f05-fa4b0e4d940e"
        );
    }

    #[test]
    fn an_entry_that_will_not_parse_still_counts_as_listed() {
        // Otherwise the next sync reports it as having come down, and writes
        // a closure into the history that never happened.
        let body: serde_json::Value =
            serde_json::from_str(r#"[{"id": "7a1b", "text": "   "}]"#).unwrap();
        let listing = listing_from("leverdemo", &body).unwrap();
        assert!(listing.roles.is_empty());
        assert_eq!(listing.listed_ids, vec!["7a1b".to_string()]);
    }

    #[test]
    fn a_posting_missing_the_parts_that_matter_is_skipped_not_guessed_at() {
        let no_title: serde_json::Value = serde_json::from_str(r#"{"id": "7a1b"}"#).unwrap();
        assert!(parse_posting(&no_title).is_none());

        let no_id: serde_json::Value = serde_json::from_str(r#"{"text": "Engineer"}"#).unwrap();
        assert!(parse_posting(&no_id).is_none());

        let thin: serde_json::Value =
            serde_json::from_str(r#"{"id": "7a1b", "text": "Engineer"}"#).unwrap();
        let role = parse_posting(&thin).unwrap();
        assert_eq!(role.location, "");
        assert_eq!(role.url, "");
        assert!(role.posted_at.is_none());
        assert!(role.description.is_none());
    }

    #[test]
    fn a_description_the_board_left_blank_is_not_a_description_on_either_path() {
        // Ten of leverdemo's postings carry "description": "". Stored as
        // Some(""), the role reads as having words that are blank and is never
        // fetched again, so the listing and the single posting both have to
        // come out as no description at all.
        let blank: serde_json::Value =
            serde_json::from_str(r#"{"id": "7a1b", "text": "Engineer", "description": "   "}"#)
                .unwrap();
        assert!(description(&blank).is_none());
        assert!(parse_posting(&blank).unwrap().description.is_none());
    }

    #[test]
    fn an_empty_board_is_an_empty_listing_and_not_an_error() {
        // Lever answers a board with nothing open with []. That is the board
        // saying so, which is the only thing allowed to close roles.
        let body: serde_json::Value = serde_json::from_str("[]").unwrap();
        let listing = listing_from("lever", &body).unwrap();
        assert!(listing.roles.is_empty());
        assert!(listing.listed_ids.is_empty());
    }

    #[test]
    fn an_answer_that_is_not_an_array_is_unreadable_rather_than_empty() {
        let body: serde_json::Value = serde_json::from_str(r#"{"jobs": []}"#).unwrap();
        assert!(matches!(
            listing_from("leverdemo", &body),
            Err(Error::BoardUnreadable(_))
        ));
    }

    #[test]
    fn the_urls_addressed_are_the_ones_lever_publishes() {
        assert_eq!(
            Lever::list_url("leverdemo"),
            "https://api.lever.co/v0/postings/leverdemo?mode=json"
        );
        assert_eq!(
            Lever::description_url("leverdemo", "33538a2f"),
            "https://api.lever.co/v0/postings/leverdemo/33538a2f"
        );
        assert_eq!(
            Lever::board_url("leverdemo"),
            "https://jobs.lever.co/leverdemo"
        );
    }
}
