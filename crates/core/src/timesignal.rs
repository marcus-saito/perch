//! Plain-language time, and the freshness ladder the whole interface leans on.
//!
//! Perch has no opinion about how good a match is. It has a precise opinion
//! about how old something is, and it says so in words a person would use.

use time::{Duration, OffsetDateTime};

/// The five steps the feed reads in. Fresh is bright, tired is quiet; nothing
/// else in the interface carries hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Freshness {
    Fresh,
    Recent,
    Settled,
    Stale,
    Tired,
}

impl Freshness {
    /// The CSS class the desktop UI uses, so both front ends agree on the ladder.
    pub fn class(self) -> &'static str {
        match self {
            Freshness::Fresh => "fresh",
            Freshness::Recent => "recent",
            Freshness::Settled => "settled",
            Freshness::Stale => "stale",
            Freshness::Tired => "tired",
        }
    }

    pub fn of(age: Duration) -> Self {
        let days = age.whole_days();
        if age < Duration::days(1) {
            Freshness::Fresh
        } else if days < 7 {
            Freshness::Recent
        } else if days < 30 {
            Freshness::Settled
        } else if days < 90 {
            Freshness::Stale
        } else {
            Freshness::Tired
        }
    }

    pub fn at(posted_at: OffsetDateTime, now: OffsetDateTime) -> Self {
        Freshness::of(now - posted_at)
    }
}

/// Where a role sits in the feed's soft date dividers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Today,
    ThisWeek,
    EarlierThisMonth,
    Older,
}

impl Bucket {
    pub fn heading(self) -> &'static str {
        match self {
            Bucket::Today => "Today",
            Bucket::ThisWeek => "This week",
            Bucket::EarlierThisMonth => "Earlier this month",
            Bucket::Older => "Older",
        }
    }

    pub fn of(age: Duration) -> Self {
        let days = age.whole_days();
        if age < Duration::days(1) {
            Bucket::Today
        } else if days < 7 {
            Bucket::ThisWeek
        } else if days < 30 {
            Bucket::EarlierThisMonth
        } else {
            Bucket::Older
        }
    }
}

fn plural(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        format!("{n} {many}")
    }
}

/// "posted 6 hours ago", "posted yesterday", "open 143 days".
///
/// Under a month Perch says when it went up. Past a month it says how long the
/// role has been open, because that is the more useful fact.
pub fn posted_signal(posted_at: OffsetDateTime, now: OffsetDateTime) -> String {
    let age = now - posted_at;
    if age < Duration::ZERO {
        return "posted just now".to_string();
    }
    if age >= Duration::days(30) {
        return format!("open {} days", age.whole_days());
    }
    format!("posted {}", ago(age))
}

/// The same ladder for something Perch did rather than something a board did:
/// "checked 11 minutes ago", "reposted 3 weeks ago".
pub fn verb_signal(verb: &str, at: OffsetDateTime, now: OffsetDateTime) -> String {
    let age = now - at;
    if age < Duration::ZERO {
        return format!("{verb} just now");
    }
    format!("{verb} {}", ago(age))
}

/// The bare relative phrase: "4 hours ago", "yesterday", "3 weeks ago".
pub fn ago(age: Duration) -> String {
    let minutes = age.whole_minutes();
    let hours = age.whole_hours();
    let days = age.whole_days();

    if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{} ago", plural(minutes, "a minute", "minutes"))
    } else if hours < 24 {
        format!("{} ago", plural(hours, "an hour", "hours"))
    } else if days == 1 {
        "yesterday".to_string()
    } else if days < 14 {
        format!("{days} days ago")
    } else if days < 60 {
        format!("{} ago", plural(days / 7, "a week", "weeks"))
    } else if days < 365 {
        format!("{} ago", plural(days / 30, "a month", "months"))
    } else {
        format!("{} ago", plural(days / 365, "a year", "years"))
    }
}

/// How long something has lasted, rather than how long ago it was: "3 weeks",
/// "a day", "a few minutes". `ago` answers *when*; this answers *how long*.
/// "yesterday" is a moment and cannot be a length.
pub fn span(length: Duration) -> String {
    let minutes = length.whole_minutes();
    let hours = length.whole_hours();
    let days = length.whole_days();

    if minutes < 2 {
        "a few minutes".to_string()
    } else if minutes < 60 {
        format!("{minutes} minutes")
    } else if hours < 24 {
        plural(hours, "an hour", "hours")
    } else if days < 14 {
        plural(days, "a day", "days")
    } else if days < 60 {
        plural(days / 7, "a week", "weeks")
    } else if days < 365 {
        plural(days / 30, "a month", "months")
    } else {
        plural(days / 365, "a year", "years")
    }
}

/// A repost only counts if the board moved the posting by more than a day.
/// Otherwise every trivial edit would read as news.
pub const REPOST_MIN_GAP: Duration = Duration::days(1);

/// And only a recent repost takes over the line. Past this, the useful fact is
/// how long the role has been open, not that someone touched it in March.
pub const REPOST_WINDOW: Duration = Duration::days(30);

/// The moment the feed's line describes, together with the line itself.
///
/// Ordering, date bucketing and freshness all read `at`, which is why a row
/// saying "reposted 3 weeks ago" sits three weeks down the feed rather than
/// wherever it was first published. The words and the position cannot
/// disagree, because they come from the same decision.
#[derive(Debug, Clone)]
pub struct Signal {
    pub at: OffsetDateTime,
    pub text: String,
}

impl Signal {
    pub fn freshness(&self, now: OffsetDateTime) -> Freshness {
        Freshness::at(self.at, now)
    }
    pub fn bucket(&self, now: OffsetDateTime) -> Bucket {
        Bucket::of(now - self.at)
    }
}

/// Decide what a role's time signal says, and what date it says it about.
pub fn signal(
    posted_at: OffsetDateTime,
    remote_updated_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> Signal {
    if let Some(updated) = remote_updated_at {
        let moved = updated - posted_at > REPOST_MIN_GAP;
        let recent = now - updated < REPOST_WINDOW && now >= updated;
        if moved && recent {
            return Signal {
                at: updated,
                text: format!("reposted {}", ago(now - updated)),
            };
        }
    }
    Signal {
        at: posted_at,
        text: posted_signal(posted_at, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-08-29 12:00 UTC);

    fn hours(h: i64) -> OffsetDateTime {
        NOW - Duration::hours(h)
    }
    fn days(d: i64) -> OffsetDateTime {
        NOW - Duration::days(d)
    }

    #[test]
    fn signals_read_the_way_the_mockups_read() {
        assert_eq!(posted_signal(hours(4), NOW), "posted 4 hours ago");
        assert_eq!(posted_signal(hours(9), NOW), "posted 9 hours ago");
        assert_eq!(posted_signal(hours(1), NOW), "posted an hour ago");
        assert_eq!(posted_signal(days(1), NOW), "posted yesterday");
        assert_eq!(posted_signal(days(3), NOW), "posted 3 days ago");
        assert_eq!(posted_signal(days(12), NOW), "posted 12 days ago");
        assert_eq!(posted_signal(days(21), NOW), "posted 3 weeks ago");
        assert_eq!(posted_signal(days(41), NOW), "open 41 days");
        assert_eq!(posted_signal(days(213), NOW), "open 213 days");
    }

    #[test]
    fn a_month_is_where_posted_becomes_open() {
        assert_eq!(posted_signal(days(29), NOW), "posted 4 weeks ago");
        assert_eq!(posted_signal(days(30), NOW), "open 30 days");
    }

    #[test]
    fn a_span_is_a_length_not_a_moment() {
        assert_eq!(span(Duration::seconds(20)), "a few minutes");
        assert_eq!(span(Duration::minutes(11)), "11 minutes");
        assert_eq!(span(Duration::days(1)), "a day");
        assert_eq!(span(Duration::days(5)), "5 days");
        assert_eq!(span(Duration::hours(1)), "an hour");
        assert_eq!(span(Duration::days(21)), "3 weeks");
        assert_eq!(span(Duration::days(400)), "a year");
        for d in [0i64, 1, 59, 3600, 86_400, 86_400 * 400] {
            assert!(!span(Duration::seconds(d)).ends_with(" ago"));
        }
    }

    #[test]
    fn minutes_and_singulars() {
        assert_eq!(ago(Duration::seconds(20)), "just now");
        assert_eq!(ago(Duration::minutes(1)), "a minute ago");
        assert_eq!(ago(Duration::minutes(11)), "11 minutes ago");
        assert_eq!(ago(Duration::days(7)), "7 days ago");
        assert_eq!(ago(Duration::days(14)), "2 weeks ago");
        assert_eq!(ago(Duration::days(70)), "2 months ago");
        assert_eq!(ago(Duration::days(400)), "a year ago");
    }

    #[test]
    fn a_clock_skewed_future_posting_does_not_print_nonsense() {
        let future = NOW + Duration::hours(3);
        assert_eq!(posted_signal(future, NOW), "posted just now");
        assert_eq!(verb_signal("checked", future, NOW), "checked just now");
    }

    #[test]
    fn freshness_ladder_matches_the_design() {
        assert_eq!(Freshness::at(hours(4), NOW), Freshness::Fresh);
        assert_eq!(Freshness::at(hours(23), NOW), Freshness::Fresh);
        assert_eq!(Freshness::at(days(1), NOW), Freshness::Recent);
        assert_eq!(Freshness::at(days(6), NOW), Freshness::Recent);
        assert_eq!(Freshness::at(days(7), NOW), Freshness::Settled);
        assert_eq!(Freshness::at(days(29), NOW), Freshness::Settled);
        assert_eq!(Freshness::at(days(30), NOW), Freshness::Stale);
        assert_eq!(Freshness::at(days(89), NOW), Freshness::Stale);
        assert_eq!(Freshness::at(days(90), NOW), Freshness::Tired);
        assert_eq!(Freshness::at(days(213), NOW), Freshness::Tired);
    }

    #[test]
    fn freshness_never_reads_brighter_as_it_ages() {
        let mut previous = Freshness::Fresh;
        for d in 0..400 {
            let f = Freshness::at(days(d), NOW);
            assert!(f >= previous, "day {d} read brighter than day {}", d - 1);
            previous = f;
        }
    }

    #[test]
    fn a_recent_repost_takes_over_the_line() {
        let s = signal(days(213), Some(days(21)), NOW);
        assert_eq!(s.text, "reposted 3 weeks ago");
        // And it takes over the position too, or the feed would print a line
        // saying "3 weeks" underneath rows that are months older.
        assert_eq!(s.at, days(21));
        assert_eq!(s.bucket(NOW), Bucket::EarlierThisMonth);
        assert_eq!(s.freshness(NOW), Freshness::Settled);
    }

    #[test]
    fn an_old_repost_is_not_the_useful_fact() {
        let s = signal(days(213), Some(days(60)), NOW);
        assert_eq!(s.text, "open 213 days");
        assert_eq!(s.at, days(213));
        assert_eq!(s.freshness(NOW), Freshness::Tired);
    }

    #[test]
    fn a_trivial_edit_is_not_a_repost() {
        // The board touched it six hours after publishing. That is not news.
        let s = signal(days(40), Some(days(40) + Duration::hours(6)), NOW);
        assert_eq!(s.text, "open 40 days");
        assert_eq!(s.at, days(40));
    }

    #[test]
    fn the_line_and_the_position_never_disagree() {
        // Whatever the signal says, its date is the date it describes.
        for posted in [1i64, 8, 31, 200, 700] {
            for updated in [None, Some(0i64), Some(3), Some(29), Some(120)] {
                let s = signal(days(posted), updated.map(days), NOW);
                let described = if s.text.starts_with("reposted") {
                    updated.map(days).unwrap()
                } else {
                    days(posted)
                };
                assert_eq!(s.at, described, "posted {posted}, updated {updated:?}");
            }
        }
    }

    #[test]
    fn buckets_line_up_with_the_feed_dividers() {
        assert_eq!(Bucket::of(Duration::hours(4)).heading(), "Today");
        assert_eq!(Bucket::of(Duration::days(3)).heading(), "This week");
        assert_eq!(
            Bucket::of(Duration::days(12)).heading(),
            "Earlier this month"
        );
        assert_eq!(Bucket::of(Duration::days(41)).heading(), "Older");
    }
}
