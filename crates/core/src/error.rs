use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),

    #[error("no public job board found for {0}")]
    BoardNotFound(String),

    #[error("{0} did not answer with a job listing")]
    BoardUnreadable(String),

    #[error("{0} is already on the watchlist")]
    AlreadyWatched(String),

    #[error("{0} is not on the watchlist")]
    NotWatched(String),

    #[error("no role here with the reference {0}")]
    NoSuchRole(String),

    #[error("{0} matches more than one role; use the full reference")]
    AmbiguousRole(String),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    // The source is declared below, so the message must not repeat it or the
    // CLI's error chain prints the same parse error twice.
    #[error("could not read {path}")]
    Toml {
        path: String,
        #[source]
        source: TomlComplaint,
    },
}

/// What went wrong in a TOML file, said without quoting the file.
///
/// `toml::de::Error` renders the line it failed on, and a type error quotes the
/// offending value. Either one puts the contents of the file on the screen.
/// These are files a person is invited to open and paste into, and a key pasted
/// into one by mistake must not come back out through an error message. The
/// line number and the bare complaint are what is needed to fix the file, so
/// they are what is kept.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TomlComplaint(String);

impl TomlComplaint {
    /// `text` is the file the error came from, and is read only to count the
    /// newlines before the failure.
    pub fn new(source: &toml::de::Error, text: &str) -> Self {
        let said = with_quoted_values_removed(source.message());
        match source.span() {
            Some(span) => {
                let before = text.get(..span.start).unwrap_or(text);
                let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
                Self(format!("line {line}: {said}"))
            }
            None => Self(said),
        }
    }
}

/// Every quoted run replaced. TOML quotes the value it is complaining about,
/// and that value is the one thing here that must not reach the screen.
fn with_quoted_values_removed(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    for (i, between) in message.split('"').enumerate() {
        if i % 2 == 0 {
            out.push_str(between);
        } else {
            out.push_str("\"…\"");
        }
    }
    out
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(m: impl fmt::Display) -> Self {
        Error::Msg(m.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Config {
        #[allow(dead_code)]
        endpoint: String,
        #[allow(dead_code)]
        allowed: bool,
    }

    fn complaint_about(text: &str) -> String {
        let failure = toml::from_str::<Config>(text).expect_err("this text should not parse");
        TomlComplaint::new(&failure, text).to_string()
    }

    #[test]
    fn a_key_pasted_into_the_file_is_not_in_the_complaint_about_it() {
        // The accident this guards against: someone follows the convention
        // every other tool uses, writes their key into the config file, and the
        // parse error prints that line back to the terminal.
        let said = complaint_about("endpoint = \"x\"\napi_key = \"sk-must-not-appear\"\n");
        assert!(!said.contains("sk-must-not-appear"), "{said}");
        assert!(said.contains("unknown field `api_key`"), "{said}");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_complained_about_without_quoting_it() {
        // TOML names the value it rejected, so the complaint is kept and the
        // value inside it is not.
        let said = complaint_about("endpoint = \"x\"\nallowed = \"sk-must-not-appear\"\n");
        assert!(!said.contains("sk-must-not-appear"), "{said}");
        assert!(said.contains("expected a boolean"), "{said}");
    }

    #[test]
    fn the_complaint_names_the_line_the_failure_is_on() {
        let said = complaint_about("endpoint = \"x\"\nallowed = true\n\napi_key = \"k\"\n");
        assert!(said.starts_with("line 4:"), "{said}");
    }
}
