//! Ashby. The job-board API is enough to watch a board, and perch-fill
//! knows the Ashby form, so this adapter declares both.

use super::AtsAdapter;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::model::{Ats, DetectedBoard, Listing, RemoteRole};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct Ashby;

const API: &str = "https://api.ashbyhq.com/posting-api/job-board";

impl Ashby {
    /// Pull a board token out of whatever the person typed. Accepts a bare
    /// token, a jobs.ashbyhq.com board, posting or apply URL, or a company
    /// name that happens to be the token.
    fn token_from(input: &str) -> Option<String> {
        let trimmed = input.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }

        if let Some(rest) = trimmed.split("ashbyhq.com/").nth(1) {
            // On the board and posting URLs the token is the first segment.
            // On the API URL two fixed segments come first.
            let token = rest.split(['/', '?', '#']).find(|segment| {
                !segment.is_empty() && *segment != "posting-api" && *segment != "job-board"
            })?;
            return Some(token.to_ascii_lowercase());
        }

        // Not an Ashby URL, and not something we can guess a token from. This
        // is what returns None for a Greenhouse or Lever address, so the next
        // adapter gets its turn.
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

    fn board_url(token: &str) -> String {
        format!("https://jobs.ashbyhq.com/{token}")
    }

    fn listing_url(token: &str) -> String {
        format!("{API}/{token}")
    }
}

impl AtsAdapter for Ashby {
    fn ats(&self) -> Ats {
        Ats::Ashby
    }

    fn fill_supported(&self) -> bool {
        true
    }

    fn detect(&self, input: &str, http: &Http) -> Result<Option<DetectedBoard>> {
        let Some(token) = Self::token_from(input) else {
            return Ok(None);
        };
        let Some(body) = http.get_json(&Self::listing_url(&token))? else {
            return Ok(None);
        };

        // A board that exists but has nothing open is still worth watching.
        if body.get("jobs").and_then(|j| j.as_array()).is_none() {
            return Ok(None);
        }

        Ok(Some(DetectedBoard {
            ats: Ats::Ashby,
            token: token.clone(),
            url: Self::board_url(&token),
            // Ashby's payload never names the company. The name the person
            // typed is kept when they typed one, and the token stands in when
            // they gave an address.
            company_name: super::name_as_typed(input, &token),
            fill_supported: self.fill_supported(),
        }))
    }

    fn fetch(&self, token: &str, http: &Http) -> Result<Listing> {
        // Ashby sends every description with the listing and offers no leaner
        // mode, so a sync carries them whether or not anyone reads them. They
        // are kept rather than dropped: refetching a board of several
        // megabytes to read one posting would cost more than holding them.
        let url = Self::listing_url(token);

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
        // Ashby has no per-posting endpoint: the two addresses that would be
        // one answer 401. So the whole board is read and the posting picked
        // out of it. The description already arrives with the listing, which
        // leaves this as the fallback for a role stored without one.
        let Some(body) = http.get_json(&Self::listing_url(token))? else {
            return Ok(None);
        };
        let Some(jobs) = body.get("jobs").and_then(|j| j.as_array()) else {
            return Ok(None);
        };
        Ok(jobs
            .iter()
            .find(|job| id_of(job).as_deref() == Some(external_id))
            .and_then(description))
    }
}

fn listing_from(token: &str, body: &serde_json::Value) -> Result<Listing> {
    let Some(jobs) = body.get("jobs").and_then(|j| j.as_array()) else {
        return Err(Error::BoardUnreadable(token.to_string()));
    };

    let mut listing = Listing::default();
    for job in jobs {
        if let Some(id) = id_of(job) {
            // Recorded even when the entry will not parse, and even when the
            // board keeps it off itself: the board is still naming the
            // posting, so it has not come down.
            listing.listed_ids.push(id);
        }
        if !is_listed(job) {
            continue;
        }
        if let Some(role) = parse_job(job) {
            listing.roles.push(role);
        }
    }
    Ok(listing)
}

/// Whether the board shows this posting publicly.
///
/// Ashby lets a company publish a posting without putting it on its board, for
/// a search it is running quietly or a link it hands out itself. Perch reports
/// what a board shows, so an unlisted posting is not a role here. Its id still
/// goes into `listed_ids`, because the closure rule is absence from the
/// board's own list: the board is still naming this one, and a posting that
/// alternates between listed and not would otherwise be written into the
/// history as closing and reopening, which is not what happened to it. When
/// the posting really does come down its id stops arriving and it closes then.
///
/// A payload that does not carry the field is taken as listed, because only an
/// explicit false is the board saying it withheld the posting.
fn is_listed(job: &serde_json::Value) -> bool {
    job.get("isListed")
        .and_then(|listed| listed.as_bool())
        .unwrap_or(true)
}

fn id_of(job: &serde_json::Value) -> Option<String> {
    let id = job.get("id")?.as_str()?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn parse_time(value: Option<&serde_json::Value>) -> Option<OffsetDateTime> {
    let text = value?.as_str()?;
    OffsetDateTime::parse(text, &Rfc3339).ok()
}

/// An empty description is left as None. Stored as Some(""), it would count as
/// the posting's words and the detail pane would show a blank where the words
/// go, with nothing left to fetch them again.
fn description(job: &serde_json::Value) -> Option<String> {
    let html = job.get("descriptionHtml")?.as_str()?;
    (!html.trim().is_empty()).then(|| html.to_string())
}

fn parse_job(job: &serde_json::Value) -> Option<RemoteRole> {
    let external_id = id_of(job)?;
    let title = job.get("title")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let location = job
        .get("location")
        .and_then(|l| l.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // applyUrl is the page carrying the form, and role.url is both the page
    // Perch opens and the page the fill types into. jobUrl describes the
    // posting a click short of the form, so it is the fallback.
    let url = job
        .get("applyUrl")
        .and_then(|u| u.as_str())
        .or_else(|| job.get("jobUrl").and_then(|u| u.as_str()))
        .unwrap_or("")
        .to_string();

    Some(RemoteRole {
        external_id,
        title,
        location,
        url,
        posted_at: parse_time(job.get("publishedAt")),
        // Ashby's payload carries no field for this, and Perch does not guess
        // at a date it was not given.
        updated_at: None,
        description: description(job),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_out_of_whatever_someone_types() {
        let t = Ashby::token_from;
        assert_eq!(t("linear").as_deref(), Some("linear"));
        assert_eq!(t("Val Town").as_deref(), Some("valtown"));
        assert_eq!(t("Fly.io").as_deref(), Some("flyio"));
        assert_eq!(
            t("https://jobs.ashbyhq.com/linear").as_deref(),
            Some("linear")
        );
        assert_eq!(t("jobs.ashbyhq.com/linear/").as_deref(), Some("linear"));
        assert_eq!(
            t("https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff").as_deref(),
            Some("linear")
        );
        assert_eq!(
            t("https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application")
                .as_deref(),
            Some("linear")
        );
        assert_eq!(
            t("https://jobs.ashbyhq.com/linear?embed=js").as_deref(),
            Some("linear")
        );
        assert_eq!(
            t("https://api.ashbyhq.com/posting-api/job-board/linear").as_deref(),
            Some("linear")
        );
    }

    #[test]
    fn a_url_from_another_ats_is_not_this_adapters_problem() {
        // Every adapter is asked in turn, so saying no to someone else's board
        // is what lets the right one answer.
        assert_eq!(Ashby::token_from("https://jobs.lever.co/oxide"), None);
        assert_eq!(
            Ashby::token_from("https://jobs.lever.co/leverdemo/f2f01e16/a"),
            None
        );
        assert_eq!(
            Ashby::token_from("https://boards.greenhouse.io/figma"),
            None
        );
        assert_eq!(
            Ashby::token_from("https://job-boards.greenhouse.io/figma/jobs/5364702004"),
            None
        );
        assert_eq!(Ashby::token_from("https://example.com/careers"), None);
        assert_eq!(Ashby::token_from(""), None);
        assert_eq!(Ashby::token_from("   "), None);
    }

    fn payload(jobs: &str) -> serde_json::Value {
        serde_json::from_str(&format!(r#"{{ "jobs": {jobs}, "apiVersion": "1" }}"#)).unwrap()
    }

    fn one_job() -> serde_json::Value {
        serde_json::from_str(
            r#"{
                "id": "d3bc1ced-3ce4-4086-a050-555055dbb1ff",
                "title": "  Senior / Staff Fullstack Engineer  ",
                "department": "Engineering",
                "team": "Core",
                "employmentType": "FullTime",
                "location": "Europe",
                "publishedAt": "2021-04-27T20:13:45.158+00:00",
                "isListed": true,
                "isRemote": true,
                "jobUrl": "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
                "applyUrl": "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application",
                "descriptionHtml": "<p>Linear is building.</p>",
                "descriptionPlain": "Linear is building."
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_job_parses_out_of_the_real_payload_shape() {
        let role = parse_job(&one_job()).unwrap();
        assert_eq!(role.external_id, "d3bc1ced-3ce4-4086-a050-555055dbb1ff");
        assert_eq!(role.title, "Senior / Staff Fullstack Engineer");
        assert_eq!(role.location, "Europe");
        assert_eq!(
            role.url,
            "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application"
        );
        assert_eq!(
            role.description.as_deref(),
            Some("<p>Linear is building.</p>")
        );
        // The milliseconds and the +00:00 offset are what the board really
        // sends, and both have to go through the same parse Greenhouse uses.
        let posted = role.posted_at.unwrap();
        assert_eq!(posted.year(), 2021);
        assert_eq!(posted.unix_timestamp(), 1619554425);
        // There is no such field in an Ashby payload.
        assert!(role.updated_at.is_none());
    }

    #[test]
    fn the_url_perch_opens_is_the_form_and_falls_back_to_the_posting_page() {
        let no_apply_url: serde_json::Value = serde_json::from_str(
            r#"{
                "id": "abc",
                "title": "Engineer",
                "jobUrl": "https://jobs.ashbyhq.com/linear/abc"
            }"#,
        )
        .unwrap();
        let role = parse_job(&no_apply_url).unwrap();
        assert_eq!(role.url, "https://jobs.ashbyhq.com/linear/abc");
    }

    #[test]
    fn an_entry_that_will_not_parse_still_counts_as_listed() {
        // Otherwise the next sync reports it as having come down, and writes
        // a closure into the history that never happened.
        let body = payload(r#"[{ "id": "abc", "title": "   " }]"#);
        let listing = listing_from("linear", &body).unwrap();
        assert!(listing.roles.is_empty());
        assert_eq!(listing.listed_ids, vec!["abc".to_string()]);
    }

    #[test]
    fn a_posting_the_board_keeps_off_itself_is_not_a_role_but_is_still_listed() {
        let body = payload(
            r#"[
                { "id": "shown", "title": "Engineer", "isListed": true },
                { "id": "hidden", "title": "Chief of Staff", "isListed": false },
                { "id": "unsaid", "title": "Designer" }
            ]"#,
        );
        let listing = listing_from("linear", &body).unwrap();

        let ids: Vec<&str> = listing
            .roles
            .iter()
            .map(|r| r.external_id.as_str())
            .collect();
        assert_eq!(ids, vec!["shown", "unsaid"]);
        // The unlisted one is still named by the board, so closing it would be
        // recording something Perch did not see happen.
        assert_eq!(listing.listed_ids, vec!["shown", "hidden", "unsaid"]);
    }

    #[test]
    fn an_empty_board_is_an_empty_listing_rather_than_an_error() {
        // The board answered and said it has nothing open. That is the one
        // thing Perch may close every role on.
        let listing = listing_from("deel", &payload("[]")).unwrap();
        assert!(listing.roles.is_empty());
        assert!(listing.listed_ids.is_empty());
    }

    #[test]
    fn an_answer_with_no_jobs_array_is_unreadable_rather_than_empty() {
        let body: serde_json::Value = serde_json::from_str(r#"{ "apiVersion": "1" }"#).unwrap();
        assert!(matches!(
            listing_from("linear", &body),
            Err(Error::BoardUnreadable(_))
        ));
    }

    #[test]
    fn a_job_missing_the_parts_that_matter_is_skipped_not_guessed_at() {
        let no_title: serde_json::Value = serde_json::from_str(r#"{"id": "abc"}"#).unwrap();
        assert!(parse_job(&no_title).is_none());

        let no_id: serde_json::Value = serde_json::from_str(r#"{"title": "Engineer"}"#).unwrap();
        assert!(parse_job(&no_id).is_none());

        let thin: serde_json::Value =
            serde_json::from_str(r#"{"id": "abc", "title": "Engineer"}"#).unwrap();
        let role = parse_job(&thin).unwrap();
        assert_eq!(role.location, "");
        assert_eq!(role.url, "");
        assert!(role.posted_at.is_none());
        assert!(role.description.is_none());
    }

    #[test]
    fn a_description_the_board_left_blank_is_not_a_description() {
        let blank: serde_json::Value =
            serde_json::from_str(r#"{"id": "abc", "title": "Engineer", "descriptionHtml": "   "}"#)
                .unwrap();
        assert!(parse_job(&blank).unwrap().description.is_none());
    }
}
