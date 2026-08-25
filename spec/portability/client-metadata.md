# Client metadata (§5)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §5 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§5") keep resolving.

The adapter MUST snapshot request-entry metadata into owned, platform-free data:

```rust
pub struct ClientMetadata {
    pub provider: EdgeProvider,
    pub client_ip: Option<IpAddr>,
    pub pop: Option<String>,
    pub original_header_names: Option<Vec<String>>,
    pub geo: GeoMetadata,
    pub network: NetworkMetadata,
    pub tls: TlsMetadata,
}

pub enum EdgeProvider {
    Cloudflare,
    Fastly,
    Mock,
}

pub struct GeoMetadata {
    pub continent: Option<String>,
    pub country_code: Option<String>,
    pub region_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub metro_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub struct NetworkMetadata {
    pub asn: Option<u32>,
    pub as_organization: Option<String>,
    pub proxy_type: Option<String>,
    pub proxy_description: Option<String>,
}

pub struct TlsMetadata {
    pub protocol: Option<String>,
    pub cipher: Option<String>,
    pub ja3: Option<String>,
    pub ja4: Option<String>,
    pub ciphers_sha1: Option<String>,
    pub extensions_sha1: Option<String>,
}

impl Context {
    pub fn client(&self) -> &ClientMetadata;
}
```

Rules:

- Missing data is `None`; the facade MUST NOT substitute presentation values such as `"unknown"`.
- Values describe the downstream client connection, not an origin subrequest.
- Unicode city and organization values are preserved.
- Original header names preserve runtime-provided order and spelling. If unavailable, the value is `None`; reconstructing it from normalized headers is forbidden.
- Provider-specific metadata MUST NOT be smuggled through request headers.
- Metadata is immutable and safe to clone into deferred tasks.

Minimum mapping:

| Common field | Cloudflare source | Fastly source |
|---|---|---|
| client IP | connecting-client address | downstream client IP API |
| POP | request colo | POP/datacenter API when available |
| geo | request continent/country/region/city/postal/metro/lat/long | client-IP geolocation |
| ASN/organization | request ASN metadata | network metadata when available |
| original header names | original-header metadata | downstream original-header API, otherwise `None` |
| JA3/JA4 | TLS/bot metadata | downstream TLS fingerprint APIs when available |
| cipher/extension hashes | request TLS metadata | downstream TLS metadata when available |

Applications derive any provider-neutral request headers from this structure.

**Implemented sources (decision D23, M10):** Cloudflare — client IP from the
`cf-connecting-ip` request header, POP/geo/network/TLS from `request.cf` (under
workerd, injected via the socket's `cfBlobHeader` — the conformance harness
sends a `cf-blob` header, which workerd parses into `request.cf` and strips);
original header names and proxy classification are not exposed by the public
API and are `None` (P8). Fastly — downstream client IP API, `compute_runtime::pop()`,
the downstream original-header API, `geo::geo_lookup` on the client IP, and JA3
(hex MD5)/JA4 from downstream TLS metadata; TLS protocol/cipher and
cipher/extension hashes are not exposed by fastly 0.13 and are `None`. Sentinel
values (`"--"` POP, empty strings, `0` ASN, `0.0` coordinates, `??` continents)
are mapped to `None` — never substituted.
