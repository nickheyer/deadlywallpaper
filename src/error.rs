use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    NotFound(String),
    Unsupported(String),
    Invalid(String),
    Media(String),
    Web(String),
    Platform(String),
    Ipc(String),
    Network(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Json(_) => "json",
            Error::NotFound(_) => "not-found",
            Error::Unsupported(_) => "unsupported",
            Error::Invalid(_) => "invalid",
            Error::Media(_) => "media",
            Error::Web(_) => "web",
            Error::Platform(_) => "platform",
            Error::Ipc(_) => "ipc",
            Error::Network(_) => "network",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "{e}"),
            Error::NotFound(m)
            | Error::Unsupported(m)
            | Error::Invalid(m)
            | Error::Media(m)
            | Error::Web(m)
            | Error::Platform(m)
            | Error::Ipc(m)
            | Error::Network(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// Attach a path or subject to an I/O error message.
pub fn ctx<T>(r: std::io::Result<T>, what: impl fmt::Display) -> Result<T> {
    r.map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{what}: {e}"))))
}
