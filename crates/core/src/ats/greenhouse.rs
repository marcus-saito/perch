//! Greenhouse. The board's public JSON is enough to watch it, and Perch can
//! fill Greenhouse application forms, so this adapter declares both.

use super::AtsAdapter;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::model::{Ats, DetectedBoard, Listing, RemoteRole};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct Greenhouse;

const API: &str = "https://boards-api.greenhouse.io/v1/boards";

impl Greenhouse {
    /// Pull a board token out of whatever the person typed. Accepts a bare
    /// token, a boards.greenhouse.io URL, a job-boards.greenhouse.io URL, or a
    /// company name that happens to be the token.
    fn token_from(input: &str) -> Option<String> {
        let trimmed = input.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }

        if let Some(rest) = trimmed.split("greenhouse.io/").nth(1) {
            // The embed URL companies paste into their own careers page names
            // the board in a query parameter, not in the path.
            if let Some(query) = rest.split('?').nth(1) {
                for pair in query.split('&') {
                    if let Some(value) = pair.strip_prefix("for=") {
                        let value = value.split('#').next().unwrap_or(value);
                        if !value.is_empty() {
                            return Some(value.to_ascii_lowercase());
                        }
                    }
                }
            }
            let token = rest.split(['/', '?', '#']).find(|segment| {
                !segment.is_empty() && *segment != "embed" && *segment != "job_board"
            })?;
            return Some(token.to_ascii_lowercase());
        }

        // Not a Greenhouse URL, and not something we can guess a token from.
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
        format!("https://boards.greenhouse.io/{token}")
    }
}

impl AtsAdapter for Greenhouse {
    fn ats(&self) -> Ats {
        Ats::Greenhouse
    }

    fn fill_supported(&self) -> bool {
        true
    }

    fn detect(&self, input: &str, http: &Http) -> Result<Option<DetectedBoard>> {
        let Some(token) = Self::token_from(input) else {
            return Ok(None);
        };
        let url = format!("{API}/{token}/jobs");
        let Some(body) = http.get_json(&url)? else {
            return Ok(None);
        };

        // A board that exists but has nothing open is still worth watching.
        let jobs = body.get("jobs").and_then(|j| j.as_array());
        if jobs.is_none() {
            return Ok(None);
        }

        let company_name = jobs
            .and_then(|jobs| jobs.first())
            .and_then(|job| job.get("company_name"))
            .and_then(|name| name.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| token.clone());

        Ok(Some(DetectedBoard {
            ats: Ats::Greenhouse,
            token: token.clone(),
            url: Self::board_url(&token),
            company_name,
            fill_supported: self.fill_supported(),
        }))
    }

    fn fetch(&self, token: &str, http: &Http) -> Result<Listing> {
        // Listings only. The description is an order of magnitude more bytes
        // and is not needed until someone opens the role, so it is fetched then.
        let url = format!("{API}/{token}/jobs");

        // A board that is not there has told us nothing, which is not the same
        // as a board saying it has nothing open. Only the latter may close
        // roles, so the former has to be an error.
        let Some(body) = http.get_json(&url)? else {
            return Err(Error::BoardUnreadable(token.to_string()));
        };
        let Some(jobs) = body.get("jobs").and_then(|j| j.as_array()) else {
            return Err(Error::BoardUnreadable(token.to_string()));
        };

        let mut listing = Listing::default();
        for job in jobs {
            if let Some(id) = external_id(job) {
                // Recorded even when the entry will not parse: the board is
                // still listing it, so it has not come down.
                listing.listed_ids.push(id);
            }
            if let Some(role) = parse_job(job) {
                listing.roles.push(role);
            }
        }
        Ok(listing)
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
        Ok(body
            .get("content")
            .and_then(|c| c.as_str())
            .map(str::to_string))
    }
}

impl Greenhouse {
    fn description_url(token: &str, external_id: &str) -> String {
        format!("{API}/{token}/jobs/{external_id}")
    }
}

fn external_id(job: &serde_json::Value) -> Option<String> {
    match job.get("id") {
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn parse_time(value: Option<&serde_json::Value>) -> Option<OffsetDateTime> {
    let text = value?.as_str()?;
    OffsetDateTime::parse(text, &Rfc3339).ok()
}

fn parse_job(job: &serde_json::Value) -> Option<RemoteRole> {
    let external_id = external_id(job)?;
    let title = job.get("title")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let location = job
        .get("location")
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let url = job
        .get("absolute_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    Some(RemoteRole {
        external_id,
        title,
        location,
        url,
        posted_at: parse_time(job.get("first_published")),
        updated_at: parse_time(job.get("updated_at")),
        // Greenhouse's listing has no description in it: it is fetched per
        // posting when someone opens the role.
        description: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_out_of_whatever_someone_types() {
        let t = Greenhouse::token_from;
        assert_eq!(t("figma").as_deref(), Some("figma"));
        assert_eq!(t("Val Town").as_deref(), Some("valtown"));
        assert_eq!(t("Fly.io").as_deref(), Some("flyio"));
        assert_eq!(
            t("https://boards.greenhouse.io/figma").as_deref(),
            Some("figma")
        );
        assert_eq!(t("boards.greenhouse.io/figma/").as_deref(), Some("figma"));
        assert_eq!(
            t("https://boards.greenhouse.io/embed/job_board?for=figma").as_deref(),
            Some("figma")
        );
        assert_eq!(
            t("https://job-boards.greenhouse.io/figma/jobs/5364702004").as_deref(),
            Some("figma")
        );
    }

    #[test]
    fn a_non_greenhouse_url_is_not_this_adapters_problem() {
        assert_eq!(Greenhouse::token_from("https://jobs.lever.co/oxide"), None);
        assert_eq!(Greenhouse::token_from("https://example.com/careers"), None);
        assert_eq!(Greenhouse::token_from(""), None);
        assert_eq!(Greenhouse::token_from("   "), None);
    }

    #[test]
    fn a_job_parses_out_of_the_real_payload_shape() {
        let job: serde_json::Value = serde_json::from_str(
            r#"{
                "absolute_url": "https://boards.greenhouse.io/figma/jobs/5364702004",
                "id": 5364702004,
                "location": { "name": "Berlin, Germany" },
                "title": "  Account Executive  ",
                "updated_at": "2026-07-22T05:37:08-04:00",
                "first_published": "2024-11-01T06:05:10-04:00"
            }"#,
        )
        .unwrap();
        let role = parse_job(&job).unwrap();
        assert_eq!(role.external_id, "5364702004");
        assert_eq!(role.title, "Account Executive");
        assert_eq!(role.location, "Berlin, Germany");
        assert!(role.posted_at.is_some());
        assert!(role.updated_at.is_some());
    }

    #[test]
    fn an_entry_that_will_not_parse_still_counts_as_listed() {
        // Otherwise the next sync reports it as having come down, and writes
        // a closure into the history that never happened.
        let job: serde_json::Value = serde_json::from_str(r#"{"id": 7, "title": "   "}"#).unwrap();
        assert!(parse_job(&job).is_none());
        assert_eq!(external_id(&job).as_deref(), Some("7"));
    }

    #[test]
    fn a_job_missing_the_parts_that_matter_is_skipped_not_guessed_at() {
        let no_title: serde_json::Value = serde_json::from_str(r#"{"id": 1}"#).unwrap();
        assert!(parse_job(&no_title).is_none());

        let no_id: serde_json::Value = serde_json::from_str(r#"{"title": "Engineer"}"#).unwrap();
        assert!(parse_job(&no_id).is_none());

        let thin: serde_json::Value =
            serde_json::from_str(r#"{"id": 7, "title": "Engineer"}"#).unwrap();
        let role = parse_job(&thin).unwrap();
        assert_eq!(role.location, "");
        assert!(role.posted_at.is_none());
    }
}
