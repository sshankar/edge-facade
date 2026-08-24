//! Shared service configuration (SPEC §7.2).
//!
//! A single `edge.toml` is the source of truth for both platforms. This
//! module parses and validates it; the runtime embed is the responsibility of
//! the entry macro (`#[edge_core::main]`, which embeds the file via
//! `include_str!` at the service crate root). `edge-cli` (SPEC §9) will
//! generate the per-platform configs from the same schema in M5.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

use crate::FetchError;

/// Errors from parsing or validating `edge.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "edge.toml: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

fn err(msg: impl Into<String>) -> ConfigError {
    ConfigError(msg.into())
}

/// The parsed service manifest. Field order matches SPEC §7.2.
#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    service: RawService,
    #[serde(default)]
    origins: HashMap<String, RawOrigin>,
    #[serde(default)]
    stores: RawStores,
    #[serde(default)]
    logging: RawLogging,
    /// Required: dynamic-backend fallback MUST be explicit (SPEC D4).
    fastly: RawFastly,
}

#[derive(Debug, Clone, Deserialize)]
struct RawService {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawOrigin {
    url: String,
    backend: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawStores {
    kv: Option<String>,
    config: Option<String>,
    secrets: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawLogging {
    endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFastly {
    dynamic_backends: bool,
}

/// Outcome of resolving a fetch URL host (SPEC §7.3).
///
/// The resolution *policy* is part of the shared config model — the
/// `[fastly] dynamic_backends` switch is declared in `edge.toml` — so the
/// decision is testable in core; the adapters map it to transport
/// (fastly `Backend::from_name` / `Backend::builder`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Step 1: host declared in `[origins]`; send to this backend name.
    Static(String),
    /// Step 2: dynamic-backend fallback, only when
    /// `[fastly] dynamic_backends = true` (decision D4).
    Dynamic {
        /// The URL host.
        host: String,
        /// Effective port (URL port, else 443/80 by scheme).
        port: u16,
        /// Whether the URL scheme is `https` (drives SSL/SNI).
        https: bool,
    },
    /// Step 3: fail closed — host undeclared and fallback disabled.
    Unresolved(String),
}

/// A validated origin, keyed by URL host for runtime resolution (SPEC §7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostOrigin {
    /// The backend name to send to (`fastly.toml` static backend).
    pub backend: String,
}

/// The validated service configuration.
#[derive(Debug, Clone)]
pub struct EdgeConfig {
    service: Service,
    origins: HashMap<String, Origin>,
    stores: Stores,
    logging: Logging,
    fastly: Fastly,
    origins_by_host: HashMap<String, HostOrigin>,
}

/// Service metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Service name, used by codegen (`fastly.toml` / `wrangler.toml`).
    pub name: String,
}

/// An origin alias as written in `edge.toml` (used by `edge-cli` codegen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The origin's URL (parsed at validation).
    pub url: url::Url,
    /// Backend name for this origin.
    pub backend: String,
}

/// Store bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stores {
    /// KV store bound to the default `Context::kv()` handle.
    pub kv: Option<String>,
    /// Config store for `Context::var`.
    pub config: Option<String>,
    /// Secret store for `Context::secret`.
    pub secrets: Option<String>,
}

/// Logging configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Logging {
    /// Fastly log endpoint name; `Context::log` falls back to `eprintln!`.
    pub endpoint: Option<String>,
}

/// Fastly-specific settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fastly {
    /// Allow dynamic-backend fallback for undeclared fetch hosts (SPEC §7.3).
    pub dynamic_backends: bool,
}

impl EdgeConfig {
    /// Parse and validate an `edge.toml` document.
    pub fn from_toml_str(s: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig =
            toml::from_str(s).map_err(|e| err(format!("invalid edge.toml: {e}")))?;
        raw.into_validated()
    }

    /// The service name.
    pub fn service(&self) -> &Service {
        &self.service
    }

    /// Look up an origin by its alias.
    pub fn origin(&self, alias: &str) -> Option<&Origin> {
        self.origins.get(alias)
    }

    /// Iterate over origin aliases and origins.
    pub fn origins(&self) -> impl Iterator<Item = (&str, &Origin)> {
        self.origins.iter().map(|(a, o)| (a.as_str(), o))
    }

    /// Resolve a fetch URL to a backend decision (SPEC §7.3).
    ///
    /// Fails with [`FetchError::BadRequest`] for relative URIs; otherwise
    /// returns the static-match, dynamic-fallback, or fail-closed outcome.
    pub fn resolve(&self, uri: &http::Uri) -> Result<Resolution, FetchError> {
        let host = uri
            .host()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| FetchError::BadRequest("fetch request URI has no host".to_string()))?
            .to_string();

        // Step 1: static origin map.
        if let Some(origin) = self.origins_by_host.get(&host) {
            return Ok(Resolution::Static(origin.backend.clone()));
        }

        // Step 2: dynamic fallback, opt-in only (D4).
        if self.fastly.dynamic_backends {
            let scheme = uri.scheme_str().unwrap_or("http");
            let port = uri
                .port_u16()
                .unwrap_or(if scheme == "https" { 443 } else { 80 });
            return Ok(Resolution::Dynamic {
                host,
                port,
                https: scheme == "https",
            });
        }

        // Step 3: fail closed.
        Ok(Resolution::Unresolved(host))
    }

    /// Resolve a fetch URL host to its static backend (SPEC §7.3 step 1).
    ///
    /// Returns `None` when the host is undeclared; the adapter then applies
    /// the dynamic fallback or fails closed (steps 2–3).
    pub fn resolve_host(&self, host: &str) -> Option<&HostOrigin> {
        self.origins_by_host.get(host)
    }

    /// Store bindings.
    pub fn stores(&self) -> &Stores {
        &self.stores
    }

    /// Logging configuration.
    pub fn logging(&self) -> &Logging {
        &self.logging
    }

    /// Fastly-specific settings.
    pub fn fastly(&self) -> &Fastly {
        &self.fastly
    }
}

impl RawConfig {
    fn into_validated(self) -> Result<EdgeConfig, ConfigError> {
        let name = self.service.name.trim();
        if name.is_empty() {
            return Err(err("`[service] name` must not be empty"));
        }

        let mut origins = HashMap::new();
        let mut origins_by_host: HashMap<String, HostOrigin> = HashMap::new();
        for (alias, origin) in &self.origins {
            if alias.trim().is_empty() {
                return Err(err("origin alias must not be empty"));
            }
            let url = url::Url::parse(&origin.url)
                .map_err(|e| err(format!("origin `{alias}`: invalid url: {e}")))?;
            match url.scheme() {
                "http" | "https" => {}
                other => {
                    return Err(err(format!(
                        "origin `{alias}`: unsupported scheme `{other}` (http/https only)"
                    )))
                }
            }
            let host = url
                .host_str()
                .ok_or_else(|| err(format!("origin `{alias}`: url has no host")))?
                .to_string();
            if origin.backend.trim().is_empty() {
                return Err(err(format!("origin `{alias}`: backend must not be empty")));
            }
            if origins_by_host
                .insert(
                    host.clone(),
                    HostOrigin {
                        backend: origin.backend.clone(),
                    },
                )
                .is_some()
            {
                return Err(err(format!(
                    "origin `{alias}`: host `{host}` already declared by another origin"
                )));
            }
            origins.insert(
                alias.clone(),
                Origin {
                    url,
                    backend: origin.backend.clone(),
                },
            );
        }

        for (what, value) in [
            ("[stores] kv", &self.stores.kv),
            ("[stores] config", &self.stores.config),
            ("[stores] secrets", &self.stores.secrets),
            ("[logging] endpoint", &self.logging.endpoint),
        ] {
            if let Some(v) = value {
                if v.trim().is_empty() {
                    return Err(err(format!("{what} must not be empty")));
                }
            }
        }

        Ok(EdgeConfig {
            service: Service {
                name: name.to_string(),
            },
            origins,
            stores: Stores {
                kv: self.stores.kv,
                config: self.stores.config,
                secrets: self.stores.secrets,
            },
            logging: Logging {
                endpoint: self.logging.endpoint,
            },
            fastly: Fastly {
                dynamic_backends: self.fastly.dynamic_backends,
            },
            origins_by_host,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
[service]
name = "hello-world"

[origins]
api = { url = "https://api.example.com", backend = "api_backend" }
cdn = { url = "http://cdn.example.net:8080", backend = "cdn_backend" }

[stores]
kv = "edge_kv"
config = "edge_config"
secrets = "edge_secrets"

[logging]
endpoint = "default_logging"

[fastly]
dynamic_backends = false
"#;

    #[test]
    fn parses_and_validates() {
        let cfg = EdgeConfig::from_toml_str(VALID).unwrap();
        assert_eq!(cfg.service().name, "hello-world");
        assert_eq!(cfg.origins().count(), 2);
        assert_eq!(cfg.origin("api").unwrap().backend, "api_backend");
        assert_eq!(cfg.stores().kv.as_deref(), Some("edge_kv"));
        assert_eq!(cfg.logging().endpoint.as_deref(), Some("default_logging"));
        assert!(!cfg.fastly().dynamic_backends);

        // Resolution map is keyed by URL host.
        let api = cfg.resolve_host("api.example.com").unwrap();
        assert_eq!(api.backend, "api_backend");
        let cdn = cfg.resolve_host("cdn.example.net").unwrap();
        assert_eq!(cdn.backend, "cdn_backend");
        assert!(cfg.resolve_host("undeclared.example.com").is_none());
    }

    #[test]
    fn dynamic_backends_must_be_explicit() {
        let err =
            EdgeConfig::from_toml_str("[service]\nname = \"x\"\n[origins]\n[stores]\n[logging]\n")
                .unwrap_err();
        // Either the `[fastly]` table or its `dynamic_backends` field must
        // be spelled out.
        assert!(
            err.0.contains("fastly") || err.0.contains("dynamic_backends"),
            "got: {err}"
        );
    }

    #[test]
    fn duplicate_host_is_rejected() {
        let err = EdgeConfig::from_toml_str(
            r#"
[service]
name = "x"
[origins]
a = { url = "https://same.example.com", backend = "b1" }
b = { url = "https://same.example.com", backend = "b2" }
[fastly]
dynamic_backends = false
"#,
        )
        .unwrap_err();
        assert!(err.0.contains("already declared"), "got: {err}");
    }

    #[test]
    fn bad_scheme_is_rejected() {
        let err = EdgeConfig::from_toml_str(
            r#"
[service]
name = "x"
[origins]
a = { url = "ftp://example.com", backend = "b" }
[fastly]
dynamic_backends = false
"#,
        )
        .unwrap_err();
        assert!(err.0.contains("unsupported scheme"), "got: {err}");
    }

    #[test]
    fn empty_name_is_rejected() {
        let err = EdgeConfig::from_toml_str(
            "[service]\nname = \"\"\n[origins]\n[stores]\n[logging]\n[fastly]\ndynamic_backends = false\n",
        )
        .unwrap_err();
        assert!(err.0.contains("name"), "got: {err}");
    }

    #[test]
    fn error_implements_std_error() {
        let e = EdgeConfig::from_toml_str("not toml at all [[[").unwrap_err();
        let _: &dyn std::error::Error = &e;
        assert!(!e.to_string().is_empty());
    }

    fn cfg() -> EdgeConfig {
        EdgeConfig::from_toml_str(VALID).unwrap()
    }

    fn uri(s: &str) -> http::Uri {
        s.parse().unwrap()
    }

    #[test]
    fn resolve_static_match() {
        assert_eq!(
            cfg().resolve(&uri("https://api.example.com/v1")).unwrap(),
            Resolution::Static("api_backend".into())
        );
        assert_eq!(
            cfg()
                .resolve(&uri("http://cdn.example.net:8080/x"))
                .unwrap(),
            Resolution::Static("cdn_backend".into())
        );
    }

    #[test]
    fn resolve_fails_closed_when_dynamic_disabled() {
        // VALID has dynamic_backends = false.
        assert_eq!(
            cfg().resolve(&uri("https://other.example.com/")).unwrap(),
            Resolution::Unresolved("other.example.com".into())
        );
    }

    #[test]
    fn resolve_dynamic_fallback_when_enabled() {
        let dynamic = EdgeConfig::from_toml_str(
            r#"
[service]
name = "x"
[origins]
a = { url = "https://api.example.com", backend = "b1" }
[fastly]
dynamic_backends = true
"#,
        )
        .unwrap();
        assert_eq!(
            dynamic
                .resolve(&uri("http://other.example.com:9999/"))
                .unwrap(),
            Resolution::Dynamic {
                host: "other.example.com".into(),
                port: 9999,
                https: false
            }
        );
        assert_eq!(
            dynamic.resolve(&uri("https://other.example.com/")).unwrap(),
            Resolution::Dynamic {
                host: "other.example.com".into(),
                port: 443,
                https: true
            }
        );
        // Declared hosts still take the static path.
        assert_eq!(
            dynamic.resolve(&uri("https://api.example.com/")).unwrap(),
            Resolution::Static("b1".into())
        );
    }

    #[test]
    fn resolve_relative_uri_is_bad_request() {
        assert!(matches!(
            cfg().resolve(&uri("/relative")).unwrap_err(),
            crate::FetchError::BadRequest(_)
        ));
    }
}
