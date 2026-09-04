//! `profile.toml` is a plain text file the person owns and can edit by hand.
//!
//! Two things are deliberately absent and must stay absent: anything
//! demographic, and anything secret. EEO answers are never filled and never
//! stored, so there is nowhere in this struct to keep one. API keys live in the
//! system keychain, so there is nowhere here to keep one of those either.

use crate::error::{Error, Result, TomlComplaint};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    /// In the person's own words, not a dropdown Perch invented.
    pub work_authorisation: String,
    pub links: Links,
    /// Free-form, in the person's own words. Used to fill "skills" boxes.
    pub skills: Vec<String>,
    pub experience: Vec<Position>,
    pub documents: Vec<Document>,
}

/// One role held, exactly as the résumé states it. Perch does not interpret
/// dates or infer seniority; it keeps what the document said.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Position {
    pub company: String,
    pub title: String,
    pub dates: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Links {
    pub github: String,
    pub website: String,
    pub linkedin: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Document {
    pub name: String,
    pub path: String,
    /// "résumé" or "letter": what it is, not what it scores.
    pub kind: String,
}

impl Profile {
    /// An absent profile is not an error. Watching and syncing need nothing
    /// from it, so Perch runs perfectly well before anyone fills it in.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::Toml {
                path: path.display().to_string(),
                source: TomlComplaint::new(&source, &text),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    /// Write the profile back as TOML.
    ///
    /// Pretty-printed rather than compact, so the file stays something a
    /// person can open and edit.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|err| Error::msg(format!("could not write the profile: {err}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// The document to attach to an application, unless the person says
    /// otherwise: the first one whose kind reads as a résumé, and failing
    /// that the first one listed. Defined once so the command line and the
    /// desktop cannot quietly attach different files from the same profile.
    pub fn preferred_resume(&self) -> Option<&Document> {
        self.documents
            .iter()
            .find(|d| {
                let kind = d.kind.to_lowercase();
                kind.contains("sum") || kind.contains("cv")
            })
            .or_else(|| self.documents.first())
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.email.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_profile_is_an_empty_one_not_a_failure() {
        let profile = Profile::load(Path::new("/nowhere/at/all/profile.toml")).unwrap();
        assert!(profile.is_empty());
    }

    #[test]
    fn a_hand_written_profile_round_trips() {
        let text = r#"
            name = "Dana Ferreira"
            email = "dana@dferreira.dev"
            location = "Oakland, California"
            work_authorisation = "US citizen. I don't need sponsorship now or later."

            [links]
            github = "github.com/dferreira"

            [[documents]]
            name = "resume-systems.pdf"
            path = "~/documents/resume-systems.pdf"
            kind = "résumé"
        "#;
        let profile: Profile = toml::from_str(text).unwrap();
        assert_eq!(profile.name, "Dana Ferreira");
        assert_eq!(profile.links.github, "github.com/dferreira");
        assert_eq!(profile.documents.len(), 1);
        assert!(!profile.is_empty());
    }

    #[test]
    fn a_saved_profile_reads_back_the_same_and_stays_editable() {
        let dir = std::env::temp_dir().join(format!("perch-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.toml");

        let profile = Profile {
            name: "Dana Ferreira".into(),
            email: "dana@dferreira.dev".into(),
            work_authorisation: "US citizen. I don't need sponsorship now or later.".into(),
            skills: vec!["Rust".into(), "Tokio".into()],
            experience: vec![Position {
                company: "Cloudflare".into(),
                title: "Senior Software Engineer, Storage".into(),
                dates: "March 2023 – February 2026".into(),
            }],
            ..Profile::default()
        };
        profile.save(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"Dana Ferreira\""), "{text}");
        let back = Profile::load(&path).unwrap();
        assert_eq!(back.name, profile.name);
        assert_eq!(back.skills, profile.skills);
        assert_eq!(back.experience[0].company, "Cloudflare");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_resume_is_chosen_by_kind_not_by_position() {
        let profile: Profile = toml::from_str(
            r#"
            [[documents]]
            name = "letter-opening.md"
            path = "~/docs/letter-opening.md"
            kind = "letter"

            [[documents]]
            name = "resume-systems.pdf"
            path = "~/docs/resume-systems.pdf"
            kind = "résumé"
            "#,
        )
        .unwrap();
        assert_eq!(
            profile.preferred_resume().unwrap().name,
            "resume-systems.pdf"
        );

        // With nothing that reads as a résumé, the first is better than none.
        let letters: Profile = toml::from_str(
            r#"
            [[documents]]
            name = "letter-opening.md"
            path = "~/docs/letter-opening.md"
            kind = "letter"
            "#,
        )
        .unwrap();
        assert_eq!(
            letters.preferred_resume().unwrap().name,
            "letter-opening.md"
        );
        assert!(Profile::default().preferred_resume().is_none());
    }

    #[test]
    fn experience_and_skills_round_trip() {
        let text = r#"
            name = "Dana Ferreira"
            skills = ["Rust", "Tokio"]

            [[experience]]
            company = "Cloudflare"
            title = "Senior Software Engineer, Storage"
            dates = "March 2023 – February 2026"
        "#;
        let profile: Profile = toml::from_str(text).unwrap();
        assert_eq!(profile.skills, ["Rust", "Tokio"]);
        assert_eq!(profile.experience[0].company, "Cloudflare");
    }

    #[test]
    fn there_is_nowhere_to_put_a_demographic_answer() {
        // deny_unknown_fields is what makes this true rather than merely
        // intended: a profile carrying EEO data fails to load at all.
        let text = r#"
            name = "Dana Ferreira"
            gender = "prefer not to say"
        "#;
        assert!(toml::from_str::<Profile>(text).is_err());
    }
}
