# Martin Tile Server

## 1. Identity & Project Scope

**Project:** Martin Tile Server
**Primary Language:** Rust (Backend)
**Frontend:** TypeScript / React (`martin/martin-ui`)

**Major Components:**

* `martin` / `martin-core`: Rust backend & business logic
* `mbtiles`: Utility CLI
* `martin/martin-ui`: React-based Web UI (isolated frontend)

---

## 2. Non‑Negotiable Directives (HARD RULES)

### Directive 000 — Quality bar for LLM‑assisted changes

All LLM‑assisted contributions **must aim for a higher standard of excellence** than human‑only changes.

* Treat LLMs as a **quality multiplier, not a speed multiplier**.
* Do not submit first‑draft code unless explicitly requesting design feedback.
* Invest time saved into:

  * Additional tests (edge cases, regression, stress)
  * Clearer structure and naming
  * Removing TODOs and technical debt
* Code produced with LLM assistance remains **the human author’s responsibility**.

LLM‑generated changes that show lack of care (missed obvious cases, shallow error handling, poor UX) may be declined outright.

---

## 2. Non‑Negotiable Directives (HARD RULES)

### Directive 001 — Build Integrity

* **NEVER cancel** `cargo build`, `cargo test`, `just check`, or `just ci-test` once started.
* Always allocate sufficient timeout **before** starting long-running commands.

### Directive 002 — Scope Isolation

You **MUST** correctly determine scope **before** running commands.

#### Frontend-only scope (UI / React)

* Allowed area: `martin/martin-ui/**`
* **PROHIBITED:**

  * `cargo build`
  * `cargo run`
  * `just start`
  * Any Rust compilation

#### Backend or Full-stack scope

* Rust, DB, API, CLI, config, catalog, tile logic
* Full bootstrap and validation **required**

---

## 3. Step 1: Determine Scope (MANDATORY)

### PATH A — Frontend Only

**Trigger:** CSS, JS, React components, frontend tests, UI behavior

Steps:

1. `cd martin/martin-ui`
2. `npm clean-install --no-fund`
3. `just test-frontend`
4. (Optional) `npm run lint`
5. (Optional) `just type-check`

If a frontend task attempts Rust compilation → **STOP immediately**.

---

### PATH B — Backend / Full System

**Trigger:** Rust logic, APIs, DB, tiles, CLI, config

Bootstrap order (exact):

```bash
cargo install just --locked
just validate-tools
just start
cargo build --workspace
```

---

## 4. Execution Heuristics & Timeouts

### Frontend Operations

| Command              | Timeout | Notes               |
| -------------------- | ------- | ------------------- |
| `npm install`        | 5m      | Only in `martin-ui` |
| `just test-frontend` | 2m      | UI tests only       |
| `npm start`          | n/a     | Dev server          |

### Backend Operations

| Command                   | Timeout |
| ------------------------- | ------- |
| `cargo build --workspace` | 20m     |
| `just check`              | 20m     |
| `cargo clippy`            | 5m      |
| `cargo test`              | 5m      |
| `just test`               | 30m     |
| `just ci-test`            | 45m     |

---

## 5. Validation Protocols

### Frontend Validation (No Rust)

* `just test-frontend` → **must pass**
* `just type-check` → no TS errors
* Rust logs are irrelevant here

### Backend Validation (Required)

1. `cargo build --workspace`
2. `just start`
3. `cargo run --bin martin -- ...`
4. Health check:

   * `/health`
   * `/catalog`

---

## 6. Manual Validation Scenarios (Backend)

### Scenario 1 — MBTiles

```bash
cargo run --bin martin -- --webui enable-for-all tests/fixtures/mbtiles
curl http://localhost:3000/health
curl http://localhost:3000/catalog
```

### Scenario 2 — PostgreSQL

```bash
just start
PGHOST=localhost PGPORT=5411 PGUSER=postgres PGPASSWORD=postgres PGDATABASE=db tests/fixtures/initdb.sh
export DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'
cargo run --bin martin -- --webui enable-for-all
```

### Scenario 3 — CLI Tools

```bash
cargo run --bin martin-cp -- --help
cargo run --bin mbtiles -- --help
```

---

## 7. Directory Rules

* `martin/martin-ui/` → **Frontend-only zone**
* `martin/` → Rust CLI entry
* `martin-core/` → Core logic
* `tests/` → Fixtures & integration tests

Rule: **If you enter `martin-ui`, stay there.**

---

## 8. Engineering principles (applies to all scopes)

### Correctness over convenience

* Model the **full error space**—no shortcuts or silent fallbacks.
* Handle edge cases explicitly, including race conditions, timing issues, and platform differences.
* Prefer **compile‑time guarantees** (types, invariants) over runtime checks where possible.

### User experience as a primary driver

* Errors must be actionable and specific.
* Prefer structured, contextual error messages over generic ones.
* Write user‑facing messages in clear, present tense.

### Pragmatic incrementalism

* Avoid over‑generic abstractions.
* Prefer specific, composable logic.
* Evolve designs incrementally and document trade‑offs when they matter.

### Production‑grade Rust engineering

* Use the type system aggressively (newtypes, builders, state encoding).
* Avoid shared mutable state; prefer message passing or ownership transfer.
* Be mindful of performance characteristics (allocation, cloning, hot paths).
* Tests are part of the feature: missing tests means the change is incomplete.

---

## 9. Troubleshooting Logic

1. Frontend task + Rust build → **Error**
2. Frontend dependency failure → remove `node_modules`, reinstall
3. Integration test DB failures → `just restart`
4. CI failures → rerun `just ci-test` locally

---

## 9. Performance & Reliability Notes

* Release builds are significantly faster
* Integration tests may fail in CI due to S3/network limits
* Martin is optimized for high‑throughput tile serving

---

## 10. Final Authority Rule

If two instructions ever conflict:

1. **This document wins**
2. Hard rules override heuristics
3. Scope isolation overrides convenience