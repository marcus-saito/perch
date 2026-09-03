//! Printing. The CLI reads the way the interface reads: age is the only
//! hierarchy, nothing is scored, and an empty result is a fact rather than a
//! failure.

use perch_core::{Bucket, Freshness, Role, Why};

use std::io::IsTerminal;
use time::OffsetDateTime;

/// Colour only when a person is watching. Piped output stays plain text.
pub struct Style {
    on: bool,
}

impl Style {
    pub fn detect() -> Self {
        let disabled = std::env::var_os("NO_COLOR").is_some();
        Self {
            on: !disabled && std::io::stdout().is_terminal(),
        }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }
    pub fn faint_italic(&self, text: &str) -> String {
        self.wrap("2;3", text)
    }
    /// The one accent, spent only on the freshest time signal.
    pub fn accent(&self, text: &str) -> String {
        self.wrap("38;5;173", text)
    }
    pub fn heading(&self, text: &str) -> String {
        self.wrap("2;4", text)
    }
    pub fn bright(&self, text: &str) -> String {
        self.wrap("0", text)
    }

    /// A rule name, printed the way the interface prints it: set apart from the
    /// prose around it, never coloured.
    pub fn rule(&self, text: &str) -> String {
        self.wrap("1;2", text)
    }

    /// Warm and full strength when new; quiet when it has been sitting there.
    pub fn by_freshness(&self, freshness: Freshness, text: &str) -> String {
        match freshness {
            Freshness::Fresh => self.accent(text),
            Freshness::Recent => self.bright(text),
            Freshness::Settled => self.dim(text),
            Freshness::Stale => self.dim(text),
            Freshness::Tired => self.faint_italic(text),
        }
    }

    pub fn title_by_freshness(&self, freshness: Freshness, text: &str) -> String {
        match freshness {
            Freshness::Fresh | Freshness::Recent => self.bright(text),
            _ => self.dim(text),
        }
    }
}

pub fn print_feed(roles: &[(Role, Option<Why>)], style: &Style, now: OffsetDateTime) {
    let mut bucket: Option<Bucket> = None;

    for (role, why) in roles {
        let signal = role.signal(now);
        let this = signal.bucket(now);
        if bucket != Some(this) {
            println!();
            println!("  {}", style.heading(this.heading()));
            println!();
            bucket = Some(this);
        }

        let freshness = signal.freshness(now);

        let mut meta = Vec::new();
        if !role.location.is_empty() {
            meta.push(role.location.clone());
        }
        meta.push(style.by_freshness(freshness, &signal.text));
        if !role.fill_supported {
            meta.push(style.dim("opens in browser"));
        }

        println!("  {}", style.dim(&role.company_name));
        println!(
            "  {}  {}",
            style.title_by_freshness(freshness, &role.title),
            style.dim(&role.reference)
        );
        println!("  {}", meta.join(style.dim(" · ").as_str()));

        // Every row that survived a rule says which one, and what set it off.
        if let Some(why) = why {
            println!(
                "  {} {}{}",
                style.dim("matched"),
                style.rule(&why.rule),
                style.dim(&format!(": {}", why.because))
            );
        }
        println!();
    }
}
