//! Validate a deployed `fastly.toml` `[setup]` against `edge.toml` (SPEC §9
//! `edge-cli check`, decision D6).
//!
//! Checks:
//!
//! 1. Every origin's backend name exists in `[setup.backends]`.
//! 2. `override_host` matches the origin's URL host (D5.1 parity: the origin
//!    must receive `Host: <URL host>`).
//! 3. `use_ssl` matches the origin's URL scheme.
//! 4. Store bindings declared in `[stores]` exist in `[setup]` with the
//!    same name.

use std::collections::HashMap;

use edge_core::config::EdgeConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FastlyToml {
    #[serde(default)]
    setup: Setup,
}

#[derive(Debug, Default, Deserialize)]
struct Setup {
    #[serde(default)]
    backends: HashMap<String, Backend>,
    #[serde(default)]
    kv_store: Option<Named>,
    #[serde(default)]
    config_store: Option<Named>,
    #[serde(default)]
    secret_store: Option<Named>,
}

#[derive(Debug, Deserialize)]
struct Backend {
    target: Option<String>,
    override_host: Option<String>,
    use_ssl: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Named {
    name: String,
}

/// Return human-readable problems found; empty means valid.
pub fn validate(config: &EdgeConfig, fastly_toml_str: &str) -> Vec<String> {
    let parsed: FastlyToml = match toml::from_str(fastly_toml_str) {
        Ok(p) => p,
        Err(e) => return vec![format!("fastly.toml is not valid TOML: {e}")],
    };
    let mut problems = Vec::new();

    // Origins vs [setup.backends].
    let mut origins: Vec<_> = config.origins().collect();
    origins.sort_by_key(|(a, _)| *a);
    for (alias, origin) in origins {
        let host = origin.url.host_str().unwrap_or_default();
        let https = origin.url.scheme() == "https";
        match parsed.setup.backends.get(&origin.backend) {
            None => problems.push(format!(
                "origin `{alias}`: backend `{}` missing from [setup.backends]",
                origin.backend
            )),
            Some(backend) => {
                if let Some(target) = &backend.target {
                    if target != host {
                        problems.push(format!(
                            "origin `{alias}`: backend `{}` target `{target}` != URL host `{host}`",
                            origin.backend
                        ));
                    }
                } else {
                    problems.push(format!(
                        "origin `{alias}`: backend `{}` has no target",
                        origin.backend
                    ));
                }
                if let Some(override_host) = &backend.override_host {
                    if override_host != host {
                        problems.push(format!(
                            "origin `{alias}`: backend `{}` override_host `{override_host}` != URL host `{host}`",
                            origin.backend
                        ));
                    }
                } else {
                    problems.push(format!(
                        "origin `{alias}`: backend `{}` has no override_host (Host-parity, D5.1)",
                        origin.backend
                    ));
                }
                if let Some(use_ssl) = backend.use_ssl {
                    if use_ssl != https {
                        problems.push(format!(
                            "origin `{alias}`: backend `{}` use_ssl = {use_ssl} != URL scheme (https={https})",
                            origin.backend
                        ));
                    }
                } else {
                    problems.push(format!(
                        "origin `{alias}`: backend `{}` has no use_ssl flag",
                        origin.backend
                    ));
                }
            }
        }
    }

    // Store bindings vs [setup] stores.
    let stores = config.stores();
    for (binding, declared, actual) in [
        ("kv", stores.kv.as_deref(), parsed.setup.kv_store.as_ref()),
        (
            "config",
            stores.config.as_deref(),
            parsed.setup.config_store.as_ref(),
        ),
        (
            "secrets",
            stores.secrets.as_deref(),
            parsed.setup.secret_store.as_ref(),
        ),
    ] {
        match (declared, actual) {
            (None, _) => {}
            (Some(name), None) => problems.push(format!(
                "[stores] {binding} = \"{name}\" but no {binding}_store in [setup]"
            )),
            (Some(name), Some(store)) if store.name != name => problems.push(format!(
                "[stores] {binding} = \"{name}\" but [setup] {binding}_store is \"{}\"",
                store.name
            )),
            _ => {}
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(toml_str: &str) -> EdgeConfig {
        EdgeConfig::from_toml_str(toml_str).unwrap()
    }

    const EDGE: &str = r#"
[service]
name = "svc"

[origins]
api = { url = "https://api.example.com", backend = "api_backend" }

[stores]
kv = "edge_kv"

[fastly]
dynamic_backends = false
"#;

    const GOOD_FASTLY: &str = r#"
name = "svc"
[setup]
backends = { api_backend = { target = "api.example.com", override_host = "api.example.com", use_ssl = true } }
kv_store = { name = "edge_kv" }
"#;

    #[test]
    fn matching_configs_pass() {
        assert!(validate(&config(EDGE), GOOD_FASTLY).is_empty());
    }

    #[test]
    fn missing_backend_is_flagged() {
        let fastly = r#"
[setup]
backends = {}
kv_store = { name = "edge_kv" }
"#;
        let problems = validate(&config(EDGE), fastly);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("missing from [setup.backends]"));
    }

    #[test]
    fn override_host_mismatch_is_flagged() {
        let fastly = r#"
[setup]
backends = { api_backend = { target = "other.example.com", override_host = "other.example.com", use_ssl = true } }
"#;
        let problems = validate(&config(EDGE), fastly);
        assert!(problems
            .iter()
            .any(|p| p.contains("override_host") && p.contains("other.example.com")));
    }

    #[test]
    fn ssl_mismatch_is_flagged() {
        let fastly = r#"
[setup]
backends = { api_backend = { target = "api.example.com", override_host = "api.example.com", use_ssl = false } }
"#;
        let problems = validate(&config(EDGE), fastly);
        assert!(problems.iter().any(|p| p.contains("use_ssl")));
    }

    #[test]
    fn missing_store_is_flagged() {
        let fastly = r#"
[setup]
backends = { api_backend = { target = "api.example.com", override_host = "api.example.com", use_ssl = true } }
"#;
        let problems = validate(&config(EDGE), fastly);
        assert!(problems
            .iter()
            .any(|p| p.contains("no kv_store in [setup]")));
    }
}
