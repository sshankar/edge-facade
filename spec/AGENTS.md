# Spec wiki — maintenance schema (AGENTS.md)

This file is the schema for the Edge SDK spec wiki in this directory,
following the LLM-maintained-wiki pattern: the wiki is a persistent,
interlinked set of markdown pages the LLM reads, writes, and keeps current.
You (the agent) own the maintenance; the human owns the decisions and the
review. **Read this file before editing anything under `spec/`.**

## 1. Structure

```
spec/
├── README.md            # index/catalog — read first; updated on every change
├── AGENTS.md            # this schema
├── overview.md, capability-matrix.md, fetch.md, config.md,
│   conformance.md, questions.md      # topic pages
├── api/                 # §6 common API surface (one page per subsection)
├── adapters/            # §8 adapter contracts + §10 build/deploy
├── milestones/          # §12 — one page per milestone (m0.md … m14.md)
├── decisions/           # §13 — one page per decision (dNN.md)
└── portability/         # SPEC-PORTABILITY-PRIMITIVES split (draft v0.2)
```

Related material that stays outside `spec/`:
- `PLAN-M*.md` at the repo root — per-milestone implementation plans
  (status headers, verification tables). Promoted into
  `milestones/m*.md` by the compaction pass; until then, milestone pages
  link to them.
- `README.md` at the repo root — repository index (code layout, quick
  start, CI); links into `spec/`.

## 2. Page conventions

- **One topic per page.** If a page is getting long or covers two subjects,
  split it and update the index.
- **Links are relative markdown paths** — `[D02](decisions/d02.md)` — never
  `[[wikilinks]]`. The tree must render on GitHub and in any markdown
  viewer.
- **Preserve original numbering as headings.** The monolith's section and
  decision numbers are referenced from 127 code comments ("SPEC §8.3",
  "SPEC D21", "§6.4"). When you create a page from a section, keep the
  original heading text (`## 8.3 Execution of async on Fastly ...`, `### D21.
  Streaming response bodies (M6), no select-scheduler needed`) so those
  references stay resolvable through the index's section map.
- **Page header block** (every page):
  ```
  # <Title>
  (one-line summary of what this page pins down)
  ```
- **Status lines on mutable pages** (milestones, plans, drafts): a
  `**Status:** ...` line at the top, e.g. `**Status:** ✅ done (2026-08-24)`
  or `**Status:** draft v0.2`.
- **Filenames:** kebab-case; decision pages `dNN.md` (zero-padded),
  milestone pages `mN.md`, section pages named by topic
  (`http-types.md`, not `6-1.md`).
- **Cross-references replace prose citations.** "see SPEC D21" becomes a
  link `[D21](decisions/d21.md)`; "see §8.3" links
  `adapters/execution.md` (once that page exists). When you move/rename a
  page, fix inbound links in the same commit.

## 3. Workflows

### Landing a milestone (M7+)

When a milestone lands (e.g. M10/M11):
1. Update `milestones/mN.md`: status → ✅ done + date; record the evidence
   (which T/P tests pass, on which targets).
2. If the work records new design decisions, create `decisions/dNN.md`
   (format below) and add backlinks from the affected pages.
3. Update affected topic pages (new API, changed contracts) and the index
   `README.md`.
4. One commit: conventional title `feat(mN): ...` — the message is the log
   entry (what landed, what it touched).

### Recording a decision

New file `decisions/dNN.md` with this exact structure:
`**Status:**` (accepted/superseded + date) · `**Decision:**` · `**Alternatives:**` ·
`**Rationale:**` (with SDK-source evidence where relevant) · `**Consequences:**`
(including the constraints it imposes) · `**Revisit if:**` (the trigger that
reopens it). Link it from every page that depends on it and add it to
`decisions/README.md`.

### Querying

Read `spec/README.md` first to locate pages, then read the 1–3 relevant
pages and answer with page-path citations. File good answers back as new
pages (a comparison, a synthesis) — explorations compound in the wiki.

### Linting (periodic)

Health-check the wiki: contradictions between pages, stale statuses
(superseded decisions still marked accepted), orphan pages with no inbound
links, concepts mentioned but lacking a page, missing cross-references,
broken links. Suggest fixes; do not apply destructive changes without
saying so.

### Compacting (periodic)

Wikis accrue debt: near-duplicate pages, stale detail, root index getting
hard to scan. Propose compaction when you notice it — merge pages that
turned out to be one subject, delete what stopped mattering, promote
clusters into subdirectories with their own `README.md`. Git makes this
safe; say what you're about to delete before deleting.

## 4. Commit rules

- One change = one commit; the commit message is the wiki's log entry
  (`git log -- spec/` is the timeline). No `log.md`.
- Conventional prefixes (`feat`, `test`, `docs`, `chore`) matching the
  repo's history.
- Never commit generated/broken intermediate states: each commit should
  leave the tree browsable (no dangling relative links).

## 5. Split roadmap

The monoliths are being split into this wiki in ordered commits (each one
landing independently):

1. ✅ **Scaffold** — this schema, the index, the pilot page
   (`api/http-types.md` §6.1) and the two decisions it links
   (`decisions/d02.md`, `decisions/d21.md`).
2. ⏳ Split `SPEC.md` §1–§11 into topic pages (`overview`,
   `capability-matrix`, `api/*`, `fetch`, `adapters/*`, `config`,
   `conformance`, `questions`), preserving §-numbered headings.
3. ⏳ Split §12 milestones + §13 decisions into `milestones/m*.md` and
   `decisions/dNN.md` (all 23 decisions), with per-area decision index.
4. ⏳ Split `SPEC-PORTABILITY-PRIMITIVES.md` into `portability/*`.
5. ⏳ Promote `PLAN-M*.md` into `milestones/` (compaction), update the root
   `README.md`, and mark the migration complete.

Until a page is created, `SPEC.md` at the repo root remains the source of
truth for its content; the index marks such pages *(planned)*. Remove the
*(planned)* marker as each page lands.
