---
name: ilearned-travel-agent
description: Use ilearned MCP tools as persistent memory for a travel-agent AI harness. Search, add, refine, promote, demote, or delete structured experiences under hierarchical travel topics so the agent continuously improves booking, itinerary, and support performance over time.
---

# ilearned Travel Agent Memory

Lightweight local-first memory of learned experiences for a travel-agent AI. Experiences are structured rules (when / if / do / check) stored under hierarchical topics. Use the ilearned MCP tools (stdio or HTTP) as the sole persistence layer for lessons learned during booking, itinerary planning, customer support, and post-trip handling.

## When to Activate

- Any travel-agent task that benefits from past lessons (hotel booking edge cases, flight disruptions, visa rules, family travel constraints, loyalty programs, cancellation policies, etc.).
- After a successful or failed action that produced a reusable insight.
- Before proposing a plan or action — search first for matching experiences.
- When user feedback (positive or negative) arrives about a previous recommendation or booking step.

## Prerequisites

- ilearned binary available and MCP surface enabled (`ilearned mcp` for stdio harness, or `ilearned serve` for HTTP `/mcp`).
- Database path configured (project-local `./.ilearned/ilearned.db` preferred; add to `.gitignore` unless shared).
- MCP tools exposed to the agent: `search`, `add`, `update`, `delete`, `promote`, `demote`, `clear`, `topics_list`, `topics_search`.

## Hierarchical Topics

Use slash-separated lowercase segments matching `[a-z0-9_-]`. Prefer depth 2–4. Examples of the canonical travel taxonomy:

| Level-1 | Level-2 examples | Level-3+ examples |
|---------|------------------|-------------------|
| `travel` | `flight`, `hotel`, `car`, `rail`, `cruise`, `package` | `travel/flight/booking`, `travel/hotel/checkout`, `travel/hotel/cancellation` |
| `travel` | `visa`, `insurance`, `loyalty`, `payment` | `travel/visa/schengen`, `travel/loyalty/airline` |
| `travel` | `itinerary`, `support`, `disruption` | `travel/itinerary/family`, `travel/disruption/weather`, `travel/support/refund` |
| `travel` | `destination` | `travel/destination/japan`, `travel/destination/europe/city-break` |

- Exact match: `travel/hotel/booking`
- Multi-level wildcard (search/export only): `travel/#`, `#/cancellation`, `travel/hotel/#`
- Bare segment matches exact topic only.
- Never store `#` inside a topic name.
- Prefer existing topics; create new leaves only when no close match exists. Use `topics_search` / `topics_list` before inventing.

## Experience Structure

Every experience must fill all four fields with concrete, actionable text:

- **when** — Scenario + context + constraints (who, what trip type, constraints, constraints that make this rule relevant).
- **if** — Observable trigger(s) that fire the experience (error message, user statement, system state, external signal).
- **do** — Precise action(s) or procedure the agent should execute (steps, API calls, phrasing, checks to perform).
- **check** — Success / failure signals that confirm the experience applied correctly and the outcome was good.

Example:

```text
topic: travel/hotel/booking
when : Family trip with infant under 12 months; hotel does not list baby cot on public site
if   : User requests crib or baby cot; or child age < 1y detected in passenger data
do   : Contact hotel via booking channel or direct email; request complimentary baby cot and 
       written confirmation; note request in booking notes; offer portable travel cot as paid 
       alternative if hotel declines
check: Hotel confirmation email or portal note mentions baby cot; or alternative cot reserved 
       and cost disclosed to user before final payment
```


## Core Workflow

1. **Before acting** — Call `search` (prefer `topic` with `#` wildcard + `text` or `semantic`). Review top results. Apply high `good_count` experiences; treat high `bad_count` as warnings.
2. **After acting** — If the outcome produced a new reusable lesson, call `add`. If an existing experience was close but incomplete, call `update`.
3. **Feedback loop** — On clear user or system success of an applied experience → `promote`. On clear failure or user complaint that the experience caused harm → `demote`. Prefer promote/demote over immediate delete.
4. **Maintenance** — Periodically `topics_list` / `topics_search` to keep taxonomy tidy. Use `clear` only with explicit confirmation and only for obsolete whole topics.

## Rules for Lifecycle Operations

### Add
- Use when no existing experience covers the scenario at useful specificity.
- Always supply all four fields; keep each field focused and under ~300 chars when possible.
- Choose the most specific existing topic leaf; create a new leaf only if necessary.
- After add, note the returned `(topic, id)` for later promote/update.

### Refine (update)
- Use when an experience is directionally correct but missing a constraint, step, or check.
- Supply only the fields that change; at least one non-blank field required.
- Prefer refine over creating a near-duplicate.
- Refining refreshes `updated_at` and restores inactive/forgotten records to active.

### Promote
- Call after an experience was applied and produced a clearly positive outcome (successful booking, user praise, avoided problem).
- Increases `good_count`; refreshes lifecycle.
- Prefer promote over re-adding the same lesson.

### Demote
- Call after an experience was applied and produced a clearly negative outcome (failed booking, user complaint, policy violation, extra cost).
- Increases `bad_count`; refreshes lifecycle.
- If `bad_count` grows large relative to `good_count`, consider `update` to fix or `delete` if the experience is fundamentally wrong.

### Delete
- Use only for experiences that are factually incorrect, superseded by a better one, or violate policy/safety.
- Prefer `demote` + later review over immediate delete.
- Deleting a never-existing id is not-found; deleting an already-deleted id is idempotent success.
- Always confirm the `(topic, id)` pair before calling.

### Clear
- Destructive; requires `confirm: true` and exactly one of `topic` or `all=true`.
- Use only for wholesale topic retirement (e.g., obsolete destination rules) or full reset during development.
- Never clear production memory without explicit human approval.

## Search Guidance

- Prefer `topic: "travel/#"` (or narrower) + `text` for keyword precision.
- Add `semantic` when the wording of past experiences may differ from current query.
- Default limit 10–20; raise only when exploring.
- Use `deep: true` only when intentionally retrieving inactive experiences.
- Rank results by relevance then by `good_count - bad_count` and recency when deciding which to apply.

## Continuous Improvement Principles

- Search before every non-trivial action.
- Write experiences that are specific enough to be actionable and general enough to transfer.
- Prefer high-signal, low-noise entries; one clear lesson per experience.
- Keep topics hierarchical and consistent; refactor via update + delete rather than proliferating near-duplicates.
- Treat promote/demote as the primary feedback signal; the counts drive future ranking and trust.
- Never store secrets, full PII, payment details, or raw booking references inside experiences.
- When an experience conflicts with live policy or real-time data, the live source wins; record the conflict as a new experience if useful.

## MCP Tool Cheat-Sheet

| Tool | Required args | Notes |
|------|---------------|-------|
| `search` | (optional topic/text/semantic/limit/offset/deep) | Primary retrieval |
| `add` | topic, when, if, do, check | Returns `{added: {topic, id}}` |
| `update` | topic, id + ≥1 field | Returns `{modified: {topic, id}}` |
| `delete` | topic, id | Idempotent on already-deleted |
| `promote` | topic, id | good_count += 1 |
| `demote` | topic, id | bad_count += 1 |
| `clear` | confirm=true + (topic \| all=true) | Destructive |
| `topics_list` | (optional level/limit/offset/deep) | Discover taxonomy |
| `topics_search` | query (+ optional level/…) | Substring or `#` pattern |

## Error Handling

- `invalid_params` → fix arguments and retry.
- `resource_not_found` → experience already gone or wrong id; re-search.
- Embedding unavailable → fall back to text-only search; do not block the user.
- Never invent experiences; only persist what was actually observed or validated.

## Example Session Pattern

```text
1. User  : "Book hotel in Paris for family with 8-month-old"
2. Agent : search topic="travel/hotel/#" text="infant OR baby cot OR crib"
3. Apply matching high-good_count experiences
4. Complete booking
5. If new edge case surfaced → add under travel/hotel/booking
6. If user later confirms "cot was ready" → promote the used experience
```

Follow these rules strictly so the travel-agent memory compounds in quality and the agent’s performance improves with every interaction.