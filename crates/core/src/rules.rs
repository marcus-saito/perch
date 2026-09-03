//! Match rules. A plain text file the person owns.
//!
//! A rule either fires or it does not. There is no score, no weight and no
//! ranking. Perch says which rule fired and what in the posting set it off, so
//! every row in the feed can explain itself in one line.
//!
//! Rules are read fresh every time. Editing the file changes the next `feed`
//! immediately. There is nothing to re-sync and no cached verdict to go stale.

use crate::error::{Error, Result};
use crate::model::Role;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file as it is written on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuleFile {
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

/// One named rule. Every condition present must hold for it to fire.
///
/// `deny_unknown_fields` matters here: a misspelt `titel = [...]` has to be an
/// error the person can see, not a condition that silently never fires.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    /// Shown verbatim in the feed, so it is worth naming well.
    pub name: String,
    /// The title must contain at least one of these.
    pub title: Vec<String>,
    /// The title must contain none of these.
    pub title_excludes: Vec<String>,
    /// The location must contain at least one of these.
    pub location: Vec<String>,
    pub location_excludes: Vec<String>,
    /// The company name must contain at least one of these.
    pub company: Vec<String>,
}

/// Why a role is in the feed: the rule that fired, and the thing that fired it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Why {
    pub rule: String,
    /// Already written for a person: "'Rust' in the title".
    pub because: String,
}

/// Case-insensitive, word-boundary search returning the byte range of the
/// match **in `haystack` itself**.
///
/// Two things make this fiddlier than a `contains` call.
///
/// Plain substring matching would have a rule looking for `rust` fire on
/// "Product Manager, Trust & Safety" and then print "'rust' in the title" as
/// its reason, which is not true of that posting. A needle has to sit on its
/// own word: "Rust Engineer", "Rust/C++" and "(Rust)" all count; "Trust" and
/// "Ürust" do not.
///
/// And the range has to be an offset into the original string, not into a
/// lowercased copy of it. Case folding changes byte lengths in both directions
/// (`İ` grows, `ẞ` shrinks), so an offset taken from the folded form and used
/// on the original quotes garbage, or panics on a character boundary. Walking
/// the original's own characters keeps every offset valid by construction.
fn word_range(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    let needle: Vec<char> = needle.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = haystack.char_indices().collect();
    // One folded character per original character, so indices stay aligned.
    let folded: Vec<char> = chars
        .iter()
        .map(|(_, c)| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    if needle.len() > folded.len() {
        return None;
    }

    for start in 0..=folded.len() - needle.len() {
        if folded[start..start + needle.len()] != needle[..] {
            continue;
        }
        let end = start + needle.len();
        let before_ok = start == 0 || !folded[start - 1].is_alphanumeric();
        let after_ok = end >= folded.len() || !folded[end].is_alphanumeric();
        if before_ok && after_ok {
            let from = chars[start].0;
            let to = chars
                .get(end)
                .map(|(at, _)| *at)
                .unwrap_or_else(|| haystack.len());
            return Some((from, to));
        }
    }
    None
}

/// The first needle that appears, and the span of the posting it matched.
fn find(haystack: &str, needles: &[String]) -> Option<(usize, usize)> {
    needles.iter().find_map(|n| word_range(haystack, n))
}

/// The matched text exactly as the posting writes it, so the explanation quotes
/// the posting rather than the rule: a rule saying `rust` reports 'Rust'.
fn as_written(haystack: &str, span: (usize, usize)) -> &str {
    haystack.get(span.0..span.1).unwrap_or(haystack)
}

/// Locations are often several offices joined by pipes and semicolons. Naming
/// the whole thing would fill the line, so past a certain length the reason
/// quotes what matched instead of the whole field.
fn location_reason(location: &str, span: (usize, usize)) -> String {
    if location.chars().count() <= 40 {
        format!("location says {location}")
    } else {
        format!("'{}' in the location", as_written(location, span))
    }
}

/// Needles that are blank once trimmed can never match. A list of them is a
/// condition that does nothing, or, as an exclusion, one that matches
/// everything. Counting only usable needles keeps validation honest.
fn usable(needles: &[String]) -> usize {
    needles.iter().filter(|n| !n.trim().is_empty()).count()
}

fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} or {b}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    }
}

impl Rule {
    fn check_name(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(Error::msg(
                "a rule has no name; every rule needs one, because the feed prints it",
            ));
        }
        if name.split_whitespace().count() > 1 {
            return Err(Error::msg(format!(
                "the rule name '{name}' has a space in it; names appear in the feed, so use one word like 'rust-in-title'"
            )));
        }
        Ok(())
    }

    fn conditions(&self) -> usize {
        [
            &self.title,
            &self.title_excludes,
            &self.location,
            &self.location_excludes,
            &self.company,
        ]
        .iter()
        .filter(|c| usable(c) > 0)
        .count()
    }

    pub fn validate(&self) -> Result<()> {
        self.check_name()?;
        for (field, needles) in [
            ("title", &self.title),
            ("title_excludes", &self.title_excludes),
            ("location", &self.location),
            ("location_excludes", &self.location_excludes),
            ("company", &self.company),
        ] {
            if needles.iter().any(|n| n.trim().is_empty()) {
                return Err(Error::msg(format!(
                    "the rule '{}' has an empty entry in {field}; an empty entry matches nothing, which changes what the rule does",
                    self.name.trim()
                )));
            }
        }
        if self.conditions() == 0 {
            return Err(Error::msg(format!(
                "the rule '{}' says nothing, so it would match every role; give it something to look for, or delete it",
                self.name.trim()
            )));
        }
        Ok(())
    }

    /// Does this rule fire on this role, and if so, what is the honest reason?
    pub fn fires(&self, role: &Role) -> Option<Why> {
        // A rule with nothing usable in it must never fire. `validate` refuses
        // to load one, but a hand-built `Rule` must not become a wildcard.
        if self.conditions() == 0 {
            return None;
        }

        // Exclusions first: they can only rule a role out, never explain it.
        if find(&role.title, &self.title_excludes).is_some() {
            return None;
        }
        if find(&role.location, &self.location_excludes).is_some() {
            return None;
        }

        let mut reason: Option<String> = None;
        let mut require =
            |hit: Option<(usize, usize)>, needed: bool, say: &dyn Fn((usize, usize)) -> String| {
                if !needed {
                    return true;
                }
                match hit {
                    Some(span) => {
                        if reason.is_none() {
                            reason = Some(say(span));
                        }
                        true
                    }
                    None => false,
                }
            };

        if !require(
            find(&role.title, &self.title),
            usable(&self.title) > 0,
            &|span| format!("'{}' in the title", as_written(&role.title, span)),
        ) {
            return None;
        }
        if !require(
            find(&role.location, &self.location),
            usable(&self.location) > 0,
            &|span| location_reason(&role.location, span),
        ) {
            return None;
        }
        if !require(
            find(&role.company_name, &self.company),
            usable(&self.company) > 0,
            // Say what matched. Whether the person is still watching the
            // company is not something the rules layer can know.
            &|_| format!("the company is {}", role.company_name),
        ) {
            return None;
        }

        // A rule that only excludes still fires; say so in its own terms.
        let because = reason.unwrap_or_else(|| {
            if usable(&self.title_excludes) > 0 {
                format!("the title avoids {}", list(&self.title_excludes))
            } else {
                format!("the location avoids {}", list(&self.location_excludes))
            }
        });

        Some(Why {
            rule: self.name.trim().to_string(),
            because,
        })
    }
}

/// Every rule the person has written, in the order they wrote them.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub rules: Vec<Rule>,
}

impl Rules {
    /// No rules file is not an error. Perch filters nothing until it is told
    /// to, so the feed shows everything that is open.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err.into()),
        };
        let file: RuleFile = toml::from_str(&text).map_err(|source| Error::Toml {
            path: path.display().to_string(),
            source,
        })?;
        let rules = Self { rules: file.rules };
        rules.validate()?;
        Ok(rules)
    }

    fn validate(&self) -> Result<()> {
        let mut seen: Vec<&str> = Vec::new();
        for rule in &self.rules {
            rule.validate()?;
            let name = rule.name.trim();
            if seen.contains(&name) {
                return Err(Error::msg(format!(
                    "there are two rules called '{name}'; the feed could not say which one fired"
                )));
            }
            seen.push(name);
        }
        Ok(())
    }

    /// True when the person has not written any rules. With none written,
    /// Perch does not filter the feed.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first rule that fires, in file order. Precedence is the order the
    /// person put the rules in.
    pub fn why(&self, role: &Role) -> Option<Why> {
        self.rules.iter().find_map(|rule| rule.fires(role))
    }

    /// Everything each rule says about one role, for `rules test`.
    pub fn explain(&self, role: &Role) -> Vec<(String, Option<Why>)> {
        self.rules
            .iter()
            .map(|rule| (rule.name.trim().to_string(), rule.fires(role)))
            .collect()
    }

    /// Pairs each role with its reason. With no rules written, every role comes
    /// through with no reason attached, because there is nothing to explain.
    pub fn apply(&self, roles: Vec<Role>) -> Vec<(Role, Option<Why>)> {
        if self.is_empty() {
            return roles.into_iter().map(|r| (r, None)).collect();
        }
        roles
            .into_iter()
            .filter_map(|role| self.why(&role).map(|why| (role, Some(why))))
            .collect()
    }
}

/// What `rules edit` writes when there is no file yet: a commented starting
/// point, so the first thing the person sees is an explanation rather than an
/// empty buffer.
pub const TEMPLATE: &str = r#"# Perch match rules.
#
# A role reaches your feed if ANY rule below fires on it. A rule fires when
# EVERY line in it holds. Rules are tried top to bottom, and the first one to
# fire is the one the feed names, so put the rule you would most like to read
# about first.
#
# Nothing here is scored or weighted. A rule fires or it does not.
#
# With no rules at all, the feed shows every open role. Rules narrow it.
#
# Matching is case-insensitive and works on whole words, so a rule looking for
# "rust" fires on "Rust Engineer" and "Rust/C++" but not on "Trust & Safety".
# Several words in one entry are matched as a phrase.
#
# Rules see the posting's title, location and company: what the board puts in
# its listing. Descriptions are not read yet.
#
# Delete the # in front of a rule to switch it on.

# [[rule]]
# name = "rust-in-title"
# title = ["rust"]

# [[rule]]
# name = "remote-ok"
# location = ["remote", "anywhere"]

# [[rule]]
# name = "systems-keywords"
# title = ["distributed", "storage", "infrastructure", "platform", "runtime"]

# A rule can also rule things out. This one fires on engineering titles that
# are not staff-and-above.
# [[rule]]
# name = "not-staff-plus"
# title = ["engineer"]
# title_excludes = ["staff", "principal", "distinguished", "fellow"]

# [[rule]]
# name = "bay-area"
# location = ["san francisco", "oakland", "berkeley", "bay area"]

# [[rule]]
# name = "watched-company"
# company = ["oxide computer"]
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ats;
    use time::OffsetDateTime;

    fn role(title: &str, location: &str, company: &str) -> Role {
        Role {
            id: 1,
            board_id: 1,
            company_name: company.into(),
            ats: Ats::Greenhouse,
            fill_supported: true,
            reference: "abcd1234".into(),
            external_id: "1".into(),
            title: title.into(),
            location: location.into(),
            url: String::new(),
            posted_at: None,
            remote_updated_at: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_seen_at: OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            dismissed_at: None,
            description: None,
            description_fetched_at: None,
        }
    }

    fn rules(toml_text: &str) -> Rules {
        let file: RuleFile = toml::from_str(toml_text).unwrap();
        let rules = Rules { rules: file.rules };
        rules.validate().unwrap();
        rules
    }

    #[test]
    fn a_rule_explains_itself_by_quoting_the_posting() {
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        let why = r
            .why(&role("Rust Engineer, uv", "Remote (US)", "Astral"))
            .unwrap();
        assert_eq!(why.rule, "rust-in-title");
        // The rule says "rust"; the explanation quotes the posting's "Rust".
        assert_eq!(why.because, "'Rust' in the title");
    }

    #[test]
    fn a_needle_has_to_stand_on_its_own_word() {
        // "Trust & Safety" firing a rule about Rust, and then claiming
        // "'rust' in the title", is the failure this exists to prevent.
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        for fires in [
            "Rust Engineer",
            "Engineer, Rust",
            "Rust/C++ Engineer",
            "(Rust) Engineer",
            "RUST ENGINEER",
        ] {
            assert!(r.why(&role(fires, "Remote", "Astral")).is_some(), "{fires}");
        }
        for quiet in [
            "Product Manager, Trust & Safety",
            "Rustic Interiors Lead",
            "Entrusted Systems",
        ] {
            assert!(r.why(&role(quiet, "Remote", "Astral")).is_none(), "{quiet}");
        }
    }

    #[test]
    fn a_multi_word_needle_matches_as_a_phrase() {
        let r = rules(
            r#"
            [[rule]]
            name = "bay-area"
            location = ["san francisco", "bay area"]
        "#,
        );
        assert!(r
            .why(&role("Engineer", "San Francisco, CA", "Warp"))
            .is_some());
        assert!(r
            .why(&role("Engineer", "Berlin, Germany", "Warp"))
            .is_none());
    }

    #[test]
    fn an_exclusion_also_respects_word_boundaries() {
        let r = rules(
            r#"
            [[rule]]
            name = "not-staff-plus"
            title = ["engineer"]
            title_excludes = ["staff"]
        "#,
        );
        assert!(r.why(&role("Staff Engineer", "Remote", "Warp")).is_none());
        // "Staffing" is not "Staff", and must not rule the role out.
        assert!(r
            .why(&role("Staffing Systems Engineer", "Remote", "Warp"))
            .is_some());
    }

    #[test]
    fn a_sprawling_location_does_not_swamp_the_reason() {
        let r = rules(
            r#"
            [[rule]]
            name = "remote-ok"
            location = ["remote"]
        "#,
        );
        let short = r.why(&role("Engineer", "Remote (US)", "Fly.io")).unwrap();
        assert_eq!(short.because, "location says Remote (US)");

        let sprawling = "Remote-Friendly (Travel-Required) |  Washington, DC; San Francisco, CA | New York City, NY";
        let long = r.why(&role("Engineer", sprawling, "Anthropic")).unwrap();
        assert_eq!(long.because, "'Remote' in the location");
        assert!(long.because.chars().count() < 40);
    }

    #[test]
    fn every_condition_in_a_rule_must_hold() {
        let r = rules(
            r#"
            [[rule]]
            name = "remote-rust"
            title = ["rust"]
            location = ["remote"]
        "#,
        );
        assert!(r
            .why(&role("Rust Engineer", "Remote (US)", "Astral"))
            .is_some());
        assert!(r
            .why(&role("Rust Engineer", "Berlin, Germany", "Astral"))
            .is_none());
        assert!(r
            .why(&role("Go Engineer", "Remote (US)", "Astral"))
            .is_none());
    }

    #[test]
    fn an_exclusion_rules_a_role_out_however_well_it_otherwise_fits() {
        let r = rules(
            r#"
            [[rule]]
            name = "not-staff-plus"
            title = ["engineer"]
            title_excludes = ["staff", "principal"]
        "#,
        );
        assert!(r
            .why(&role("Senior Software Engineer", "Remote", "Warp"))
            .is_some());
        assert!(r
            .why(&role("Staff Software Engineer", "Remote", "Warp"))
            .is_none());
        assert!(r
            .why(&role("Principal Engineer", "Remote", "Warp"))
            .is_none());
    }

    #[test]
    fn a_rule_that_only_excludes_still_explains_itself() {
        let r = rules(
            r#"
            [[rule]]
            name = "no-management"
            title_excludes = ["manager", "director", "head of"]
        "#,
        );
        let why = r.why(&role("Software Engineer", "Remote", "Warp")).unwrap();
        assert_eq!(
            why.because,
            "the title avoids manager, director, or head of"
        );
        assert!(r
            .why(&role("Engineering Manager", "Remote", "Warp"))
            .is_none());
    }

    #[test]
    fn the_first_rule_to_fire_is_the_one_the_feed_names() {
        // Precedence is file order, because that is the only ordering the
        // person can see and change.
        let r = rules(
            r#"
            [[rule]]
            name = "remote-ok"
            location = ["remote"]

            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        assert_eq!(
            r.why(&role("Rust Engineer", "Remote (US)", "Astral"))
                .unwrap()
                .rule,
            "remote-ok"
        );
    }

    #[test]
    fn matching_is_case_insensitive_both_ways() {
        let r = rules(
            r#"
            [[rule]]
            name = "shouty"
            title = ["RUST"]
            location = ["Remote"]
        "#,
        );
        assert!(r
            .why(&role("rust engineer", "REMOTE (US)", "Astral"))
            .is_some());
    }

    #[test]
    fn with_no_rules_everything_comes_through_unexplained() {
        let r = Rules::default();
        let roles = vec![role("Anything", "Anywhere", "Someone")];
        let out = r.apply(roles);
        assert_eq!(out.len(), 1);
        assert!(
            out[0].1.is_none(),
            "an unfiltered feed must not invent a reason"
        );
    }

    #[test]
    fn rules_narrow_the_feed_and_every_survivor_carries_a_reason() {
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        let out = r.apply(vec![
            role("Rust Engineer", "Remote", "Astral"),
            role("Account Executive", "Berlin", "Figma"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.as_ref().unwrap().rule, "rust-in-title");
    }

    #[test]
    fn a_rule_with_nothing_in_it_is_an_error_rather_than_a_wildcard() {
        let file: RuleFile = toml::from_str("[[rule]]\nname = \"empty\"").unwrap();
        let err = (Rules { rules: file.rules })
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("says nothing"), "{err}");
    }

    #[test]
    fn a_misspelt_field_is_an_error_rather_than_a_rule_that_never_fires() {
        let text = "[[rule]]\nname = \"typo\"\ntitel = [\"rust\"]";
        assert!(toml::from_str::<RuleFile>(text).is_err());
    }

    #[test]
    fn two_rules_with_one_name_is_an_error() {
        let file: RuleFile = toml::from_str(
            "[[rule]]\nname = \"a\"\ntitle = [\"x\"]\n[[rule]]\nname = \"a\"\ntitle = [\"y\"]",
        )
        .unwrap();
        let err = (Rules { rules: file.rules })
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("two rules called"), "{err}");
    }

    #[test]
    fn a_name_with_a_space_is_refused_because_the_feed_prints_it() {
        let file: RuleFile =
            toml::from_str("[[rule]]\nname = \"rust in title\"\ntitle = [\"x\"]").unwrap();
        assert!((Rules { rules: file.rules }).validate().is_err());
    }

    #[test]
    fn a_missing_rules_file_leaves_the_feed_alone() {
        let r = Rules::load(Path::new("/nowhere/at/all/rules.toml")).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn the_template_parses_and_switching_a_rule_on_works() {
        let parsed: RuleFile = toml::from_str(TEMPLATE).unwrap();
        assert!(
            parsed.rules.is_empty(),
            "the template ships with nothing switched on"
        );

        // Uncomment only the lines that are rule syntax, the way a person
        // would: the prose above them stays a comment.
        let switched_on = TEMPLATE
            .lines()
            .filter_map(|l| l.strip_prefix("# "))
            .filter(|l| {
                l.starts_with("[[rule]]")
                    || l.split_once(" = ")
                        .is_some_and(|(k, _)| k.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let all: RuleFile = toml::from_str(&switched_on).unwrap();
        let rules = Rules { rules: all.rules };
        rules
            .validate()
            .expect("every example rule in the template must be valid");
        assert!(rules.rules.iter().any(|r| r.name == "not-staff-plus"));
    }

    #[test]
    fn explain_reports_on_every_rule_not_just_the_winner() {
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]

            [[rule]]
            name = "remote-ok"
            location = ["remote"]
        "#,
        );
        let verdicts = r.explain(&role("Rust Engineer", "Berlin", "Astral"));
        assert_eq!(verdicts.len(), 2);
        assert!(verdicts[0].1.is_some());
        assert!(verdicts[1].1.is_none());
    }

    #[test]
    fn case_folding_never_shifts_the_quoted_span() {
        // `İ` grows a byte when lowercased and `ẞ` shrinks one, so a title
        // holding both defeats any total-length guard. The quoted reason still
        // has to be a real substring of the title, and nothing may panic.
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        for title in [
            "İ Rust ẞ",
            "ẞ Rust İ",
            "İİİ Rust",
            "Ünïcödé Rust ẞ Engineer",
            "İSTANBUL Rust",
            "日本語 Rust エンジニア",
        ] {
            let why = r.why(&role(title, "Remote", "Someone")).unwrap();
            let quoted = why.because.split('\'').nth(1).unwrap();
            assert!(
                title.contains(quoted),
                "reason quoted {quoted:?}, which is not in {title:?}"
            );
            assert_eq!(quoted.to_lowercase(), "rust");
        }
    }

    #[test]
    fn a_letter_is_a_letter_whatever_alphabet_it_is_from() {
        let r = rules(
            r#"
            [[rule]]
            name = "rust-in-title"
            title = ["rust"]
        "#,
        );
        assert!(r.why(&role("Ürust Engineer", "Remote", "X")).is_none());
        assert!(r.why(&role("rustö Engineer", "Remote", "X")).is_none());
        assert!(r.why(&role("Rust エンジニア", "Remote", "X")).is_some());
    }

    #[test]
    fn a_blank_entry_is_refused_rather_than_quietly_changing_the_rule() {
        // An empty exclude would match every role; an empty positive could
        // never fire. Both have to be visible errors, not silent behaviour.
        for text in [
            r#"[[rule]]
name = "a"
title_excludes = [""]"#,
            r#"[[rule]]
name = "a"
title = [""]"#,
            r#"[[rule]]
name = "a"
title = ["engineer", "  "]"#,
        ] {
            let file: RuleFile = toml::from_str(text).unwrap();
            let err = (Rules { rules: file.rules })
                .validate()
                .unwrap_err()
                .to_string();
            assert!(err.contains("empty entry"), "{text} gave: {err}");
        }
    }

    #[test]
    fn a_hand_built_rule_with_nothing_in_it_never_fires() {
        let rule = Rule {
            name: "wildcard".into(),
            ..Default::default()
        };
        assert!(rule
            .fires(&role("Anything", "Anywhere", "Someone"))
            .is_none());
    }

    #[test]
    fn the_company_reason_states_only_what_the_rules_layer_can_know() {
        // Whether the person still watches the company is not something the
        // matcher can see, so it must not claim to.
        let r = rules(
            r#"
            [[rule]]
            name = "watched-company"
            company = ["figma"]
        "#,
        );
        let why = r.why(&role("Engineer", "Remote", "Figma")).unwrap();
        assert_eq!(why.because, "the company is Figma");
    }
}
