// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Structured operation failures, translated to legacy codes only at boundaries.
use std::{error, fmt, io, sync::Arc};

/// Stable categories for handling failures without parsing diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// Invalid arguments, identifiers or configuration.
    InvalidInput,
    /// Network connection, TLS or transfer failure.
    Transport,
    /// An unsuccessful HTTP response.
    Http,
    /// Invalid resource metadata or protocol data.
    Metadata,
    /// Filesystem or archive I/O failure.
    Io,
    /// A configured work or resource budget was exceeded.
    Limit,
    /// A remote or legacy operation failed without more specific information.
    Operation,
}

/// An operation failure retaining its category, HTTP status and underlying cause.
#[derive(Clone, Debug)]
pub struct Error {
    /// Machine-readable failure category.
    pub kind: ErrorKind,
    /// Human-readable context, excluding credentials and request bodies.
    pub message: String,
    /// HTTP status, when the failure came from a server response.
    pub status: Option<u16>,
    source: Option<Arc<dyn error::Error + Send + Sync>>,
}
impl Error {
    /// Construct a categorized failure without an underlying cause.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            source: None,
        }
    }
    /// Retain an underlying error while adding operation context.
    pub fn with_source(
        kind: ErrorKind,
        message: impl Into<String>,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            source: Some(Arc::new(source)),
        }
    }
    /// Construct an HTTP response failure without including private response data.
    pub fn http(status: u16, context: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Http,
            message: context.into(),
            status: Some(status),
            source: None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(status) = self.status {
            write!(f, " (HTTP {status})")?;
        }
        Ok(())
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}
impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Self::with_source(ErrorKind::Io, source.to_string(), source)
    }
}
impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Self::with_source(ErrorKind::Metadata, source.to_string(), source)
    }
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::Operation, message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::new(ErrorKind::Operation, message)
    }
}
/// Compatibility constructor for the initial native `Error(String)` spelling.
#[allow(non_snake_case)]
pub fn Error(message: String) -> Error {
    Error::new(ErrorKind::Operation, message)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_status_context_and_sources() {
        let error = Error::http(429, "Rate limited");
        assert_eq!(error.status, Some(429));
        assert_eq!(error.to_string(), "Rate limited (HTTP 429)");
        let error = Error::from(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert_eq!(error.kind, ErrorKind::Io);
        assert!(error::Error::source(&error).is_some());
        assert!(error.to_string().contains("denied"));
        let error = Error::from(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert_eq!(error.kind, ErrorKind::Metadata);
        assert!(error::Error::source(&error).is_some());
        assert_eq!(Error::from("message").message, "message");
        assert_eq!(Error::from("message".to_owned()).kind, ErrorKind::Operation);
    }
}
