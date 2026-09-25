# Bend2 × rust-tops — candidate laws

**Status: candidates — not protocol 1.0.** This file does not amend
[`RUST_TOPS.md`](RUST_TOPS.md) or the schema. Hard/soft is a steering hint
for agents encoding these as Bend laws. Promote a row into protocol 1.0 only
with an explicit spec bump.

Sketch mapping: protocol intents → fail-closed agent laws → Halo / agent
editing Rust. Humans read this as advisory. Agents under an AI-ops seat
treat `hard` rows as fail-closed once encoded.

## Layout

```text
<crate-or-workspace>/
  rust-tops.yaml          # instance (class, floors, hot_paths, exemptions)
  AGENTS.md               # light harness
  LAWS.bend               # Bend pack encoding rust-tops intents for this host
```

| Artifact | Role |
| --- | --- |
| `RUST_TOPS.md` + schema | Global PROTOCOL (normative 1.0) |
| `POLICIES.md` | Harvested gates / checks (practice) |
| `rust-tops.yaml` | Per-crate instance (tightness, floors) |
| `LAWS.bend` | Fail-closed agent laws derived from PROTOCOL + instance |

Laws may only demand gates already in the instance class matrix.
Laws may not invent Kani on a binary that classified as skipping proofs.

## Candidate laws

Harvested applying rust-tops 1.0. Generic: no product crate names as law.

| id | rust-tops source | candidate law | hard/soft | notes |
| --- | --- | --- | --- | --- |
| H-01 | A4+§6.5 | `law.evidence_not_self_served` | hard | tests are not derived from the impl under test |
| H-02 | A4+§6.5 | `law.vector_replay_required` | hard | a generated corpus that is never replayed is not evidence |
| H-03 | A8 | `law.primitive_port_knows_its_source` | hard | a crypto/primitive port names its upstream |
| H-04 | §5.5+§6.5 | `law.no_source_text_assertions` | hard | do not assert by grepping production source |
| H-05 | §17+§5.1 | `law.every_cfg_is_seen_by_ci` | hard | every `cfg(target_os)` / feature has a lane |
| H-06 | §17+A7 | `law.ci_matrix_matches_class` | hard | class MUSTs appear as jobs |
| H-07 | A5 | `law.no_sealed_class_data_in_side_channels` | hard | secrets never in logs, events, help, tests names |
| H-08 | §19+A8 | `law.no_unowned_hardening_todo` | hard | a security TODO has an owner or is a bug |
| H-09 | §6.6+A7 | `law.cap_is_named_or_it_is_a_bug` | hard | magic numeric caps are named constants with tests |
| H-10 | §4 | `law.class_declared_before_edit` | hard | `rust-tops.yaml` exists before the first production edit |
| H-11 | §5.4 | `law.contract_lives_near_its_owner` | soft→hard | rustdoc / schema next to the type |
| H-12 | §5.4+A6 | `law.no_write_only_state` | soft | a field that is only written is dead |
| H-13 | §5.5 | `law.artifact_asserted_by_structure` | hard | on-disk / wire artifacts have structural tests |
| H-14 | §10.1 | `law.fuzz_untrusted_ingress` | hard (parser-codec) | untrusted bytes have a fuzz target |
| H-15 | §9.2 | `law.roundtrip_property` | hard | encode/decode has a property, not only fixtures |
| H-16 | A4 | `law.spec_schema_shipped` | hard | a JSON/wire schema is in-tree before the parser |
| H-17 | §13 | `law.constant_time_tag_cmp` | hard | auth tags compare in constant time |
| H-18 | §12.1 | `law.bounded_range_io` | soft | range reads declare a max and page |
| H-19 | A8 | `law.zeroize_confidential_buffers` | soft→hard | secret buffers implement zeroize |
| H-20 | §6.3+§5.4.14 | `law.pub_contract_doc_example` | soft | public fn has a rustdoc example or a named skip |
| H-21 | A2+§7-§8 | `law.library_public_oracle` | hard | library public API has an oracle outside the CLI |
| H-22 | §5.1 | `law.fmt_is_tier0` | hard | rustfmt check is a merge gate |
| H-23 | §14.2 | `law.lock_honesty` | hard | `--locked` matches committed `Cargo.lock` |
| H-24 | §17 | `law.msrv_enforced` | hard | MSRV job exists |
| H-25 | §5.2 | `law.clippy_toolchain_pinned` | hard | clippy version is locked or toolchain-pinned |
| H-26 | §5.2 | `law.no_pedantic_group` | hard | no `clippy::pedantic` / `restriction` as a group |
| H-27 | §5.4 | `law.fallible_integrity_root` | hard | integrity roots return `Result`, not `unwrap` |
| H-28 | §6.5 | `law.event_safe_emission` | hard | telemetry/events cannot carry secrets |
| H-29 | §5.1+§17 | `law.apply_proves_the_gates` | hard | a claimed green SHA actually ran the gates |
| H-30 | §8.3 | `law.baseline_green_before_mutants` | hard | no `--baseline skip` |
| H-31 | §19 | `law.marker_sweep_is_not_a_sweep` | hard | clearing a TODO marker is not clearing the bug |
| H-32 | §5.4 | `law.security_primitive_has_a_unit_oracle` | hard | constant-time / auth helpers have unit tests |
| H-33 | §5.4 r8 | `law.cap_constants_have_boundary_tests` | hard | named caps have `==` / `>` tests |
| H-34 | §8.2 | `law.measurement_is_isolated` | hard | coverage/mutants do not race a shared `target/` |
| H-35 | §7 | `law.record_only_artifacts_are_nonempty` | hard | empty LCOV/CRAP JSON is a fail |
| H-36 | §8.3 | `law.slice_completes_with_killtest` | hard | a density slice that adds no test is incomplete |
| H-37 | G-P10 | `law.missing_tool_not_green` | hard | a missing cargo plugin is a fail, not a skip |
| H-38 | A3 | `law.mutant_kill_without_scar` | hard | kill the mutant; do not scar the production fn |
| H-39 | §5.1 | `law.report_names_its_argv` | hard | a gate report names the argv that ran |
| H-40 | A6+§5.4 r1 | `law.arg_group_becomes_a_name` | hard | >6 args become a domain type; never `*_part_2` |
| H-41 | §10.1 | `law.scaffold_runs_once` | hard | fuzz/workspace scaffolds do not collide |
| H-42 | A2 | `law.canonical_form_is_a_fixed_point` | hard | canonicalize(canonicalize(x)) == canonicalize(x) |
| H-43 | §16 | `law.crap_wires_coverage` | hard | CRAP reads the LCOV that was just recorded |
| H-44 | A3 | `law.density_extract_has_domain_name` | hard | extracts are named for the domain, not `Args2` |
| H-45 | §12 | `law.coverage_features_match_workspace` | hard | coverage `--all-features` matches the merge gate |
| H-46 | §12 | `law.perf_without_runner_is_not_gate` | hard | missing Criterion/Callgrind ≠ green perf |
| H-47 | CI | `law.sysdeps_before_all_features` | hard | FFI headers exist before `--all-features` |
| H-48 | H-05 | `law.every_cfg_target_has_a_lane` | hard | Linux-only GHA does not cover `cfg(target_os = "macos")` |
| H-49 | H-24 | `law.msrv_is_lock_max_including_target_gated` | hard | take the max rustc across target-gated deps |
| H-50 | H-35 | `law.artifact_check_is_its_own_step` | hard | `test -s` is not folded into cargo argv |
| H-51 | CI | `law.job_declares_merge_semantics` | hard | `blocks_merge` / `cadence` / `die` are distinct |
| H-52 | §17 | `law.drop_in_is_a_floor_not_a_ceiling` | hard | instance may add jobs; may not drop floor jobs |
| H-53 | CI | `law.single_deny_site` | hard | `-D warnings` on the clippy command only |

## Encode track

```text
rust-tops PROTOCOL ──encode──► LAWS.bend ──steer──► agent editing Rust
        │                                              │
        └────────────── apply (gates) ◄────────────────┘
```

A `LAWS.bend` pack **ships in v0.1.3** as a **provisional kit overlay**, not protocol 1.0.
Schema stays Bend-free. Hard rows encode as `Refuse`; soft rows as `Ack`.

| Path | What |
| --- | --- |
| [`laws/LAWS.bend`](laws/LAWS.bend) | 53 laws + proofs |
| [`laws/PROOF.bend`](laws/PROOF.bend) | names every law |
| [`flake.nix`](flake.nix) | Nix overlay (`rust-tops-laws`, `cargo-tops`) |
| `./scripts/bend-gate.sh` | names + `bend PROOF.bend` when bend is on PATH |

Promote a row into `RUST_TOPS.md` only with an explicit spec bump.
