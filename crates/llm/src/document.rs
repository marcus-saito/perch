//! Getting text out of a résumé.
//!
//! Everything downstream anchors against this text, so the extracted text has
//! to be *the document*. If a value cannot be found in it, Perch will not offer
//! that value. A bad extraction costs a missing field, not a wrong one.

use crate::error::{Error, Result};
use std::path::Path;

/// Where the text came from, so the interface can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Markdown,
    Pdf,
}

impl Kind {
    pub fn of(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("txt") => Some(Kind::Text),
            Some("md" | "markdown") => Some(Kind::Markdown),
            Some("pdf") => Some(Kind::Pdf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub text: String,
    pub kind: Kind,
    pub name: String,
}

/// PDF text extraction produces stray spacing and hyphenation. Tidy it enough
/// to read without moving any words: the words are the evidence.
fn tidy(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\n\n\n", "\n\n")
        .trim()
        .to_string()
}

pub fn read(path: &Path) -> Result<Document> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("the document")
        .to_string();

    let Some(kind) = Kind::of(path) else {
        return Err(Error::msg(format!(
            "Perch can read {name} only if it is a .pdf, .txt or .md file"
        )));
    };

    let text = match kind {
        Kind::Text | Kind::Markdown => std::fs::read_to_string(path)?,
        Kind::Pdf => pdf_extract::extract_text(path).map_err(|err| {
            Error::msg(format!(
                "Could not read the text out of {name}: {err}. A PDF that is a scan has no text in it. Exporting it as text works."
            ))
        })?,
    };

    let text = tidy(&text);
    if text.split_whitespace().count() < 20 {
        return Err(Error::msg(format!(
            "There is almost no text in {name}. If it is a scan, exporting it as text works."
        )));
    }
    Ok(Document { text, kind, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_formats_perch_can_actually_read_are_accepted() {
        assert_eq!(Kind::of(Path::new("cv.pdf")), Some(Kind::Pdf));
        assert_eq!(Kind::of(Path::new("cv.PDF")), Some(Kind::Pdf));
        assert_eq!(Kind::of(Path::new("cv.md")), Some(Kind::Markdown));
        assert_eq!(Kind::of(Path::new("cv.txt")), Some(Kind::Text));
        assert_eq!(Kind::of(Path::new("cv.docx")), None);
        assert_eq!(Kind::of(Path::new("cv")), None);
    }

    #[test]
    fn tidying_does_not_move_any_words() {
        let messy = "  DANA   FERREIRA \n\n\n  Cloudflare —   Senior Engineer  \n";
        let tidied = tidy(messy);
        assert_eq!(tidied, "DANA FERREIRA\n\nCloudflare — Senior Engineer");
        let before: Vec<&str> = messy.split_whitespace().collect();
        let after: Vec<&str> = tidied.split_whitespace().collect();
        assert_eq!(before, after, "tidying changed the words themselves");
    }

    #[test]
    fn a_document_with_nothing_in_it_says_so_plainly() {
        let dir = std::env::temp_dir().join(format!("perch-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, "a scan, probably").unwrap();
        let err = read(&path).unwrap_err().to_string();
        assert!(err.contains("almost no text"), "{err}");
        assert!(!err.contains("Error"), "the message must read as ordinary");

        let unsupported = dir.join("cv.docx");
        std::fs::write(&unsupported, "x").unwrap();
        assert!(read(&unsupported).unwrap_err().to_string().contains(".pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plain_text_resume_reads_straight_through() {
        let dir = std::env::temp_dir().join(format!("perch-doc-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cv.txt");
        let body = "DANA FERREIRA\ndana@dferreira.dev\n\nCloudflare — Senior Software Engineer, Storage\nMarch 2023 to February 2026\nBuilt the replication layer for a globally distributed object store.";
        std::fs::write(&path, body).unwrap();
        let doc = read(&path).unwrap();
        assert_eq!(doc.kind, Kind::Text);
        assert_eq!(doc.name, "cv.txt");
        assert!(doc.text.contains("Cloudflare"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
