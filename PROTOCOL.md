# Rust Testing and Optimization Protocol Suite (Rust-TOPS)

**Protocol ID:** `rust-tops`  
**Version:** 1.3.0  
**Status:** Adopted specification  
**Machine schema:** [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json)  
**Example instance:** [`config/rust-tops.example.yaml`](config/rust-tops.example.yaml)  
**Audience:** Rust library and binary authors, CI owners, and coding agents.

This document is the canonical protocol. The JSON Schema is the machine contract. If prose and schema disagree, fix the schema and then the prose.

---

> **Bend laws:** 1.1.0 adds the `laws` layer (§3.1). It checks the crate's own files and gate reports against the Bend law pack `LAWS.bend`. [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) lists the candidate laws and their sources.

> **1.2.0:** states what the CRAP thresholds apply to and how the gate holds existing code (§2.1), when an exhaustive dispatch `match` takes an exemption instead of a split (§2.2), how `max_fn_cc` relates to the CRAP threshold (§5.6), how the gate judges the coverage floors (§7.5) and the CRAP baseline (§16), how exemptions are matched (§19), and the `RUST-TOPS.md` record every adopting repo keeps (§14.4). No default threshold and no part of the CRAP formula changed.

> **1.3.0:** fixes each layer's tier by its id and moves `security-advisory` to tier 0 (§3), makes `cargo tops check` hold an instance to its class floors and required layers (§4), and takes the file name `PROTOCOL.md` (the changelog names the old one). §14.4 still requires each adopting repo's root `RUST-TOPS.md`. No default threshold and no part of the CRAP formula changed.

## 0. Purpose

Produce Rust that is:

1. **Correct** under a stacked evidence model (example tests are the floor, not the ceiling).
2. **Dense** — highest useful behavior per production line, without cleverness that hides cost.
3. **Fast** — algorithmic first, then layout/allocation, then codegen. No unexplained hot-path regression.
4. **Change-safe** — complex untested code is treated as a defect; weak tests that cannot kill mutants are treated as a defect.

This suite covers every method raised in the Uncle Bob / CRAP / mutation conversation, plus the Rust-native layers that conversation did not name: property tests, coverage-guided fuzz, Miri, sanitizers, loom, Kani, compile-fail UI tests, snapshot tests, Callgrind instruction counts, advisory/deny, and density audit.

---

## 1. Axioms

**A1. Tests are allowed to be larger than production. Production is not allowed to be large because of tests.**  
Coverage, CRAP, and mutation exist to constrain *risk*, not to dictate module shape.

**A2. Coverage is reachability, not evidence.**  
A line that executed can still be wrong. Mutation and properties supply the missing assertion strength.

**A3. CRAP is a spotlight, not a design goal.**  
Forcing CRAP below a vanity threshold by splitting functions and adding tautological tests *leaves scars*. Uncle Bob measured this: Hunt-the-Wumpus runs all passed the same acceptance cases; CRAP-below-4 made every program harder to read and did not improve design. Mutation then wrote a second suite aimed at operators, not a different program.

**A4. Do not generate tests from the implementation under test.**  
An agent pointed at `fn foo` will write a passing test that freezes current behavior, including bugs. Tests are derived from a spec, invariant, type contract, or oracle — then the implementation is written or changed to satisfy them.

**A5. Light harness.**  
Models improved while heavy process cages were being built. Keep unit tests, CRAP, mutation, properties, and a few guidelines. Do not treat the agent as a software component inside a workflow engine.

**A6. Density is not golf.**  
Fewest *necessary* lines. A match on an enum that names every state is denser *and* clearer than a boolean tangle. A lookup table is denser than twelve near-duplicate functions. `unwrap()` in a library hot path is not density; it is a missing `Result`.

**A7. Performance is measured, not claimed.**  
Wall-clock Criterion is for local work. CI gates use instruction/cache counts (Gungraun / iai-callgrind) because cloud VMs are noisy.

**A8. Unsafe is a kernel with a contract.**  
Every `unsafe` block has a one-paragraph safety comment, a Miri target, and — when the state space is bounded — a Kani proof or an explicit exemption.

---

## 2. CRAP, restated for Rust

Change Risk Anti-Patterns (Savoia & Evans, 2007):

\[
\mathrm{CRAP}(m) = \mathrm{CC}(m)^{2}\,(1-\mathrm{cov}(m))^{3} + \mathrm{CC}(m)
\]

- \(\mathrm{CC}(m)\) — cyclomatic complexity of function \(m\). In Rust count `if`, `else if`, `while`, `for`, `loop`, `?` (each early-return path), `&&` / `||` short-circuit, `match` arm that is not a trivial `_` discard of an already-exhausted enum, and labeled-block breaks that introduce a path.
- \(\mathrm{cov}(m)\) — automated coverage of \(m\) as a fraction in \([0,1]\). Prefer **region** coverage from LLVM instrumentation; fall back to line coverage if region is unavailable for that function. Do not use doctest-only coverage as the CRAP input.

Properties of the curve:

| Situation | Score |
|---|---|
| \(\mathrm{CC}=1\), 100% cov | \(1\) (floor) |
| 100% cov | \(\mathrm{CRAP}=\mathrm{CC}\) |
| 0% cov | \(\mathrm{CC}^{2}+\mathrm{CC}\) |
| \(\mathrm{CC}=10\), 0% | \(110\) |
| \(\mathrm{CC}=6\), 0% | \(42\) |
| \(\mathrm{CC}=15\), 50% | \(43.125\) |
| \(\mathrm{CC}\ge 30\) | cannot get under classic 30 no matter the coverage |

Classic industry threshold: **30 = CRAPpy**.  
This protocol uses tighter numbers on *new production code* because high coverage collapses the score onto raw complexity, and Rust functions should rarely need CC > 8 if pattern matching is used honestly.

**Protocol thresholds (production functions only; tests excluded):**

| Actor | Max CRAP | Intent |
|---|---|---|
| Human-authored new code | **4** | Prefer extract-method only when the extract has a name that exists in the domain. |
| Agent-authored new code | **6** | Same scar-guard. Slightly looser because agents over-split when squeezed. |
| Legacy / imported | **30** then ratchet | No new function may exceed 8 CC without an exemption. |
| Generated (`build.rs`, proto) | exempt | Listed in `exemptions`. |

### 2.1 New, changed, and existing code (since 1.2.0)

The thresholds above apply to **new and changed** production functions. Existing code is held by a **baseline ratchet**, not by the threshold:

- The **baseline** is a `cargo crap --format json` report of the code as it was accepted (§16). A function is identified by its file and its name.
- A **new** function is one the baseline does not have (a new name, or a function moved to another file). It fails when its CRAP is above the threshold.
- A **changed** function is one the baseline has. A change may not take it above the threshold if its baseline score was at or under it. If its baseline score was already above the threshold, its score may not rise above that baseline score (a rise of 0.01 or less is noise and not a rise).
- With no baseline, every function is new, so the whole crate is held to the threshold.
- Generated and exempted functions (§19) are out of both rules.

The "Legacy / imported: 30 then ratchet" row means that imported code enters through the baseline at whatever it scores, 30 being the classic line, and may then only move down. A function leaves the ratchet for the threshold once a baseline refresh records it at or under the threshold.

`cargo tops gate` implements this in its crap layer: it reads the `cargo crap` JSON of the run, compares each production function of the crate with the baseline, and fails on any new function above the threshold and any changed function that broke the ratchet. The threshold is `gates.crap.threshold_agent`, or `threshold_production` when the instance has no agent threshold. An instance with neither (a `workspace-root`) is not judged at the root; each package's own instance is.

### 2.2 Exhaustive dispatch matches (since 1.2.0)

The CC rule above counts every non-trivial `match` arm. So an idiomatic exhaustive dispatch — `enum` + `match` as §5.4 rule 2 asks, one arm per variant, no guards, each arm a single call or expression — scores CC equal to its arm count. A CLI `main` that dispatches 29 subcommands scores at least 29 at full coverage and can never meet a threshold of 6.

Such a match may carry a `layer: crap` exemption (§19) that names the function as `symbol`, with a reason and an expiry, instead of being split. Splitting it into groups only to bring each part under the threshold is a scar-guard violation. The exemption holds only while the function stays a table: an arm that grows a guard, a nested branch, or more than one call or expression is real branching, and the function then answers to the threshold.

Counting such a match as one decision is a candidate CC refinement for a future major version. It changes the CRAP input, so this minor version does not make it.

**Scar-guard (normative):**  
A change whose *only* justification is “CRAP was 7, now 3” and that increases production SLOC or fragments a named abstraction is a protocol violation. Fix CRAP by adding a test that asserts a real behavior, or by simplifying control flow, or by extracting a *named* concept. Never by slicing a function into `foo_part_1`.

**Rust tools (any one, pinned in the instance file):**

- `cargo-crap` — walks functions, consumes `lcov.info` from `cargo llvm-cov --lcov`, JSON + baseline delta, `.cargo-crap.toml`, default fail at 30.
- `cargo-crappy` — same formula plus an idiom penalty (`CRAPPY = CRAP × (1 + 0.25·demerits)`). Useful as an *advisory* density signal, not as the gate (idiom penalties can fight A6).
- `cargo-crap4rust` — workspace reports, production-only default.

Canonical gate tool for this spec: **`cargo-crap`** against LLVM LCOV. Record the exact version in `rust-tops.lock` (a tiny TOML next to the instance).

---

## 3. Layered evidence model

Every behavior that ships must be justified by the **lowest layer that can actually see it**. Do not stack a UI test on something a unit test can kill.

| Tier | Layer ID | Question it answers | Default cadence |
|---|---|---|---|
| 0 | `static-lint` | Is this even Rust we will accept? | every PR |
| 0 | `unit` | Does this function keep its contract on chosen examples? | every PR |
| 0 | `doc` | Do public examples compile and hold? | every PR |
| 0 | `coverage` | Did tests reach the production regions we claim? | every PR |
| 0 | `crap` | Is untested complexity leaking in? | every PR |
| 0 | `laws` | Do the facts in this crate's files and gate reports clear the Bend law pack? | every PR |
| 0 | `security-advisory` | Did we pick up a known-bad crate? | every PR |
| 1 | `integration` | Do crate-public seams work together? | every PR |
| 1 | `acceptance` | Does the named user/oracle behavior hold? | every PR for changed journeys |
| 1 | `mutation` | Would a small bug in this diff survive the suite? | every PR on the diff |
| 1 | `property` | Does the invariant hold for a large sample / shrinkable space? | every PR if the crate class requires it |
| 2 | `snapshot` | Did a stable serialization / diagnostic change? | every PR when snapshots exist |
| 2 | `compile-fail` | Do intended type/API failures still fail? | every PR for proc-macros / public type tricks |
| 3 | `fuzz` | Does a byte/structure boundary panic or hang? | PR smoke + nightly |
| 3 | `concurrency` | Do loom/model schedules catch races? | PR on concurrent crates |
| 3 | `miri` | Does unsafe exhibit UB on the exercised paths? | PR on unsafe crates, else nightly |
| 3 | `sanitizer` | ASan/TSan/LSan on FFI or `unsafe` + alloc? | nightly |
| 4 | `formal-kani` | Is a bounded kernel free of panic/overflow/UB? | PR on kernels, else scheduled |
| 4 | `bench-callgrind` | Did instruction/cache counts regress? | PR on `hot_paths` |
| 5 | `bench-criterion` | What is the wall-clock distribution on this machine? | local / dedicated runner |
| 5 | `density-audit` | Did production SLOC grow without capability? | advisory on PR, blocking on release |

**Tiers are fixed (since 1.3.0).** A layer's tier comes from its id, by this table. An instance `tier` must equal the table's value or be absent; `cargo tops check` fails any other value, and a layer id the table does not list. `security-advisory` moved from tier 5 to tier 0 in 1.3.0, so the default `--max-tier 1` gate runs the advisory check.

### 3.1 The `laws` layer (since 1.1.0)

`LAWS.bend` states each law as a verdict function over a small case type. The layer has three parts:

1. **Case.** For each law, the runner reads one fact from the crate's files (for example `rust-tops.yaml`, `Cargo.toml`, `rust-toolchain.toml`, `.github/workflows/`, `Cargo.lock`) or from the reports this gate run wrote. From that fact it picks one constructor of the law's case type. A law with no readable fact is reported `not-evaluated`. It is never encoded as `Allow`, and a report may not call it a pass.
2. **Judge.** The runner writes the chosen cases as one Bend program that imports `LAWS.bend`. Its obligation is that each verdict is not `Refuse`. `bend` checks that program. The runner keeps no verdict table of its own.
3. **Outcome.** A `Refuse` fails the run. An `Ack` is recorded and does not fail. A missing `bend`, or one that does not report the locked version, fails the run (H-37 `missing_tool_not_green`).

The judge is the Bend release pinned in `laws/bend.lock`, fetched by `scripts/fetch-bend.sh` with a sha256 check. The fetch runs only when that release is absent. No law may claim to see author intent. `evidence_not_self_served` stays `not-evaluated` until a written pattern exists for it.

---

## 4. Crate classes and default tightness

Set `crate.class` in the instance file. Defaults below are the starting gate set. Tighten; do not loosen without an exemption.

| Class | Line cov | Region cov | CRAP (new) | Mutation kill / unexplained | Property | Fuzz | Miri | Kani | Callgrind |
|---|---|---|---|---|---|---|---|---|---|
| `library-core` | 95 | 90 | 6 / 4 | 95% / 0 on diff | required on pure functions | optional | if unsafe | kernels | hot paths |
| `library-util` | 90 | 85 | 6 / 4 | 90% / 0 on diff | encouraged | no | if unsafe | no | optional |
| `parser-codec` | 95 | 90 | 6 / 4 | 95% / 0 | required (roundtrip) | **required** | if unsafe | framing kernels | decode hot path |
| `concurrency-runtime` | 90 | 85 | 8 / 6 | advisory + loom | required (ordering) | no | required | selected | hot paths |
| `unsafe-kernel` | 95 | 90 | 6 / 4 | 95% / 0 | required | encouraged | **required** | **required** | required |
| `ffi-bridge` | 90 | 85 | 8 / 6 | 90% / 0 | optional | required on inbound bytes | required | optional | optional |
| `binary-cli` | 80 lib / journeys 100 | 75 | 8 / 6 | diff only | flags/parse | parse path | no | no | no |
| `binary-service` | 85 | 80 | 8 / 6 | diff + domain | protocol invariants | public codecs | if unsafe | no | p50 handlers |
| `no-std-embedded` | 90 | 85 | 6 / 4 | 95% / 0 | required | optional | required | kernels | size + cycle |
| `proc-macro` | 85 + compile-fail | n/a branch | 8 / 6 | trybuild is the mutant analog | optional | no | no | no | no |
| `workspace-root` | inherits per-package | | | | | | | | |

**`check` holds an instance to its class (since 1.3.0).** `cargo tops check` fails an instance when:
- a floor is looser than the class profile in `profiles/`: `gates.coverage.line_min_pct` and `region_min_pct`, the CRAP thresholds (a minimum may not drop, a maximum may not rise), and `gates.mutation.kill_rate_min_pct` and `unexplained_survivors_max`. A floor that is missing counts as looser. A live, dated exemption (§19) for the floor's layer (`coverage`, `crap` or `mutation`) whose `path` covers the whole crate lifts the failure. The path is `**`, `./**`, `**/*`, `.` or `./`, and the record names no `symbol`. An exemption for part of the crate, for one symbol, or an expired one does not.
- a layer the class requires is disabled or absent: `security-advisory` for every class, and each layer a class profile marks `required` (§4 table), except `acceptance`, which no class requires until it has a runner.

Each message names the file, the key, the class value and the one-line fix.

Numbers are *minima*. A 40-line pure codec with a spec should sit at 100% region coverage because anything less is laziness, not philosophy.

---

## 5. Static lint and density rules (Tier 0)

### 5.1 Commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
```

`cargo-deny` covers licenses, advisories, bans, sources. Pin `advisory-db` date in CI.

### 5.2 Clippy policy

Deny on every PR:

- `warnings` (rustc + clippy default)
- `clippy::correctness`
- `clippy::suspicious`
- `clippy::perf`
- `clippy::complexity` (the *simplify this* group — this is density)
- `clippy::dbg_macro`, `clippy::todo` in non-test code (allow `todo!` only under `cfg(test)` scaffolding that does not ship)

Pedantic and restriction are **opt-in per lint**, never as a group. Enabling `clippy::pedantic` wholesale produces allow-noise that hides real issues.

Recommended cherry-picks (warn, then deny once the crate is clean):

- `clippy::needless_pass_by_value`
- `clippy::trivially_copy_pass_by_ref`
- `clippy::cloned_instead_of_copied`
- `clippy::map_clone`
- `clippy::redundant_clone`
- `clippy::inefficient_to_string`
- `clippy::large_enum_variant` (then box the heavy variant, do not split the enum for the lint)
- `clippy::fn_to_numeric_cast_any`
- `clippy::undocumented_unsafe_blocks`
- `clippy::multiple_unsafe_ops_per_block`

### 5.3 Complexity lints vs CRAP

Clippy’s `cognitive_complexity` lives in **restriction** and is a coarse readability heuristic (default 25). It is *not* McCabe and must not be the CRAP input.

Use:

- **McCabe / CRAP tool** for the risk gate.
- **Clippy `too_many_lines`** as a soft density alarm, configured in `clippy.toml`:

```toml
cognitive-complexity-threshold = 12
too-many-lines-threshold = 40
too-many-arguments-threshold = 6
type-complexity-threshold = 250
```

Functions that are *tables* (big `match` on a protocol opcode, a perfect hash, a static dispatch table) may exceed `too-many-lines` if each arm is one or two lines. Mark them:

```rust
#[allow(clippy::too_many_lines)] // opcode table; arms are 1-line
fn dispatch(op: Op, s: &mut S) -> Result<(), Error> { /* ... */ }
```

That is density. A 40-line arm with nested `if` is not a table; extract it.

### 5.4 Normative density rules (production)

1. One function, one named job. The name is the spec of the job.
2. Prefer `enum` + `match` over boolean mode flags.
3. Prefer `&[T]`, `&str`, `impl AsRef<Path>` at boundaries; own only when storing.
4. No `.clone()` on a hot path without a comment that says *why the copy is required*.
5. No `Vec` when an array, `SmallVec`, or stack buffer is bounded and measured cheaper.
6. No trait object on a sealed closed set; use an enum or generics.
7. No interior mutability to dodge a borrow the design should have named.
8. `unwrap` / `expect` only when a broken invariant means a programmer bug. Message names the invariant.
9. Do not introduce a trait, builder, or middleware layer for a single call site.
10. Generics stay if they remove a branch from a hot path or enable static dispatch; they go if they exist “for flexibility.”
11. Macros only when they delete repetition that types cannot. Each macro has a compile-fail or expand test.
12. `async` is not a default. Use it when the crate waits on IO or is part of an existing runtime.
13. `unsafe` is never a performance first move. Measure, then isolate.
14. Public API surface is part of density. Every public item has a rustdoc sentence that states the contract. If you cannot write the sentence, the item should be private.
15. Deleted code is the highest-density patch. If a helper exists only for a removed feature, delete it in the same PR.

### 5.5 Test density (different rules)

Tests may repeat structure. Prefer:

- table-driven tests (`rstest`, `test_case`, or a `&[(&str, Expected)]` loop)
- one property instead of twenty examples when the property is the real claim
- `insta` only for things humans read (diagnostics, rendered help, stable IR)

Do not:

- share mutable fixtures across nextest processes (`#[serial]` does not work across nextest processes; use `test-groups` with `max-threads = 1` or isolate)
- test getters that the type system already enforces
- assert `debug` formatting unless that formatting is the product

### 5.6 `max_fn_cc` and the CRAP threshold (since 1.2.0)

`density.max_fn_cc` (default 8) is a design ceiling on any production function's cyclomatic complexity. Reviewers apply it in the `density-audit` layer. The CRAP threshold (§2) is what the gate enforces. Neither default changes here, and both hold.

CRAP is never below CC, and equals it at 100% coverage. So a function that meets a CRAP threshold of 8 or less already has CC within `max_fn_cc`: under the agent threshold of 6, a new function that passes the gate has CC at most 6. For new and changed code the CRAP threshold is the tighter limit and it governs; `max_fn_cc` never loosens it.

`max_fn_cc` governs the functions the threshold does not reach. A legacy function held only by the ratchet (§2.1) can raise its CC and still lower its CRAP by gaining coverage; review holds it to `max_fn_cc`: it may not grow past CC 8, and one already past 8 may not grow. An exempted function is held to `max_fn_cc` too, except an exhaustive dispatch match (§2.2), whose `crap` exemption also covers `max_fn_cc` because density treats such a match as a table (§5.3, `allow_long_table_functions`).

---

## 6. Unit, doc, integration, acceptance (Tiers 0–1)

### 6.1 Runner

```bash
cargo nextest run --workspace --all-features --profile ci
```

Why nextest: per-test process isolation, retries, slow-test detection, JUnit, archives. Configure `.config/nextest.toml`.

```toml
[profile.ci]
retries = 0
fail-fast = false
slow-timeout = { period = "10s", terminate-after = 2 }
status-level = "fail"

[profile.ci.junit]
path = "junit.xml"
```

Retries are **forbidden** on unit and property tests. A retry that makes a race “pass” is a lie. Retries may exist only on known-flaky *external* integration (networked third party), listed in exemptions.

### 6.2 Unit tests

Location: same module, `#[cfg(test)] mod tests`, or `src/.../foo/tests.rs` if the module is large. Do not create `src/lib.rs` dumping grounds.

Each unit test names **one behavior**:

```rust
#[test]
fn parse_rejects_truncated_varint() {
    assert_eq!(parse_varint(&[0x80]), Err(Error::Truncated));
}
```

Not `test_parse_varint_1`.

When the behavior is a grid of inputs, one test + table:

```rust
#[test]
fn sign_extend_4bit() {
    const CASES: &[(u8, i8)] = &[(0x00, 0), (0x07, 7), (0x08, -8), (0x0F, -1)];
    for &(in_, out) in CASES {
        assert_eq!(sign_extend_4(in_), out, "in={in_:#x}");
    }
}
```

That is fewer lines and a stronger spec than four functions.

### 6.3 Doc tests

Public functions with a non-obvious contract get a rustdoc example that compiles. Doc tests are not a coverage strategy; they are API documentation that happens to run. Do not stuff property tests into rustdoc.

```bash
cargo test --doc --workspace --all-features
```

### 6.4 Integration tests

`tests/*.rs` — crate-public API only. One file per *journey*, not per function.

### 6.5 Acceptance tests

Acceptance is an **oracle the implementation is not allowed to see**.

Forms that count:

- Gherkin / rstest given-when-then against a public binary (`assert_cmd` + `predicates`)
- a second, slower, obviously-correct implementation used only in tests (differential)
- a fixture corpus with expected hashes / decoded structures checked into `tests/corpus/`
- protocol traces: bytes in, state out

Uncle Bob’s point stands: agents cannot see the screen and will satisfy the tests they are given. If the product is a CLI, acceptance must run the binary. If the product is a library, acceptance is the public-API oracle, not the private helper.

```rust
// tests/accept_encode.rs
use assert_cmd::Command;

#[test]
fn encode_empty_emits_single_zero_block() {
    let mut cmd = Command::cargo_bin("enc").unwrap();
    cmd.write_stdin("").assert().success().stdout(predicates::ord::eq(&[0u8][..]));
}
```

### 6.6 Compile-fail / UI (proc-macros, tricky APIs)

`trybuild` or `ui_test`. These are the mutation tests of the type system: if a change makes an intended failure compile, the suite must go red.

### 6.7 Snapshots

`insta` for diagnostics, help text, pretty IR. Snapshots are reviewed artifacts. Never snapshot a full `Debug` of an unstable struct to “get coverage.”

---

## 7. Coverage (Tier 0)

### 7.1 Tool

**`cargo-llvm-cov`** wrapping `-C instrument-coverage`. Do not use tarpaulin as the gate; source-based LLVM coverage is the accurate method.

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked

cargo llvm-cov nextest --workspace --all-features \
  --lcov --output-path target/coverage/lcov.info \
  --fail-under-lines 90
```

HTML for humans: `cargo llvm-cov --html`.  
JSON for machines: `cargo llvm-cov --json --output-path target/coverage/cov.json`.

Branch and MC/DC coverage exist behind unstable flags (`--branch`, `--mcdc`) and have crashed on some async code. Protocol rule:

- Gate on **line** and **region** on stable.
- Collect branch on nightly when it works for the crate; never block a PR on a tool SIGSEGV.
- Do not enable branch coverage as a vanity number if the report is wrong on `async`/desugared state machines.

### 7.2 What to exclude

Default ignore:

- `tests/**`, `benches/**`, `examples/**`
- `target/**`
- generated files (`*.pb.rs`, `OUT_DIR`)
- `#[cfg(test)]` modules

Do **not** exclude production error paths. Untested `Err` arms are how CRAP should fire.

### 7.3 Per-file floors

A workspace average of 95% with one 20% file is a lie. Use `--fail-under-file-lines` or `cargo-coverage-gate` so every production file meets the class minimum, except files listed in exemptions.

### 7.4 How coverage interacts with CRAP

CRAP consumes the LCOV map. Functions with no coverage records score as 0% covered (pessimistic). That is intended: a new function that no test reached is automatically CRAPpy if it has any branching.

### 7.5 How the gate judges the floors (since 1.2.0)

`cargo tops gate` runs the suite once under `cargo llvm-cov nextest --workspace`, writes the LCOV for `cargo crap`, and writes a per-file summary with `cargo llvm-cov report --json --summary-only`. The run is workspace-wide on purpose: a test in any member counts for the code it reaches, and the one run stands for `unit`, `integration`, and `property`.

The floors judge only the instance's crate. Its root is the directory of the package `crate.name` names, found from the gate directory's `Cargo.toml` or from `cargo metadata`, and otherwise the gate directory itself. A file outside that root belongs to another crate and is not judged by this instance. Inside it, a file is production unless a glob in `gates.coverage.exclude_globs`, or one of `tests/**`, `benches/**`, `examples/**`, matches it. A glob matches the path relative to the crate root or any trailing part of it that starts at a directory, so `tests/**` also leaves out `src/tests/`, as `cargo llvm-cov` does.

The gate fails when the crate's line or region total over its production files is below `line_min_pct` or `region_min_pct`, and lists every production file below `per_file_line_min_pct` that no live `coverage` exemption names. A floor that is absent or `null` is not judged. A floor is met at exactly its value. A report that holds none of the crate's production files fails, because its floors cannot be judged.

---

## 8. Mutation testing (Tier 1)

### 8.1 Tool

**`cargo-mutants`** (current line as of 2026-06: v27.x). `mutagen` is unmaintained. Research tools (mutest-rs and similar) may be faster; they are not the protocol default until they are a `cargo install` with a stable report format.

```bash
cargo install --locked cargo-mutants
cargo mutants --version
```

### 8.2 What it does

1. Copy tree, run unmutated baseline (`cargo test` / nextest).
2. Apply AST-level mutants (function-body replacement by return-type default, operator swaps, match-arm removal, `NonZero` edge values, etc.).
3. Rebuild and run tests per mutant until fail or timeout.
4. Report **caught / missed (survived) / unviable / timeout**.

Coverage says a line ran. A survivor says no test *cared*.

### 8.3 PR gate (normative)

On every PR that touches `src/`:

```bash
git diff origin/main...HEAD --unified=0 > /tmp/pr.diff
cargo mutants --in-diff /tmp/pr.diff -j 2 -- --skip ignored
```

Pass criteria:

- baseline green
- **0 unexplained survivors** in the diff
- kill rate on examined mutants ≥ class minimum (see §4)

A survivor is explained only if *all* of the following hold:

1. The mutant is equivalent (`x + 0`, `x | 0` on a proven-zero, debug-only formatting).
2. A one-line reason is written in the PR and in `exemptions` if it will persist.
3. `#[mutants::skip]` is applied at the tightest scope (expression, then function, never file) with the same reason.

```rust
#[mutants::skip] // equivalent: trailing comma in this macro has no runtime effect
```

Skipping a whole module because “mutation is slow” is not an explanation.

### 8.4 Nightly / release

```bash
cargo mutants --workspace --jobs 4 --timeout-multiplier 2
```

Shard large workspaces. Do not require 100% workspace kill rate on UI glue; require it on `hot_paths` and `unsafe`.

### 8.5 Agent rule

The agent may run `cargo mutants --in-diff`. The agent may add a test that kills a survivor **if the test is derived from the spec**. The agent may not:

- weaken an assertion so the mutant is “caught” by a panic in setup
- wrap the function in `#[mutants::skip]`
- replace `==` with a looser comparison to make a flaky property pass

### 8.6 Cost control

Mutation recompiles. Keep it cheap:

- `--in-diff` on PRs
- `--file src/hot.rs` when iterating
- nextest for speed
- exclude generated code in `.cargo/mutants.toml`

```toml
# .cargo/mutants.toml
exclude_re = ["src/generated/", "/pb\\.rs$"]
examine_re = ["src/"]
```

---

## 9. Property-based testing (Tier 1)

Uncle Bob separately adopted property testing as a hardening layer agents can run. In Rust it is first-class.

### 9.1 Framework

Default: **`proptest`** (strategies + shrinking + seed replay).  
Acceptable: `quickcheck` for tiny crates, `arbtest` if already in tree. Do not mix two frameworks in one crate.

```toml
[dev-dependencies]
proptest = "1"
```

### 9.2 When required

Required for crate classes `library-core`, `parser-codec`, `unsafe-kernel`, `no-std-embedded`, `concurrency-runtime`, and for any function that claims an **invariant** rather than an example:

- roundtrip: `decode(encode(x)) == x` (or `== canonicalize(x)`)
- homomorphism: `f(a).combine(f(b)) == f(a.combine(b))`
- idempotence, commutativity, monotonicity
- bounds: `0 <= idx < len` after a successful parse
- inverse pairs: `compress`/`decompress`
- scheduler: “cancelling a task means it does not run after the cancel point”

### 9.3 Shape

```rust
proptest! {
    #[test]
    fn varint_roundtrip(n in 0u64..=u64::MAX) {
        let mut buf = Vec::new();
        encode_varint(&mut buf, n);
        let (got, rest) = decode_varint(&buf).unwrap();
        prop_assert_eq!(got, n);
        prop_assert!(rest.is_empty());
    }
}
```

Rules:

- Assert **properties**, not “does not panic” alone (panic-freedom is a bonus, not the claim).
- Persist failing seeds (proptest does this under `proptest-regressions/`). Those files are source.
- CI cases: 256 (default-ish) is acceptable; nightly: 1024–4096 on core parsers.
- Range the strategy at the *domain*, not `any::<Vec<u64>>()` if the function only accepts 32-byte keys.
- Independent oracle when possible: a slow encode in the test module, written to be obviously correct, compared against the fast path. The oracle is allowed to be verbose. The production path is not.

### 9.4 Interaction with mutation

A good property kills operator mutants that example tests miss (`<` vs `<=`, off-by-one on max). If a survivor is an inequality on a bound, add a property at the bound, do not add a single magic example unless the bound is a spec constant.

---

## 10. Fuzzing (Tier 3)

### 10.1 Where

Every boundary that turns **untrusted bytes** into structure: parsers, decoders, signature frames, compression, config files, wire protocols, `FromStr` on untrusted input.

```bash
cargo install cargo-fuzz
cargo fuzz init
cargo fuzz run decode_frame -- -max_total_time=30   # PR smoke
```

### 10.2 Target shape

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(frame) = parse_frame(data) {
        let mut out = Vec::new();
        encode_frame(&mut out, &frame);
        let again = parse_frame(&out).expect("roundtrip");
        assert_eq!(frame, again);
    }
});
```

A fuzzer that only checks “no panic” is acceptable as a *crash* net. Prefer a roundtrip or differential assert once parse succeeds.

### 10.3 Cadence

- PR: 30–60 s smoke on each target touched by the diff (or all targets if few).
- Nightly: hours, corpus checked in or stored as CI cache.
- Crashes become regression unit tests (minimized input in `tests/corpus/`).

Fuzz is not a substitute for CRAP or mutation. It is the layer that finds the input nobody wrote down.

---

## 11. Concurrency, Miri, sanitizers, Kani (Tiers 3–4)

### 11.1 loom

For crates that invent synchronization (`concurrency-runtime`, lock-free queues, custom executors):

```rust
#[cfg(loom)]
#[test]
fn mutex_is_exclusive() {
    loom::model(|| { /* ... */ });
}
```

Run under `RUSTFLAGS="--cfg loom"` in a dedicated CI job. loom explores schedules; it is not a substitute for TSan on FFI.

### 11.2 Miri

```bash
rustup +nightly component add miri
cargo +nightly miri test --lib
```

Required on any crate with `unsafe_policy != forbid` for the modules that contain `unsafe`. Write *mean* tests: zero length, odd alignment, empty slices, maximum `usize` lengths that should fail, overlapping ranges that must be rejected.

Miri is slow. Scope it: `cargo +nightly miri test --lib parse::unsafe_decode`.

### 11.3 Sanitizers

Nightly job on FFI / alloc-heavy / `unsafe` crates:

```bash
RUSTFLAGS="-Zsanitizer=address" cargo test -Zbuild-std --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Zsanitizer=thread"  cargo test -Zbuild-std --target x86_64-unknown-linux-gnu
```

AddressSanitizer for use-after-free / leaks across FFI. ThreadSanitizer for data races the borrow checker cannot see (raw pointers, C threads).

### 11.4 Kani

```bash
cargo install --locked kani-verifier
cargo kani setup
```

```rust
#[kani::proof]
fn decode_never_overflows_len() {
    let bytes: [u8; 8] = kani::any();
    let _ = decode_header(&bytes);
}
```

Use on small kernels: integer codecs, permission bitmaps, ring-buffer indices, `unsafe` slice construction from length+ptr that you already validated.

Set an unwind bound. If Kani times out, shrink the proof; do not delete it.

---

## 12. Performance protocol (Tiers 4–5)

Density without measurement is folklore. This section is the optimization half of the suite.

### 12.1 Order of attack (normative)

1. **Algorithm / complexity** — wrong big-O dwarfs micro-tuning.
2. **Allocations and copies** — count them (`dhat`, Gungraun DHAT, `#[cfg(feature = "count-allocs")]` tests).
3. **Layout** — field order, enum discriminant, SoA vs AoS, pack wire structs explicitly.
4. **Branch regularity** — `match` on compact enums, avoid unpredictable `hashmap` on tiny closed sets.
5. **SIMD / `packed_simd` / `std::simd`** only after 1–4 and with a portable scalar fallback test.
6. **`unsafe`** last, isolated, Miri-clean.

### 12.2 Release profile (default for binaries and measured libs)

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"      # binaries; libraries should leave panic=unwind unless you own the runtime
debug = 0
strip = "none"       # keep symbols until you measure size vs debugability; strip in dist job

[profile.release-lto-fat]
inherits = "release"
lto = "fat"
```

`panic = "abort"` is a binary/dist choice, not a library default. Libraries that abort surprise embedders.

`opt-level = "z"` or `"s"` is for `no-std` size classes; gate on **size of `.text`** plus a cycle count, not on Criterion ms.

### 12.3 Local: Criterion

```toml
[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }

[[bench]]
name = "hot"
harness = false
```

```rust
fn bench_decode(c: &mut Criterion) {
    let bytes = include_bytes!("../tests/corpus/frame.min");
    c.bench_function("decode_min", |b| {
        b.iter(|| parse_frame(std::hint::black_box(&bytes[..])).unwrap())
    });
}
```

Always `black_box` input and use the output. Do not bench `println!`.

Criterion is **not** a CI gate on GitHub-hosted VMs.

### 12.4 CI: Gungraun / iai-callgrind

Count instructions (`Ir`), data-cache reads/writes, estimated cycles. Fail the PR if a named hot path exceeds:

| Event | Max regression |
|---|---|
| `Ir` (instructions) | **+2%** on `library-core` / kernels; **+5%** otherwise |
| Estimated cycles | **+5%** / **+8%** |
| Heap bytes (DHAT) on a bounded fixture | **+0%** without a comment |

One-shot, environment-stable. Store baselines in `benches/baselines/` or the tool’s default `target/iai` cache that CI restores.

A regression is allowed only with a written reason in the PR (“adds constant-time compare, +4% Ir, required for A8”) and an instance-file note.

### 12.5 Density-related perf anti-patterns (deny in review)

- Returning `String` when `&str` or `Cow<str>` suffices.
- `format!` in a hot loop.
- `collect::<Vec<_>>()` only to iterate again.
- `Box<dyn Error>` on an inner loop; use a concrete `enum Error`.
- HashMap for `N ≤ 8` closed keys; use `match` or array.
- `async` function that never awaits.
- Logging on the success path at `info` inside a per-packet function.

### 12.6 Size (embedded / wasm)

Track `cargo bloat` or `twiggy` on release artifacts. A new public generic that monomorphizes across 12 types is a density and size defect unless each monomorph is required.

---

## 13. Security and supply chain

```bash
cargo deny check advisories bans licenses sources
cargo audit   # acceptable alternative for advisories-only
```

`RUSTSEC` on a crate you call in a parser or crypto path is a Tier 0 fail. Dev-only advisories may be postponed with an exemption expiry ≤ 30 days.

Pin `Cargo.lock` for binaries and workspaces that ship. Libraries published to crates.io follow crates.io lockfile norms but CI still uses `--locked`.

---

## 14. Light agent harness

This is the entire process cage. Do not add more stages unless a layer in §3 is failing.

### 14.1 Files the agent reads

- `AGENTS.md` (human-facing short form of this spec)
- `rust-tops.yaml` (instance)
- `rust-tops.lock` (tool versions)
- the module-level rustdoc contract

### 14.2 Standing orders to the agent

1. Derive tests from the stated contract, not from the current body.
2. Keep production functions small *because the domain is small*, not because CRAP yelled.
3. After implementation: `fmt`, `clippy -D warnings`, `nextest`, `llvm-cov` (if available), `cargo crap`, `cargo mutants --in-diff`.
4. Stop when gates pass. Do not “clean up” by introducing traits, builders, or extra files.
5. If a mutant survives, add the missing assertion or property. If CRAP is high, add coverage on the risky branch or reduce real branching.
6. Do not enable extra clippy groups. Do not add dependencies for a one-liner.
7. Prefer standard library. A new crate must pull its weight in *deleted production lines* or *measured* perf/safety.

### 14.3 `AGENTS.md`

`cargo tops init` writes `AGENTS.md`. That file is the maintained harness for protocol 1.3.0. The agent runs `cargo tops check`, then `cargo tops gate`. A Bend `Refuse` fails the run. `not-evaluated` is never a pass. Read `rust-tops/cases.json` under the Cargo target directory (`$CARGO_TARGET_DIR` when it is set, otherwise `cargo metadata`'s `target_directory`). Do not edit `LAWS.bend`, `laws/bend.lock`, or `scripts/fetch-bend.sh` to get a green gate. Do not pass `--fail-under-lines` unless the class floor says so and the LCOV is already nonempty.

### 14.4 The adoption record `RUST-TOPS.md` (since 1.2.0)

Every adopting repository keeps a `RUST-TOPS.md` at its root. It is the one page a reader or an agent opens to learn what the repo adopted. It records:

1. that the repo adopts this protocol, with a link to <https://github.com/Hedronite/rust-tops>;
2. the adopted protocol version and the URL of that version of this spec;
3. the `cargo-tops` version the gate uses;
4. each crate's class and thresholds, one row per `rust-tops.yaml`, taken from that file's `gates`;
5. how to run the gate locally;
6. where the CRAP baseline lives and how to refresh it;
7. every exemption in force;
8. known deviations from this protocol.

The template is [`config/drop-in/RUST-TOPS.md`](config/drop-in/RUST-TOPS.md). `cargo tops init` writes it beside the instance, with items 2–4 and 6 filled from that instance. When `init` runs in one crate's directory of a larger repo, move its row into the root record and delete the copy. Keep it current when an instance, an exemption, or the baseline path changes. A record that disagrees with a `rust-tops.yaml` is wrong; the instance wins.

---

## 15. Repository layout

```
.
├── rust-tops.yaml                 # instance of the schema
├── rust-tops.lock                 # pinned CLI versions
├── rust-toolchain.toml            # channel = crate.msrv
├── LAWS.bend                      # Bend law pack (laws layer, §3.1)
├── laws/bend.lock                 # pinned Bend release and sha256
├── scripts/fetch-bend.sh          # fetches that release
├── AGENTS.md
├── RUST-TOPS.md                   # adoption record (§14.4)
├── clippy.toml
├── deny.toml
├── .cargo/mutants.toml
├── .cargo-crap.toml
├── .config/nextest.toml
├── Cargo.toml
├── Cargo.lock
├── src/
├── tests/                         # integration + acceptance
│   ├── accept_*.rs
│   └── corpus/
├── tests/compile-fail/            # trybuild (if any)
├── benches/
│   ├── hot.rs                     # criterion and/or gungraun
│   └── baselines/
├── fuzz/
│   └── fuzz_targets/
├── proptest-regressions/
└── .github/workflows/rust-tops.yml
```

`RUST-TOPS.md` (the adoption record, §14.4) sits at the repo root beside `AGENTS.md`. A tracked CRAP baseline (§16) is source.

Generated reports (`target/coverage`, mutants scratch, iai output, `target/rust-tops/cases.bend`) are artifacts, not source.

---

## 16. `.cargo-crap.toml` and coverage wiring

```toml
# .cargo-crap.toml (the drop-in, config/drop-in/cargo-crap.toml)
threshold = 6.0
try-weight = 0.0
exclude = ["src/generated/**", "tests/**", "benches/**", "examples/**"]
```

`cargo-crap` does not fail the run by itself here: the gate reads its JSON and decides (§2.1). Earlier text showed `fail = true` and a `[coverage]` table; neither is a `cargo-crap` field, and the LCOV path is passed on the command line.

Pipeline in CI, as `cargo tops gate` runs it (artifacts in `<target>/rust-tops/`):

```bash
cargo llvm-cov nextest --workspace --all-features --profile ci \
  --lcov --output-path target/rust-tops/lcov.info
cargo llvm-cov report --json --summary-only --output-path target/rust-tops/coverage.json
cargo crap --lcov target/rust-tops/lcov.info --format json --output target/rust-tops/crap.json
```

The gate then judges the coverage floors (§7.5) and fails if any *non-exempt production* function is new and has `crap > threshold_agent`, or is in the baseline and broke the ratchet (§2.1).

**Baseline.** The baseline is a `cargo crap --format json` report. By default it is `target/crap-baseline.json` (in the Cargo target directory, so `CARGO_TARGET_DIR` moves it). An instance may name a tracked path instead with `gates.crap.baseline`, relative to the instance directory; a repo whose CI runs the gate should, or CI judges every function as new. `cargo tops crap-baseline` copies the last gate run's report there, sorted by file and function. Refresh the baseline to record code that improved or new code at or under the threshold, in the same change. Never refresh it to let a score rise; a refresh is reviewed like any other change to a gate.

---

## 17. CI matrix (reference workflow)

Jobs are separated so a 90-minute Miri run cannot hide a 20-second clippy fail.

| Job | When | Blocks merge |
|---|---|---|
| `fmt-clippy-deny` | all PRs | yes |
| `nextest-doc` | all PRs | yes |
| `coverage-crap` | all PRs | yes |
| `mutants-diff` | PRs touching `src/` | yes |
| `property` | included in nextest | yes (if present) |
| `fuzz-smoke` | PRs touching parsers | yes (timeout fail = infra, crash = fail) |
| `loom` | concurrent crates | yes |
| `miri` | unsafe crates | yes |
| `kani` | kernels | yes |
| `callgrind-hot` | PRs touching `hot_paths` | yes |
| `fuzz-nightly` | schedule | no (ticket on crash) |
| `mutants-full` | schedule / release | release only |
| `criterion` | dedicated runner optional | no |
| `sanitizers` | schedule | release for ffi/unsafe |

---

## 18. Scoring model (optional dashboard)

Not a vanity badge. Use it internally to see which layer is carrying the crate.

\[
\mathrm{Score} = 0.25\,\mathrm{Kill} + 0.20\,(1-\mathrm{CrapNorm}) + 0.20\,\mathrm{RegionCov} + 0.15\,\mathrm{Prop} + 0.10\,\mathrm{Fuzz} + 0.10\,\mathrm{Perf}
\]

- \(\mathrm{Kill}\) — mutation kill rate on in-scope mutants, \(0..1\)
- \(\mathrm{CrapNorm}\) — fraction of production functions with CRAP above agent threshold
- \(\mathrm{RegionCov}\) — region coverage \(0..1\)
- \(\mathrm{Prop}\) — 1 if every documented invariant has a property or proof, else 0
- \(\mathrm{Fuzz}\) — 1 if every untrusted boundary has a target, else 0
- \(\mathrm{Perf}\) — 1 if no unexplained Callgrind miss on hot paths

Do **not** put this score in README marketing. It is a diagnostic.

---

## 19. Exemption record

Every skip is data.

```yaml
exemptions:
  - path: src/generated/schema.rs
    layer: crap
    reason: prost output; regenerated; covered by decode roundtrip on public API
    owner: platform
    expires: "2027-01-01"
  - path: src/wire.rs
    symbol: encode_legacy_v1
    layer: mutation
    reason: mutant replaces deprecated encoder body with Default; journey accept_legacy_v1 kills behavior
    owner: protocol
    expires: "2026-12-01"
```

Expired exemptions fail CI. Reasons shorter than one clause are rejected.

How the gate reads them (since 1.2.0): `path` is a glob matched like the coverage exclude globs (§7.5), relative to the crate root. A `crap` exemption covers the functions of the matching files, or, with `symbol`, only the function of that name (`run` also matches a method `Cli::run`). A `coverage` exemption takes a file out of the per-file floor; its `symbol` is not read, and the file still counts toward the crate total. An exemption counts until its `expires` date. On that date and after, or when `expires` is not a real day, it exempts nothing and the gate reports it as a failure.

---

## 20. Worked protocol — a 40-line codec

Target: varint encode/decode, `library-core` + `parser-codec` traits.

**Contract (the spec, written first):**

- Unsigned LEB128.
- `encode` writes the minimal byte sequence.
- `decode` consumes exactly those bytes and returns `(value, rest)`.
- Overlong encodings are `Error::NonCanonical`.
- Truncation is `Error::Truncated`.
- 10-byte overflow is `Error::Overflow`.

**Production shape (density):** two functions, one error enum, no traits.

**Tests that must exist before the agent is done:**

1. Table of canonical encodings (0, 1, 127, 128, 300, `u64::MAX`).
2. Property: `decode(encode(n)) == (n, empty)` for all `u64`.
3. Property: any successful decode of random bytes, if re-encoded, matches the consumed prefix (canonicalizes or rejects overlong — pick one and test it).
4. Fuzz target on `&[u8]` with the same roundtrip assert.
5. Mutation on `src/varint.rs` — survivors on `< 0x80` vs `<=` are killed by the 127/128 table plus the property.

**CRAP expectation:** each function CC ≈ 3–5, 100% region coverage, CRAP = CC ≤ 6. If someone writes a 12-branch god decoder, reject the design, do not add 12 tests to paint it green.

**Perf:** Callgrind on `decode` of a 1-byte and a 10-byte fixture. No heap allocation on decode into `u64`.

That is the whole suite applied to a small surface. Larger crates are the same protocol with more files, not more process.

---

## 21. Command cookbook

```bash
# install (once)
cargo install --locked cargo-nextest cargo-llvm-cov cargo-mutants cargo-crap cargo-deny cargo-fuzz
rustup component add llvm-tools-preview clippy rustfmt
# optional
cargo install --locked kani-verifier && cargo kani setup
cargo install --locked gungraun-runner   # or iai-callgrind-runner; follow current crate name

# PR loop
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
cargo nextest run --workspace --all-features --profile ci
cargo test --doc --workspace
cargo llvm-cov nextest --workspace --all-features \
  --lcov --output-path target/coverage/lcov.info --fail-under-lines 90
cargo crap
git diff origin/main...HEAD --unified=0 > /tmp/pr.diff
cargo mutants --in-diff /tmp/pr.diff

# extra by class
cargo +nightly miri test --lib
cargo kani
cargo fuzz run decode_frame -- -max_total_time=60
cargo bench --bench hot          # local criterion
# callgrind bench via the crate's harness
```

---

## 22. Normative vs advisory

**Normative (non-compliance is a failed protocol run):** §§1 A1–A8, 2 (formula + scar-guard + thresholds for new and changed code + the baseline ratchet), 4 class minima, 5.4 density rules, 6.5 acceptance-oracle rule, 7 coverage floors, 14.4 the adoption record, 8.3 zero unexplained survivors on the diff, 9.2 required properties, 11.2 Miri on unsafe, 12.1 order of attack, 12.4 Callgrind on listed hot paths, 14.2 agent orders, 19 exemption shape, 3.1 a `Refuse` from an enabled `laws` layer.

**Advisory:** clippy pedantic cherry-picks, dashboard score, Criterion numbers on laptops, `cargo-crappy` idiom penalty, branch coverage until LLVM is stable on the crate.

---

## 23. Versioning

This spec is `1.3.0`. Additive layers bump minor. Changing a default threshold or the CRAP formula bump major. Instances pin `protocol.version`.

1.1.0 added the `laws` layer (§3.1). A 1.0.0 instance that does not list `laws` still validates.

1.2.0 states how the thresholds apply to new and changed code and the baseline ratchet (§2.1, §16), the dispatch-match exemption (§2.2), the relation of `max_fn_cc` to CRAP (§5.6), how the floors are judged (§7.5), how exemptions are read (§19), and the adoption record (§14.4). The schema adds the optional `gates.crap.baseline`. A 1.1.0 instance still validates. No default threshold and no part of the formula changed.

1.3.0 fixes the tier of each layer by its id and moves `security-advisory` to tier 0 (§3), makes `check` hold an instance to its class (§4), and takes the file name `PROTOCOL.md`. The schema describes `tier` as fixed by layer id. A 1.2.0 instance with a loose floor or a wrong tier now fails `check` until it is fixed or carries a dated exemption. No default threshold and no part of the formula changed.

---

## 24. References (tools and origins)

- Savoia & Evans, Change Risk Anti-Patterns / Google Testing Blog (CRAP formula).
- Robert C. Martin, 2026 notes on light agent harnesses, CRAP, mutation, property testing, and scars.
- Rich Sutton, *The Bitter Lesson*.
- `cargo-llvm-cov`, `cargo-mutants` (`mutants.rs`), `cargo-crap` / `cargo-crappy` / `cargo-crap4rust`.
- `proptest`, `cargo-fuzz`, `loom`, Miri, Kani, Criterion, Gungraun / iai-callgrind.
- Clippy lint configuration (`cognitive-complexity-threshold`, `too-many-lines-threshold`).
- cargo-deny, nextest.

Tool versions belong in `rust-tops.lock`, not in this paragraph.
