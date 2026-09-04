//! Perch is a local-first job search companion.
//!
//! Everything here runs on one machine against public job board APIs. There is
//! no account, no backend and no telemetry, and nothing in this crate can
//! submit an application: the fill layer has no submit primitive, so the
//! guarantee holds by construction rather than by policy.

pub mod ats;
pub mod error;
pub mod html;
pub mod http;
pub mod model;
pub mod paths;
pub mod profile;
pub mod rules;
pub mod store;
pub mod sync;
pub mod timesignal;

pub use error::{Error, Result, TomlComplaint};
pub use html::Block;
pub use http::Http;
pub use model::{
    Application, ApplicationState, Ats, Board, Company, DetectedBoard, EventKind, RemoteRole, Role,
    RoleEvent, Velocity,
};
pub use paths::Paths;
pub use profile::{Position, Profile};
pub use rules::{Rule, Rules, Why};
pub use store::{FeedFilter, Store, SyncReport};
pub use timesignal::{ago, posted_signal, signal, span, verb_signal, Bucket, Freshness, Signal};

/// The posting's own words, fetched the first time someone asks for them and
/// kept afterwards. Listings stay small because of this: a sync that pulled
/// every description would be an order of magnitude more traffic.
pub fn ensure_description(
    store: &Store,
    role: &Role,
    http: &Http,
    now: time::OffsetDateTime,
) -> Result<Option<String>> {
    if let Some(text) = &role.description {
        return Ok(Some(text.clone()));
    }
    let Some(board) = store.board_by_id(role.board_id)? else {
        return Ok(None);
    };
    let Some(adapter) = ats::adapter_for(role.ats) else {
        return Ok(None);
    };
    let Some(text) = adapter.fetch_description(&board.token, &role.external_id, http)? else {
        return Ok(None);
    };
    store.set_description(role.id, &text, now)?;
    Ok(Some(text))
}

/// Ask every adapter, in order, whether it recognises what the person typed.
pub fn detect_board(input: &str, http: &Http) -> Result<Option<DetectedBoard>> {
    first_recognised(ats::adapters().into_iter().map(|a| a.detect(input, http)))
}

/// The first adapter that recognised the input, or the first failure if none
/// did.
///
/// An adapter that will not answer does not stop the others, the same rule
/// [`sync::sync_all`] follows. Greenhouse is asked first and a bare company
/// name is a token on every ATS, so propagating its failure would mean a
/// Greenhouse outage blocking `watch add cursor` from finding Cursor's Ashby
/// board. A failure is still worth raising when nothing was recognised,
/// because the board being looked for may be the one that would not answer,
/// and when all that was recognised is a board Perch cannot fill, for the
/// reason written at that arm.
///
/// Kept out of [`detect_board`] so the order these answers come back in can be
/// tested without the network.
fn first_recognised(
    answers: impl IntoIterator<Item = Result<Option<DetectedBoard>>>,
) -> Result<Option<DetectedBoard>> {
    let mut failure = None;
    for answer in answers {
        match answer {
            Ok(Some(found)) => match failure {
                // The adapter that would not answer may be the one whose
                // address this is. JSON-LD reads any http address, so during a
                // Lever outage it answers for a Lever posting page, and the
                // board is stored as one Perch cannot fill. Nothing detects a
                // board a second time, so that stands for good.
                Some(err) if !found.fill_supported => return Err(err),
                _ => return Ok(Some(found)),
            },
            Ok(None) => {}
            Err(err) => {
                if failure.is_none() {
                    failure = Some(err);
                }
            }
        }
    }
    match failure {
        Some(err) => Err(err),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_board() -> DetectedBoard {
        DetectedBoard {
            ats: Ats::Ashby,
            token: "cursor".into(),
            url: "https://jobs.ashbyhq.com/cursor".into(),
            company_name: "cursor".into(),
            fill_supported: true,
        }
    }

    #[test]
    fn a_board_is_still_found_when_an_earlier_adapter_could_not_be_reached() {
        let answers = vec![
            Err(Error::msg("boards-api.greenhouse.io would not answer")),
            Ok(None),
            Ok(Some(a_board())),
        ];
        let found = first_recognised(answers).unwrap().unwrap();
        assert_eq!(found.token, "cursor");
    }

    /// A JSON-LD page, the one kind of board Perch watches without filling.
    fn a_page_perch_can_only_watch() -> DetectedBoard {
        DetectedBoard {
            ats: Ats::JsonLd,
            token: "https://jobs.lever.co/leverdemo/33538a2f".into(),
            url: "https://jobs.lever.co/leverdemo/33538a2f".into(),
            company_name: "Lever Demo 2".into(),
            fill_supported: false,
        }
    }

    #[test]
    fn a_board_that_would_not_answer_is_not_replaced_by_one_perch_cannot_fill() {
        // A Lever posting page carries JobPosting markup, so with Lever down
        // the JSON-LD adapter answers for it. Storing that answer watches one
        // posting on a board of hundreds and turns filling off, and a board is
        // detected once, so it stays that way after Lever comes back.
        let answers = vec![
            Ok(None),
            Err(Error::msg("api.lever.co would not answer")),
            Ok(None),
            Ok(Some(a_page_perch_can_only_watch())),
        ];
        let err = first_recognised(answers).expect_err("the outage is the answer here");
        assert!(err.to_string().contains("lever"));
    }

    #[test]
    fn a_page_perch_can_only_watch_is_still_a_board_when_nothing_failed() {
        // Which is the ordinary case for this adapter: a careers page on no
        // ATS Perch knows.
        let answers = vec![
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(a_page_perch_can_only_watch())),
        ];
        let found = first_recognised(answers).unwrap().unwrap();
        assert!(!found.fill_supported);
    }

    #[test]
    fn a_failure_is_reported_when_no_adapter_recognised_the_input() {
        // Otherwise a board that exists behind an outage reads as one that
        // does not exist, and the person is told the wrong thing.
        let answers = vec![
            Err(Error::msg("boards-api.greenhouse.io would not answer")),
            Ok(None),
        ];
        let err = first_recognised(answers).expect_err("the failure is the answer here");
        assert!(err.to_string().contains("greenhouse"));
    }

    #[test]
    fn an_input_no_adapter_recognised_is_no_board_rather_than_an_error() {
        let answers = vec![Ok(None), Ok(None)];
        assert!(first_recognised(answers).unwrap().is_none());
    }

    #[test]
    fn no_adapter_is_asked_after_one_recognises_the_input() {
        // Detection stops at the first answer, so adding a board costs one
        // request rather than one per ATS.
        let answers = std::iter::once(Ok(Some(a_board()))).chain(std::iter::once_with(|| {
            panic!("asked after a board was found")
        }));
        assert!(first_recognised(answers).unwrap().is_some());
    }
}
