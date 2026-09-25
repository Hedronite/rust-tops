# Rust Testing and Optimization Protocol Suite (Rust-TOPS)

**Protocol ID:** `rust-tops`  
**Version:** 1.0.0  
**Status:** Adopted specification  
**Machine schema:** [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json)  
**Example instance:** [`config/rust-tops.example.yaml`](config/rust-tops.example.yaml)  
**Audience:** Rust library and binary authors, CI owners, and coding agents.

This document is the canonical protocol. The JSON Schema is the machine contract. If prose and schema disagree, fix the schema and then the prose.

---

> **Bend2 encode track — v0 sketch, non-normative:** see [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md). Does not amend this 1.0 body or schema.

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
| 5 | `security-advisory` | Did we pick up a known-bad crate? | every PR |
| 5 | `density-audit` | Did production SLOC grow without capability? | advisory on PR, blocking on release |

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

### 14.3 Suggested `AGENTS.md` stub

```markdown
# Agent standing orders (Rust-TOPS 1.0)

Goal: correct, dense, fast Rust. Production: fewest necessary lines.
Tests: as many as the contract needs. Do not write tests by reading the impl.

Gates before you stop:
- cargo fmt --all
- cargo clippy --all-targets --all-features -- -D warnings
- cargo nextest run --profile ci
- cargo llvm-cov nextest --fail-under-lines <class>
- cargo crap          # new fns ≤ 6
- cargo mutants --in-diff
- property tests for every claimed invariant
- no new clone/alloc on paths listed in rust-tops.yaml hot_paths

Forbidden:
- #[mutants::skip] without an equivalent-mutant reason
- splitting a function only to lower CRAP
- new traits/builders with a single impl
- unwrap on library success paths
```

---

## 15. Repository layout

```
.
├── rust-tops.yaml                 # instance of the schema
├── rust-tops.lock                 # pinned CLI versions
├── AGENTS.md
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

Generated reports (`target/coverage`, mutants scratch, iai output) are artifacts, not source.

---

## 16. `.cargo-crap.toml` and coverage wiring

```toml
# .cargo-crap.toml
threshold = 6.0
fail = true
exclude = ["src/generated/**", "tests/**"]

[coverage]
lcov = "target/coverage/lcov.info"
```

Pipeline in CI:

```bash
cargo llvm-cov nextest --workspace --all-features \
  --lcov --output-path target/coverage/lcov.info \
  --fail-under-lines 90
cargo crap --format json --output target/crap.json
```

Fail if any *non-exempt production* function has `crap > threshold_agent` (6) on new code, or if delta vs `target/crap-baseline.json` introduces a new function above threshold.

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

**Normative (non-compliance is a failed protocol run):** §§1 A1–A8, 2 (formula + scar-guard + thresholds for new code), 4 class minima, 5.4 density rules, 6.5 acceptance-oracle rule, 7 coverage floors, 8.3 zero unexplained survivors on the diff, 9.2 required properties, 11.2 Miri on unsafe, 12.1 order of attack, 12.4 Callgrind on listed hot paths, 14.2 agent orders, 19 exemption shape.

**Advisory:** clippy pedantic cherry-picks, dashboard score, Criterion numbers on laptops, `cargo-crappy` idiom penalty, branch coverage until LLVM is stable on the crate.

---

## 23. Versioning

This spec is `1.0.0`. Additive layers bump minor. Changing a default threshold or the CRAP formula bump major. Instances pin `protocol.version`.

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
