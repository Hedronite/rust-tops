# spec/

`tops_core.bend` is a Bend spec of three cargo-tops decisions. It is the spec
for the conformance corpus (`conformance/tops_core.json`) that the Rust tests
replay. Checks:

```text
bend spec/tops_core.bend               # runs main (or checks when no main); rc 0
bend spec/tops_core.bend --check-only  # prints ALL PROOFS CHECK
bend spec/tops_core.bend --verdict     # rechecks every proof with the Lean kernel; exit 0
```

Bend is the pinned 2.0.36 (`laws/bend.lock`, `scripts/fetch-bend.sh`). `--verdict`
needs Lean 4.34.0. Set `BEND_NO_TELEMETRY=1`.

## Types and functions

Types are enums and `Nat` only. There is no `F32`.

| Bend | Rust counterpart | Notes |
|---|---|---|
| `Outcome` | `steps::Outcome`, `crates/cargo-tops/src/steps.rs:108` | five variants, same names |
| `Row` | the `&'static str` rows of `crates/cargo-tops/src/gate/judge.rs:278` and `:315` | `RowPass` is `"pass"`, `RowFail` `"fail"`, `RowToolMissing` `"tool-missing"`, `RowToolMismatch` `"tool-mismatch"`, `RowVacuous` `"vacuous"`, `RowNotRun` `"not-run"` |
| `Expiry` | `Exemption::expires` plus `steps::valid_expiry`, `steps.rs:713` | `Valid(day)` is a real day, `Invalid` is a string `valid_expiry` rejects |
| `runs_verdict(rs)` | `runs_verdict`, `crates/cargo-tops/src/gate/judge.rs:278` | worst run by rank; the empty list is `RowPass`, as the Rust |
| `layer_verdict(rs, missed)` | `layer_verdict`, `crates/cargo-tops/src/gate/judge.rs:315` | `missed` is `Judgement::missed(id)` (`judge.rs:45`); `None` judgement is `missed = False` |
| `live(e, today)` | `Exemption::live`, `crates/cargo-tops/src/exemptions.rs:42` | strictly after `today`; `Invalid` never counts. A day `n` is the Rust string `exemptions::civil_date(n)` (`exemptions.rs:98`) |

Helpers that carry the proofs and have no Rust counterpart: `to_row`, `rank`
(rank of a row; `RowNotRun` ranks 0 and is never returned by `runs_verdict`),
`worse` (the worse of two outcomes), `worst_of`, `gate` (a pass that missed a
floor becomes `RowFail`), `has_failed`, `all_passed`, `no_runs`, `cat`.

Two names differ from the spec text, because Bend has one constructor
namespace and `Base` owns `Fail`: the six `Row` constructors carry a `Row`
prefix (`RowPass`, `RowFail`, `RowNotRun`, `RowToolMissing`, `RowToolMismatch`,
`RowVacuous`), and list concatenation is `cat(a, b)` (Bend's `++` is for
strings). The corpus generator maps `Row*` back to the Rust strings.

## Laws

Every law binds its variables with `for` and is proved by case split or
induction. None is a bare `{==}`. A hypothesis is a `for h:` proof argument,
as in Base's `Equal.cong`.

| ID | Law | Binds | Proof |
|---|---|---|---|
| C1 | `fail_outranks_all` | `rs` | induction on `rs` (`worst_failed`), then `Equal.cong` through `to_row` |
| C2 | `green_needs_every_run` | `rs`, `missed` | case split on `rs`; `gate_pass` splits `gate` by rows and `missed`; induction on `rs` (`worst_pass`) |
| C3 | `order_does_not_matter` | `a`, `b` | induction on `a` (`worst_cat`), then `worse_comm` (25 pairs) and `worse_assoc` (case split), through `Equal.trans` |
| C4 | `more_runs_never_better` | `r`, `rs` | case split on all 25 `(r, worst_of(rs))` pairs (`rank_step`) |
| C5 | `expired_covers_nothing` | `e`, `today` | case split on `e`; the witness `d` is returned with its two proofs |

## Trust base

Stated by the Bend README:

- Plain `bend` checks with a checker that has no proof.
- `--verdict` rechecks each proof with a small kernel (`bend2/bendtt.lean`)
  that has a Lean proof; the parser and the translation into that kernel are
  unproven.
- Laws hold for Bend's semantics; backends can differ (for example `F32` NaN).
  This spec uses no `F32`.

What the spec adds: it is a hand-written model. It binds the Rust only through
the conformance corpus, which the Rust tests replay over the whole bounded
domain. A disagreement between the corpus and the Rust is a finding for the
owners to rule on, not a bug to patch quietly.
