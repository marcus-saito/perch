use crate::error::{Error, Result};
use time::OffsetDateTime;

/// Perch monitors broadly and fills narrowly, so the ATS a board runs on and
/// whether Perch can fill its forms are two different facts. The adapter
/// declares the second one; see [`crate::ats::AtsAdapter::fill_supported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ats {
    Greenhouse,
    Lever,
    Ashby,
    JsonLd,
}

impl Ats {
    pub fn as_str(self) -> &'static str {
        match self {
            Ats::Greenhouse => "greenhouse",
            Ats::Lever => "lever",
            Ats::Ashby => "ashby",
            Ats::JsonLd => "json-ld",
        }
    }

    /// How it reads in the interface: "Greenhouse", "JSON-LD".
    pub fn label(self) -> &'static str {
        match self {
            Ats::Greenhouse => "Greenhouse",
            Ats::Lever => "Lever",
            Ats::Ashby => "Ashby",
            Ats::JsonLd => "JSON-LD",
        }
    }

    /// The label with the article that reads correctly in front of it, so a
    /// sentence built around it does not say "a Ashby board".
    ///
    /// The first letter decides it, which is right for every label here.
    /// "JSON-LD" is read out as "jay", so it takes "a" like its spelling says.
    pub fn with_article(self) -> String {
        let label = self.label();
        let article = if label.starts_with(['A', 'E', 'I', 'O', 'U']) {
            "an"
        } else {
            "a"
        };
        format!("{article} {label}")
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "greenhouse" => Ok(Ats::Greenhouse),
            "lever" => Ok(Ats::Lever),
            "ashby" => Ok(Ats::Ashby),
            "json-ld" => Ok(Ats::JsonLd),
            other => Err(Error::msg(format!("unknown ATS in the database: {other}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub added_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub id: i64,
    pub company_id: i64,
    pub company_name: String,
    pub ats: Ats,
    /// The board's own identifier on that ATS: a Greenhouse board token, say.
    pub token: String,
    pub url: String,
    pub fill_supported: bool,
    pub last_checked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct Role {
    pub id: i64,
    pub board_id: i64,
    pub company_name: String,
    pub ats: Ats,
    pub fill_supported: bool,
    /// Short, stable handle for the CLI: `perch dismiss 4f2a1c`.
    pub reference: String,
    pub external_id: String,
    pub title: String,
    pub location: String,
    pub url: String,
    /// When the board says it was published. Absent on boards that do not say.
    pub posted_at: Option<OffsetDateTime>,
    /// When the board last said it changed. Drives the repost signal.
    pub remote_updated_at: Option<OffsetDateTime>,
    /// When *Perch* first saw it. Bounds every claim Perch makes about history.
    pub first_seen_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub closed_at: Option<OffsetDateTime>,
    pub dismissed_at: Option<OffsetDateTime>,
    /// The posting's own words, as the board gave them. Fetched only when
    /// someone opens the role, because listings are an order of magnitude
    /// smaller without them.
    pub description: Option<String>,
    pub description_fetched_at: Option<OffsetDateTime>,
}

impl Role {
    /// The date the time signal is measured from: what the board claims, or
    /// failing that, the first time Perch saw it.
    pub fn effective_posted_at(&self) -> OffsetDateTime {
        self.posted_at.unwrap_or(self.first_seen_at)
    }

    /// What the feed prints for this role, and the date that line describes.
    /// Everything ordering-related reads [`crate::timesignal::Signal::at`], so
    /// the row's words and the row's position always agree.
    pub fn signal(&self, now: OffsetDateTime) -> crate::timesignal::Signal {
        crate::timesignal::signal(self.effective_posted_at(), self.remote_updated_at, now)
    }
}

/// What Perch observed happening to a posting. The detail pane renders these
/// as board history, and every one of them is something Perch saw itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    FirstSeen,
    Reposted,
    Retitled,
    Relocated,
    Closed,
    Reopened,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::FirstSeen => "first_seen",
            EventKind::Reposted => "reposted",
            EventKind::Retitled => "retitled",
            EventKind::Relocated => "relocated",
            EventKind::Closed => "closed",
            EventKind::Reopened => "reopened",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "first_seen" => Ok(EventKind::FirstSeen),
            "reposted" => Ok(EventKind::Reposted),
            "retitled" => Ok(EventKind::Retitled),
            "relocated" => Ok(EventKind::Relocated),
            "closed" => Ok(EventKind::Closed),
            "reopened" => Ok(EventKind::Reopened),
            other => Err(Error::msg(format!(
                "unknown event in the database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoleEvent {
    pub id: i64,
    pub role_id: i64,
    pub at: OffsetDateTime,
    pub kind: EventKind,
    /// Plain language, already written for a person: "was 'Runtime Engineer'".
    pub detail: Option<String>,
}

/// A posting as the board describes it, before Perch has an opinion about it.
#[derive(Debug, Clone)]
pub struct RemoteRole {
    pub external_id: String,
    pub title: String,
    pub location: String,
    pub url: String,
    pub posted_at: Option<OffsetDateTime>,
    pub updated_at: Option<OffsetDateTime>,
    /// The posting's own words, when the board sent them with the listing.
    /// Ashby has no per-posting endpoint and puts the description in the list
    /// call, so it has already arrived by the time the listing is parsed.
    /// Dropping it would mean refetching a board of several megabytes to read
    /// one role. An adapter whose listing does not carry the description
    /// leaves this `None`, and that role is still filled in on demand by
    /// [`crate::ats::AtsAdapter::fetch_description`].
    pub description: Option<String>,
}

/// One board's listing, as fetched.
///
/// `listed_ids` holds every posting the board named, including entries this
/// build could not parse. `absorb` closes roles by their absence from that
/// list, so an entry Perch failed to read stays open rather than being
/// reported as having come down. Perch records only what it observed.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub roles: Vec<RemoteRole>,
    pub listed_ids: Vec<String>,
}

/// Where an application stands. Three states, because the design shows three
/// groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationState {
    InFlight,
    Responded,
    Archived,
}

impl ApplicationState {
    pub fn as_str(self) -> &'static str {
        match self {
            ApplicationState::InFlight => "in_flight",
            ApplicationState::Responded => "responded",
            ApplicationState::Archived => "archived",
        }
    }

    /// How it reads in the interface.
    pub fn heading(self) -> &'static str {
        match self {
            ApplicationState::InFlight => "In flight",
            ApplicationState::Responded => "Responded",
            ApplicationState::Archived => "Archived",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "in-flight" | "inflight" | "sent" | "applied" => Ok(ApplicationState::InFlight),
            "responded" | "reply" | "replied" => Ok(ApplicationState::Responded),
            "archived" | "archive" => Ok(ApplicationState::Archived),
            other => Err(Error::msg(format!(
                "'{other}' is not one of in-flight, responded or archived"
            ))),
        }
    }
}

/// An application the person sent. Perch records that it happened and what came
/// back; it has no part in the sending.
#[derive(Debug, Clone)]
pub struct Application {
    pub id: i64,
    pub role_id: i64,
    pub company_name: String,
    pub title: String,
    pub reference: String,
    pub state: ApplicationState,
    pub applied_at: OffsetDateTime,
    /// The last thing that happened, in the person's own words.
    pub note: Option<String>,
    pub note_at: Option<OffsetDateTime>,
    pub archived_at: Option<OffsetDateTime>,
    /// True when Perch archived it on its own after a long silence, rather
    /// than the person filing it away.
    pub archived_quietly: bool,
}

/// What a board has actually done lately, measured only from what Perch saw.
#[derive(Debug, Clone)]
pub struct Velocity {
    pub opened_recently: usize,
    pub still_open: usize,
    /// Median days from posting to coming down, across postings Perch watched
    /// close. Absent until enough of them have.
    pub median_days_to_close: Option<i64>,
    /// When Perch started watching this board. Every number above is bounded
    /// by it, and the interface says so.
    pub watching_since: OffsetDateTime,
}

/// What `watch add` found when it went looking.
#[derive(Debug, Clone)]
pub struct DetectedBoard {
    pub ats: Ats,
    pub token: String,
    pub url: String,
    pub company_name: String,
    pub fill_supported: bool,
}

/// A stable short handle for a posting, so the CLI can name one.
/// FNV-1a over the identity triple: same posting, same reference, every sync.
pub fn role_reference(ats: Ats, token: &str, external_id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let parts = [
        ats.as_str().as_bytes(),
        b":",
        token.as_bytes(),
        b":",
        external_id.as_bytes(),
    ];
    for byte in parts.iter().flat_map(|p| p.iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    // FNV-1a alone avalanches poorly on its last byte: its final multiply only
    // propagates bits upward. Ids differing in their tail (which is what two
    // postings on one board look like) come out sharing a long prefix, and
    // short references stop naming one role. Finish with fmix64 so every input
    // bit reaches every output bit.
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^= hash >> 33;
    format!("{:08x}", hash & 0xffff_ffff)
}

/// Names a company the way a person typed it: "Val Town" and "valtown" and
/// "https://boards.greenhouse.io/valtown" all reduce to the same slug.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_gets_the_article_that_reads_correctly_in_front_of_it() {
        assert_eq!(Ats::Greenhouse.with_article(), "a Greenhouse");
        assert_eq!(Ats::Lever.with_article(), "a Lever");
        assert_eq!(Ats::Ashby.with_article(), "an Ashby");
        assert_eq!(Ats::JsonLd.with_article(), "a JSON-LD");
    }

    #[test]
    fn reference_is_stable_and_identity_scoped() {
        let a = role_reference(Ats::Greenhouse, "valtown", "123");
        assert_eq!(a, role_reference(Ats::Greenhouse, "valtown", "123"));
        assert_ne!(a, role_reference(Ats::Greenhouse, "valtown", "124"));
        assert_ne!(a, role_reference(Ats::Lever, "valtown", "123"));
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn adjacent_postings_do_not_share_a_reference_prefix() {
        // Board ids on one board differ only in their last characters. If that
        // barely moves the reference, short prefixes stop naming one role.
        let refs: Vec<String> = (0..64)
            .map(|n| role_reference(Ats::Greenhouse, "valtown", &format!("500000{n:02}")))
            .collect();
        let heads: std::collections::HashSet<&str> = refs.iter().map(|r| &r[..4]).collect();
        assert!(
            heads.len() >= 62,
            "64 postings on one board produced only {} distinct 4-character prefixes",
            heads.len()
        );
        let all: std::collections::HashSet<&String> = refs.iter().collect();
        assert_eq!(all.len(), 64, "two postings on one board share a reference");
    }

    #[test]
    fn slugs_collapse_punctuation() {
        assert_eq!(slugify("Val Town"), "val-town");
        assert_eq!(slugify("Fly.io"), "fly-io");
        assert_eq!(slugify("  Oxide   Computer  "), "oxide-computer");
        assert_eq!(slugify("Zed Industries!"), "zed-industries");
    }
}
