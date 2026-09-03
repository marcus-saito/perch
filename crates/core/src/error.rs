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
        source: toml::de::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(m: impl fmt::Display) -> Self {
        Error::Msg(m.to_string())
    }
}
