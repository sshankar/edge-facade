//! Client metadata (SPEC-PORTABILITY-PRIMITIVES §5, milestone M10).
//!
//! [`crate::Context::client`] returns an owned, platform-free snapshot of the
//! downstream client connection, captured at request entry by the adapter.
//!
//! Rules (from the spec):
//!
//! - Missing data is `None`; the facade MUST NOT substitute presentation
//!   values such as `"unknown"`.
//! - Values describe the downstream client connection, not an origin
//!   subrequest.
//! - Unicode city and organization values are preserved.
//! - Original header names preserve runtime-provided order and spelling. If
//!   unavailable, the value is `None`; reconstructing it from normalized
//!   headers is forbidden (P8).
//! - Provider-specific metadata is never smuggled through request headers.
//! - The snapshot is immutable and safe to clone into deferred tasks.

use std::net::IpAddr;

use serde::Serialize;

/// The platform that processed the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Default)]
pub enum EdgeProvider {
    /// Cloudflare Workers (`edge-cloudflare`).
    Cloudflare,
    /// Fastly Compute (`edge-fastly`).
    Fastly,
    /// The native mock (`edge_core::testing`).
    #[default]
    Mock,
}

/// Geographic metadata about the downstream client (SPEC-PORTABILITY-PRIMITIVES §5).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct GeoMetadata {
    /// Two-letter continent code, e.g. `NA`.
    pub continent: Option<String>,
    /// Two-letter country code, e.g. `US`.
    pub country_code: Option<String>,
    /// Region/state code or name, e.g. `TX` (CF) or `Texas` (Fastly).
    pub region_code: Option<String>,
    /// City name, e.g. `Austin`.
    pub city: Option<String>,
    /// Postal code.
    pub postal_code: Option<String>,
    /// DMA metro code.
    pub metro_code: Option<String>,
    /// Latitude.
    pub latitude: Option<f64>,
    /// Longitude.
    pub longitude: Option<f64>,
}

/// Network metadata about the downstream client (SPEC-PORTABILITY-PRIMITIVES §5).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NetworkMetadata {
    /// Autonomous System Number.
    pub asn: Option<u32>,
    /// AS organization name.
    pub as_organization: Option<String>,
    /// Proxy/VPN classification, e.g. `Hosting`, `Vpn` (Fastly).
    pub proxy_type: Option<String>,
    /// Proxy/VPN description, e.g. `Cloud` (Fastly).
    pub proxy_description: Option<String>,
}

/// TLS metadata about the downstream client connection (SPEC-PORTABILITY-PRIMITIVES §5).
///
/// Source availability differs by platform (documented in SPEC
/// PORTABILITY-PRIMITIVES §5, "Minimum mapping"): Cloudflare exposes
/// protocol + cipher via `request.cf`; the Fastly 0.13 SDK exposes JA3/JA4
/// fingerprints; both platforms' unavailable fields are `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TlsMetadata {
    /// TLS protocol, e.g. `TLSv1.3`.
    pub protocol: Option<String>,
    /// TLS cipher, e.g. `AEAD-AES128-GCM-SHA256`.
    pub cipher: Option<String>,
    /// JA3 fingerprint (hex MD5), Fastly.
    pub ja3: Option<String>,
    /// JA4 fingerprint, Fastly.
    pub ja4: Option<String>,
    /// SHA1 of the client cipher list.
    pub ciphers_sha1: Option<String>,
    /// SHA1 of the client extension list.
    pub extensions_sha1: Option<String>,
}

/// The client metadata snapshot returned by [`crate::Context::client`]
/// (SPEC-PORTABILITY-PRIMITIVES §5).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClientMetadata {
    /// The platform that processed the request.
    pub provider: EdgeProvider,
    /// The downstream client IP address.
    pub client_ip: Option<IpAddr>,
    /// The POP/datacenter that received the request (e.g. `DFW`).
    pub pop: Option<String>,
    /// Original request header names (order + spelling as received), when
    /// the platform exposes them. `None` when unavailable — never
    /// reconstructed from normalized headers (P8).
    pub original_header_names: Option<Vec<String>>,
    /// Geographic metadata.
    pub geo: GeoMetadata,
    /// Network metadata.
    pub network: NetworkMetadata,
    /// TLS metadata.
    pub tls: TlsMetadata,
}

impl ClientMetadata {
    /// A fully-empty snapshot for the given provider (every field `None`).
    pub fn empty(provider: EdgeProvider) -> Self {
        ClientMetadata {
            provider,
            ..ClientMetadata::default()
        }
    }
}
