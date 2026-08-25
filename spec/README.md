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
| [overview](overview.md) | Purpose, non-goals, design principles, workspace layout (§1/§2/§4/§5) |
| [capability-matrix](capability-matrix.md) | Ground truth per platform, verified against SDK source (§3) |
| [questions](questions.md) | Open questions & risks (§14) |

## API surface

| Page | Summary |
|---|---|
| [api/http-types](api/http-types.md) | `Body`/`ChunkStream`, request/response types, helpers (§6.1) |
| [api/handler](api/handler.md) | Handler contract (`#[edge_core::main]`, §6.2) |
| [api/context](api/context.md) | `Context` capabilities, `Platform` SPI (§6.3) |
| [api/errors](api/errors.md) | Normalized error model (§6.4) |
| [api/kv](api/kv.md) | KV store API (§6.5) |
| [api/router](api/router.md) | Router (§6.6) |

## Fetch, adapters, config, conformance

| Page | Summary |
|---|---|
| [fetch](fetch.md) | URL-first fetch & backend resolution (§7, D1/D4/D5/D10) |
| [adapters/README](adapters/README.md) | Adapter contracts cluster index (§8) |
| [adapters/cloudflare](adapters/cloudflare.md) | workers-rs adapter contract (§8.1) |
| [adapters/fastly](adapters/fastly.md) | Fastly Compute adapter contract (§8.2) |
| [adapters/execution](adapters/execution.md) | Fastly executor contract (§8.3, D3) |
| [config](config.md) | edge.toml config, edge-cli codegen, build & deploy (§9/§10, D6/D9/D11) |
| [conformance](conformance.md) | T1–T12 + P7–P11 conformance tables (§11) |

## Milestones

[Roadmap](milestones/README.md) — one page per milestone
(`milestones/m0.md` … `milestones/m14.md`) with deliverable, exit criteria,
status, and links to the implementation plan (`PLAN-M*.md` at the repo root
until promoted) and the decisions it lands.

## Decisions

[Index](decisions/README.md) — one page per decision (`decisions/dNN.md`),
each with status, decision, alternatives, rationale, consequences,
revisit-if. All 23 decisions are split.

## Portability primitives (draft v0.2)

[Index](portability/README.md) — the draft v0.2 extension split into pages
(goals, deferred-work, time-deadlines, client-metadata, log-fields,
dictionaries, scheduled, rate-limiting, config, conformance P1–P15,
delivery-order). M10/M11 shipped; the rest gated on M7+.

## Migration status

`SPEC.md` §1–§14 and `SPEC-PORTABILITY-PRIMITIVES.md` are fully split
(2026-08-25). Remaining: the plan promotion (`PLAN-M*.md` → milestone
pages) — see `AGENTS.md` §"Split roadmap" for progress.
