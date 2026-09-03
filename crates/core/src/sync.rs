//! `perch sync` reads every watched board now.

use crate::ats::adapter_for;
use crate::error::Result;
use crate::http::Http;
use crate::store::{Store, SyncReport};
use time::OffsetDateTime;

/// Polls each watched board in turn and folds the results in.
///
/// A board that will not answer does not stop the others: its failure comes
/// back in `failures` so the caller can say which one, and the rest still sync.
pub fn sync_all(store: &mut Store, http: &Http, now: OffsetDateTime) -> Result<SyncOutcome> {
    let mut outcome = SyncOutcome::default();
    for board in store.boards()? {
        let Some(adapter) = adapter_for(board.ats) else {
            outcome.unsupported.push(board.company_name.clone());
            continue;
        };
        match adapter.fetch(&board.token, http) {
            Ok(listing) => outcome.reports.push(store.absorb(&board, &listing, now)?),
            Err(err) => {
                // `last_checked_at` is deliberately not touched. It means "the
                // last time Perch actually read this board", so a board that
                // will not answer goes visibly stale. That staleness is the
                // only sign the person gets that a company has gone dark.
                outcome
                    .failures
                    .push((board.company_name.clone(), err.to_string()));
            }
        }
    }
    Ok(outcome)
}

#[derive(Debug, Default)]
pub struct SyncOutcome {
    pub reports: Vec<SyncReport>,
    /// Boards whose ATS Perch can no longer read: a database from a newer
    /// version, say. Named rather than silently skipped.
    pub unsupported: Vec<String>,
    pub failures: Vec<(String, String)>,
}

impl SyncOutcome {
    pub fn first_seen(&self) -> usize {
        self.reports.iter().map(|r| r.first_seen).sum()
    }
    pub fn changed(&self) -> usize {
        self.reports
            .iter()
            .map(|r| r.reposted + r.retitled + r.reopened)
            .sum()
    }
    pub fn closed(&self) -> usize {
        self.reports.iter().map(|r| r.closed).sum()
    }
    pub fn quiet(&self) -> bool {
        self.reports.iter().all(|r| r.quiet()) && self.failures.is_empty()
    }
}
