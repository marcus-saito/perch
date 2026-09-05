//! Adapters for the boards Perch watches.
//!
//! Adding an ATS is one file and one line in [`adapters`]. Whether Perch can
//! *fill* that ATS's forms is declared separately from whether it can *watch*
//! them, because Perch monitors broadly and fills narrowly.

use crate::error::Result;
use crate::http::Http;
use crate::model::{Ats, DetectedBoard, Listing};

pub mod ashby;
pub mod greenhouse;
pub mod jsonld;
pub mod lever;

pub trait AtsAdapter: Send + Sync {
    fn ats(&self) -> Ats;

    /// Declared per adapter, never inferred from [`AtsAdapter::ats`]. A board
    /// Perch can read but not fill opens in the browser instead, and the
    /// interface says so in those words.
    fn fill_supported(&self) -> bool;

    /// Work out whether `input` names a board on this ATS. `input` is whatever
    /// the person typed: a company name, a board URL, a careers page.
    /// `Ok(None)` means "not this one": try the next adapter.
    fn detect(&self, input: &str, http: &Http) -> Result<Option<DetectedBoard>>;

    /// Every posting the board currently lists.
    ///
    /// Must return an error rather than an empty listing when the board cannot
    /// be read at all: an empty listing is taken as "everything came down",
    /// and Perch may only conclude that from a board that actually said so.
    fn fetch(&self, token: &str, http: &Http) -> Result<Listing>;

    /// The posting's own words. Fetched only when someone opens a role, so a
    /// sync stays small. `Ok(None)` means the board no longer has it.
    fn fetch_description(
        &self,
        token: &str,
        external_id: &str,
        http: &Http,
    ) -> Result<Option<String>>;
}

/// The name a board goes by when its payload never states one.
///
/// Ashby and Lever name nothing, so the only name the board gives is the
/// token, which is lowercase and stripped of spaces. When the person typed a
/// plain name rather than an address, that name is the one they will look for
/// in the feed, so it is kept as typed. An address gives no name to keep.
pub(crate) fn name_as_typed(input: &str, token: &str) -> String {
    let typed = input.trim();
    if typed.is_empty() || typed.contains("://") || typed.contains('/') {
        token.to_string()
    } else {
        typed.to_string()
    }
}

/// Every adapter Perch knows, in the order `watch add` tries them.
pub fn adapters() -> Vec<Box<dyn AtsAdapter>> {
    vec![
        Box::new(greenhouse::Greenhouse),
        Box::new(lever::Lever),
        Box::new(ashby::Ashby),
        // Last, and it has to stay last. It accepts any http(s) address, so
        // asked earlier it would answer for a Greenhouse, Lever or Ashby URL
        // before the adapter that can actually fill that board got its turn,
        // and the board would be stored as one Perch cannot fill. The order
        // holds while the adapter whose address it is answers at all;
        // `first_recognised` is what covers the one that was down.
        Box::new(jsonld::JsonLd),
    ]
}

pub fn adapter_for(ats: Ats) -> Option<Box<dyn AtsAdapter>> {
    adapters().into_iter().find(|a| a.ats() == ats)
}

/// The boards Perch can watch, named for someone whose input no adapter
/// recognised: "Greenhouse, Lever, Ashby and JSON-LD".
///
/// Read off [`adapters`] rather than written out, because a message that lists
/// them by hand goes stale the next time one is added and then tells people
/// Perch reads fewer boards than it does.
pub fn watchable() -> String {
    let names: Vec<&str> = adapters().iter().map(|a| a.ats().label()).collect();
    match names.as_slice() {
        [] => String::new(),
        [only] => only.to_string(),
        [front @ .., last] => format!("{} and {last}", front.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boards_perch_offers_to_watch_are_every_adapter_it_has() {
        // The point of reading the list off `adapters` is that this stays true
        // without anyone remembering to edit a sentence.
        let named = watchable();
        for adapter in adapters() {
            assert!(
                named.contains(adapter.ats().label()),
                "{named} leaves out {}",
                adapter.ats().label()
            );
        }
        assert_eq!(named, "Greenhouse, Lever, Ashby and JSON-LD");
    }

    #[test]
    fn a_plain_name_is_kept_the_way_the_person_typed_it() {
        assert_eq!(name_as_typed("Val Town", "valtown"), "Val Town");
        assert_eq!(name_as_typed("  Cursor ", "cursor"), "Cursor");
        // An address names nothing, so the token stands in.
        assert_eq!(
            name_as_typed("https://jobs.ashbyhq.com/cursor", "cursor"),
            "cursor"
        );
        assert_eq!(name_as_typed("jobs.lever.co/fly", "fly"), "fly");
        assert_eq!(name_as_typed("", "fly"), "fly");
    }

    #[test]
    fn every_adapter_can_be_found_again_by_the_ats_it_declares() {
        // A board is stored by its ATS and read back through `adapter_for`, so
        // an adapter that cannot be found again is one whose boards go
        // unreadable at the next sync.
        for adapter in adapters() {
            let found = adapter_for(adapter.ats()).expect("adapter is reachable by its own ats");
            assert_eq!(found.ats(), adapter.ats());
        }
    }
}
