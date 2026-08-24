//! Error types for the Edge SDK.

use std::fmt;

/// The common error type returned by Edge SDK APIs.
///
/// Variants are normalized across platforms: adapters map platform-native
/// failures into these categories so handlers can branch on semantics rather
/// than platforms.
#[derive(Debug)]
pub enum Error {
    /// A subrequest (`Context::fetch`) failed.
    Fetch(FetchError),
    /// A KV operation failed.
    Kv(KvError),
    /// Configuration lookup or validation failed.
    Config(String),
    /// Routing failed.
    Router(PathError),
    /// Body conversion or buffering failed.
    Body(std::io::Error),
    /// An unexpected internal failure.
    Internal(String),
}

/// Errors from `Context::fetch` (subrequests).
///
/// Categories are semantic and platform-independent; the mapping from
/// platform-native failures to these categories is an adapter responsibility
/// (see SPEC §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The URL host has no backend and dynamic-backend fallback is disabled.
    UnresolvedBackend(String),
    /// Connection-level failure: DNS, refused, reset.
    Connection(String),
    /// TLS handshake or certificate failure.
    Tls(String),
    /// The request timed out.
    Timeout,
    /// The platform denied the request (permissions, allowlist, disabled
    /// dynamic backends).
    Permission,
    /// The request or URL was malformed.
    BadRequest(String),
    /// Any platform-specific failure, prefixed with the platform name.
    Platform(String),
}

/// Errors from KV operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvError {
    /// A platform-specific KV failure.
    Platform(String),
}

/// Errors from [`crate::Router`] matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// No route matched the request.
    NotFound,
    /// The route pattern could not be registered.
    InvalidPattern(String),
}

impl FetchError {
    /// The category name, stable for serialization and cross-platform
    /// assertions (SPEC §11 T6: same `FetchError` category on both
    /// platforms).
    pub fn category(&self) -> &'static str {
        match self {
            FetchError::UnresolvedBackend(_) => "UnresolvedBackend",
            FetchError::Connection(_) => "Connection",
            FetchError::Tls(_) => "Tls",
            FetchError::Timeout => "Timeout",
            FetchError::Permission => "Permission",
            FetchError::BadRequest(_) => "BadRequest",
            FetchError::Platform(_) => "Platform",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Fetch(e) => write!(f, "fetch failed: {e}"),
            Error::Kv(e) => write!(f, "kv failed: {e}"),
            Error::Config(e) => write!(f, "config error: {e}"),
            Error::Router(e) => write!(f, "routing error: {e}"),
            Error::Body(e) => write!(f, "body error: {e}"),
            Error::Internal(e) => write!(f, "internal error: {e}"),
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::UnresolvedBackend(host) => {
                write!(f, "no backend resolves host `{host}`")
            }
            FetchError::Connection(e) => write!(f, "connection failure: {e}"),
            FetchError::Tls(e) => write!(f, "TLS failure: {e}"),
            FetchError::Timeout => f.write_str("request timed out"),
            FetchError::Permission => f.write_str("request denied by platform policy"),
            FetchError::BadRequest(e) => write!(f, "bad request: {e}"),
            FetchError::Platform(e) => f.write_str(e),
        }
    }
}

impl fmt::Display for KvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KvError::Platform(e) => f.write_str(e),
        }
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::NotFound => f.write_str("no route matched"),
            PathError::InvalidPattern(e) => write!(f, "invalid route pattern: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Fetch(e) => Some(e),
            Error::Kv(e) => Some(e),
            Error::Body(e) => Some(e),
            _ => None,
        }
    }
}

impl std::error::Error for FetchError {}
impl std::error::Error for KvError {}
impl std::error::Error for PathError {}

impl From<FetchError> for Error {
    fn from(e: FetchError) -> Self {
        Error::Fetch(e)
    }
}

impl From<KvError> for Error {
    fn from(e: KvError) -> Self {
        Error::Kv(e)
    }
}

impl From<PathError> for Error {
    fn from(e: PathError) -> Self {
        Error::Router(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Body(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn display_strings_are_informative() {
        assert_eq!(
            Error::Fetch(FetchError::UnresolvedBackend("api.example.com".into())).to_string(),
            "fetch failed: no backend resolves host `api.example.com`"
        );
        assert_eq!(
            Error::Router(PathError::NotFound).to_string(),
            "routing error: no route matched"
        );
        assert_eq!(
            Error::Kv(KvError::Platform("boom".into())).to_string(),
            "kv failed: boom"
        );
        assert_eq!(Error::Internal("x".into()).to_string(), "internal error: x");
    }

    #[test]
    fn category_names_are_stable() {
        assert_eq!(
            FetchError::UnresolvedBackend("h".into()).category(),
            "UnresolvedBackend"
        );
        assert_eq!(FetchError::Connection("x".into()).category(), "Connection");
        assert_eq!(FetchError::Timeout.category(), "Timeout");
        assert_eq!(FetchError::Permission.category(), "Permission");
        assert_eq!(FetchError::BadRequest("x".into()).category(), "BadRequest");
        assert_eq!(FetchError::Tls("x".into()).category(), "Tls");
        assert_eq!(FetchError::Platform("x".into()).category(), "Platform");
    }

    #[test]
    fn from_conversions_wrap_variants() {
        assert!(matches!(
            Error::from(FetchError::Timeout),
            Error::Fetch(FetchError::Timeout)
        ));
        assert!(matches!(
            Error::from(KvError::Platform("x".into())),
            Error::Kv(_)
        ));
        assert!(matches!(
            Error::from(PathError::NotFound),
            Error::Router(PathError::NotFound)
        ));
    }

    #[test]
    fn error_source_chains_inner_errors() {
        let io_err = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad utf8");
        let err = Error::from(io_err);
        assert!(err.source().is_some());
        assert!(matches!(err, Error::Body(_)));
    }
}
