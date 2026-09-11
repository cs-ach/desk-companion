use std::path::PathBuf;

use rquickjs::CaughtError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Js {
        message: String,
        stack: Option<String>,
    },

    #[error(transparent)]
    Runtime(#[from] rquickjs::Error),

    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Error {
    pub fn stack(&self) -> Option<&str> {
        match self {
            Error::Js { stack, .. } => stack.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn from_caught(error: CaughtError<'_>) -> Self {
        match error {
            CaughtError::Error(e) => Error::Runtime(e),

            CaughtError::Exception(exception) => Error::Js {
                message: exception
                    .message()
                    .unwrap_or_else(|| "uncaught exception".to_string()),
                stack: exception.stack(),
            },

            CaughtError::Value(value) => Error::Js {
                message: value
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| format!("{value:?}")),
                stack: None,
            },
        }
    }
}
