use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// The file is readable but is not something this version understands.
    #[error("{0}")]
    Format(String),
    /// An edit that the graph rules do not allow (e.g. wiring a flow port to a data port).
    #[error("{0}")]
    Graph(String),
}

impl Error {
    pub fn format(msg: impl Into<String>) -> Self {
        Error::Format(msg.into())
    }

    pub fn graph(msg: impl Into<String>) -> Self {
        Error::Graph(msg.into())
    }
}
