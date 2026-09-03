#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),

    #[error("no model is configured, so there is nothing to read the résumé with")]
    NoModel,

    #[error("{0} is not on this Mac, and you have not said your résumé may be sent there")]
    ConsentMissing(String),

    #[error("could not read {path}")]
    Config {
        path: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("{0} answered with a redirect rather than a reply, and Perch does not follow one")]
    Redirected(String),

    /// A remote endpoint turned the request away. Named separately because the
    /// remedy is a key rather than anything about the résumé or the model, and
    /// reqwest's own words for it do not say that.
    #[error("{host} would not take the request without a key it accepts")]
    Unauthorized { host: String },

    #[error("the model's reply was not the shape Perch asked for")]
    BadShape,

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(m: impl std::fmt::Display) -> Self {
        Error::Msg(m.to_string())
    }
}
