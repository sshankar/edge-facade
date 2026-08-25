# Decisions (§13)

One page per design decision (`dNN.md`), each with status, decision,
alternatives, rationale, consequences, and the trigger that reopens it.
Original decision numbers are preserved in the page headings so code
references ("SPEC D21") keep resolving. See the [wiki index](../README.md)
for the full catalog.

| # | Decision | Area |
|---|---|---|
| [D02](d02.md) | Fully buffered `Bytes` bodies in v1 (no streaming) | HTTP types |
| [D21](d21.md) | Streaming response bodies (M6), no select-scheduler needed | HTTP types |
| D01, D03–D20, D22–D23 *(planned)* | — | — |

Remaining decisions are still in `SPEC.md` §13 at the repo root until the
split reaches this area; see [AGENTS.md](../AGENTS.md) §"Split roadmap".
