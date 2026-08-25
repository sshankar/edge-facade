# Edge SDK — spec wiki

The specification and milestone plans live here as interlinked pages
(split from the former `SPEC.md` / `SPEC-PORTABILITY-PRIMITIVES.md`
monoliths, 2026-08-25, following the LLM-maintained-wiki pattern). Read
this index first, then drill into the relevant page. Maintenance
conventions live in [`AGENTS.md`](AGENTS.md).

**Conventions in one line:** one topic per page · relative markdown links
(`[D02](decisions/d02.md)`, no wikilinks) · original §/D numbers preserved as
headings so code comments like "SPEC §8.3" or "SPEC D21" keep resolving ·
one change = one commit, the message is the log entry.

## Overview

| Page | Summary |
|---|---|
| overview *(planned)* | Purpose, non-goals, design principles (§1/§2/§4) |
| capability-matrix *(planned)* | Ground truth per platform, verified against SDK source (§3) |
| questions *(planned)* | Open questions & risks (§14) |

## API surface

| Page | Summary |
|---|---|
| [api/http-types](api/http-types.md) | `Body`/`ChunkStream`, request/response types, helpers (§6.1) |
| api/handler *(planned)* | Handler contract (`#[edge_core::main]`, §6.2) |
| api/context *(planned)* | `Context` capabilities, `Platform` SPI (§6.3) |
| api/errors *(planned)* | Normalized error model (§6.4) |
| api/kv *(planned)* | KV store API (§6.5) |
| api/router *(planned)* | Router (§6.6) |

## Fetch, adapters, config, conformance

| Page | Summary |
|---|---|
| fetch *(planned)* | URL-first fetch & backend resolution (§7, D1/D4/D5/D10) |
| adapters/cloudflare *(planned)* | workers-rs adapter contract (§8.1) |
| adapters/fastly *(planned)* | Fastly Compute adapter contract (§8.2) |
| adapters/execution *(planned)* | Fastly executor contract (§8.3, D3) |
| config *(planned)* | edge.toml config & edge-cli codegen (§9, D6/D9/D11) |
| conformance *(planned)* | T1–T12 suite + portability P1–P15 links (§11) |

## Milestones

Roadmap *(planned)* — one page per milestone
(`milestones/m0.md` … `milestones/m14.md`) with deliverable, exit criteria,
status, and links to the implementation plan (`PLAN-M*.md` at the repo root
until promoted) and the decisions it lands.

## Decisions

[Index](decisions/README.md) — one page per decision (`decisions/dNN.md`),
each with status, decision, alternatives, rationale, consequences,
revisit-if. D02 and D21 are the pilot pages.

## Portability primitives (draft v0.2)

Formerly `SPEC-PORTABILITY-PRIMITIVES.md`; the split is pending. Planned
pages under `portability/`: deferred-work, time-deadlines, client-metadata,
log-fields, dictionaries, scheduled, rate-limiting, config, conformance
(P1–P15), delivery-order.

## Migration status

The split is in progress; pages marked *(planned)* above are still inside
`SPEC.md` at the repo root and have not been moved yet. The split proceeds
in ordered commits — see `AGENTS.md` §"Split roadmap". Until it completes,
treat `SPEC.md` as the source of truth for any page not yet created.
