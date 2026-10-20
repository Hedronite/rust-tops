#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[package]
edition = "2021"
---

//! Generate `laws/LAWS.bend`, `laws/PROOF.bend`, and `laws/NAMES` from the
//! H-01..H-53 harvest catalog.
//!
//! SoT for *names* is this catalog (must match `BEND2-INTEGRATION.md`).
//! Regenerate: `cargo +nightly -Zscript scripts/gen-laws.rs`
//! Do not weaken a law to make a proof pass.
//!
//! Native cargo-script (RFC 3502), **std only**: tables + `format!` + `fs`.
//! It is a maintainer tool: it is not a workspace member, it does not change
//! the kit `rust-version = "1.85"`, and it is never pulled by the Nix
//! `law-catalog` check (`scripts/bend-gate.sh` stays names-only POSIX).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

/// One harvest row: `(law_name, type_name, fn_name, bad_ctor, good_ctor, bad_verdict)`.
///
/// `bad_verdict` is `Refuse` (hard / soft→hard) or `Ack` (soft), exactly as the
/// catalog spells it — the literal is interpolated verbatim so a new verdict
/// spelling cannot silently drift from `BEND2-INTEGRATION.md`.
struct Row {
    law: &'static str,
    typ: &'static str,
    fun: &'static str,
    bad: &'static str,
    good: &'static str,
    bad_verdict: &'static str,
}

/// The 52 binary laws. Order is load-bearing: `LAWS.bend`, `PROOF.bend` and
/// `NAMES` all follow it, and the fuzz block is emitted immediately before
/// `roundtrip_property`.
const BINARY: &[Row] = &[
    Row { law: "evidence_not_self_served", typ: "Evidence", fun: "evidence_verdict", bad: "FromImpl", good: "FromContract", bad_verdict: "Refuse" },
    Row { law: "vector_replay_required", typ: "VectorReplay", fun: "vector_verdict", bad: "NeverReplayed", good: "Replayed", bad_verdict: "Refuse" },
    Row { law: "primitive_port_knows_its_source", typ: "PrimitivePort", fun: "port_verdict", bad: "UnnamedPort", good: "NamedPort", bad_verdict: "Refuse" },
    Row { law: "no_source_text_assertions", typ: "SourceAssert", fun: "source_assert_verdict", bad: "SourceGrep", good: "StructuralAssert", bad_verdict: "Refuse" },
    Row { law: "every_cfg_is_seen_by_ci", typ: "CfgLane", fun: "cfg_verdict", bad: "CfgUnseen", good: "CfgSeen", bad_verdict: "Refuse" },
    Row { law: "ci_matrix_matches_class", typ: "ClassJob", fun: "class_job_verdict", bad: "ClassJobMissing", good: "ClassJobPresent", bad_verdict: "Refuse" },
    Row { law: "no_sealed_class_data_in_side_channels", typ: "SideChannel", fun: "side_channel_verdict", bad: "SecretInSideChannel", good: "SecretSealed", bad_verdict: "Refuse" },
    Row { law: "no_unowned_hardening_todo", typ: "HardeningTodo", fun: "hardening_verdict", bad: "HardeningUnowned", good: "HardeningOwned", bad_verdict: "Refuse" },
    Row { law: "cap_is_named_or_it_is_a_bug", typ: "Cap", fun: "cap_verdict", bad: "MagicCap", good: "NamedCap", bad_verdict: "Refuse" },
    Row { law: "class_declared_before_edit", typ: "ClassDecl", fun: "class_decl_verdict", bad: "ClassMissing", good: "ClassDeclared", bad_verdict: "Refuse" },
    Row { law: "contract_lives_near_its_owner", typ: "ContractSite", fun: "contract_verdict", bad: "ContractFar", good: "ContractNear", bad_verdict: "Refuse" },
    Row { law: "no_write_only_state", typ: "WriteOnly", fun: "write_only_verdict", bad: "WriteOnlyField", good: "FieldRead", bad_verdict: "Ack" },
    Row { law: "artifact_asserted_by_structure", typ: "ArtifactAssert", fun: "artifact_assert_verdict", bad: "ArtifactUnguarded", good: "ArtifactStructural", bad_verdict: "Refuse" },
    Row { law: "roundtrip_property", typ: "Roundtrip", fun: "roundtrip_verdict", bad: "FixturesOnly", good: "RoundtripProperty", bad_verdict: "Refuse" },
    Row { law: "spec_schema_shipped", typ: "Schema", fun: "schema_verdict", bad: "SchemaMissing", good: "SchemaShipped", bad_verdict: "Refuse" },
    Row { law: "constant_time_tag_cmp", typ: "TagCmp", fun: "tag_cmp_verdict", bad: "TagEqCmp", good: "TagConstantTime", bad_verdict: "Refuse" },
    Row { law: "bounded_range_io", typ: "RangeIo", fun: "range_verdict", bad: "RangeUnbounded", good: "RangeBounded", bad_verdict: "Ack" },
    Row { law: "zeroize_confidential_buffers", typ: "SecretBuf", fun: "zeroize_verdict", bad: "BufferPlain", good: "BufferZeroized", bad_verdict: "Refuse" },
    Row { law: "pub_contract_doc_example", typ: "DocExample", fun: "doc_verdict", bad: "DocMissing", good: "DocExampleOrSkip", bad_verdict: "Ack" },
    Row { law: "library_public_oracle", typ: "PublicOracle", fun: "oracle_verdict", bad: "CliOnlyOracle", good: "LibraryOracle", bad_verdict: "Refuse" },
    Row { law: "fmt_is_tier0", typ: "FmtGate", fun: "fmt_verdict", bad: "FmtUngated", good: "FmtTier0", bad_verdict: "Refuse" },
    Row { law: "lock_honesty", typ: "LockState", fun: "lock_verdict", bad: "LockDrift", good: "LockHonest", bad_verdict: "Refuse" },
    Row { law: "msrv_enforced", typ: "MsrvJob", fun: "msrv_job_verdict", bad: "MsrvJobMissing", good: "MsrvJobPresent", bad_verdict: "Refuse" },
    Row { law: "clippy_toolchain_pinned", typ: "ClippyPin", fun: "clippy_pin_verdict", bad: "ClippyUnpinned", good: "ClippyPinned", bad_verdict: "Refuse" },
    Row { law: "no_pedantic_group", typ: "Pedantic", fun: "pedantic_verdict", bad: "PedanticGroup", good: "PerLint", bad_verdict: "Refuse" },
    Row { law: "fallible_integrity_root", typ: "IntegrityRoot", fun: "integrity_verdict", bad: "IntegrityUnwrap", good: "IntegrityResult", bad_verdict: "Refuse" },
    Row { law: "event_safe_emission", typ: "EventEmit", fun: "event_verdict", bad: "EventLeaksSecret", good: "EventSafe", bad_verdict: "Refuse" },
    Row { law: "apply_proves_the_gates", typ: "ApplyClaim", fun: "apply_verdict", bad: "ApplyClaimed", good: "ApplyProved", bad_verdict: "Refuse" },
    Row { law: "baseline_green_before_mutants", typ: "MutantsBaseline", fun: "baseline_verdict", bad: "BaselineSkip", good: "BaselineGreen", bad_verdict: "Refuse" },
    Row { law: "marker_sweep_is_not_a_sweep", typ: "MarkerSweep", fun: "marker_verdict", bad: "MarkerCleared", good: "BugClosed", bad_verdict: "Refuse" },
    Row { law: "security_primitive_has_a_unit_oracle", typ: "PrimitiveOracle", fun: "primitive_oracle_verdict", bad: "PrimitiveTransitive", good: "PrimitiveUnitOracle", bad_verdict: "Refuse" },
    Row { law: "cap_constants_have_boundary_tests", typ: "CapBoundary", fun: "cap_bound_verdict", bad: "CapUntested", good: "CapBoundaryTested", bad_verdict: "Refuse" },
    Row { law: "measurement_is_isolated", typ: "MeasureDir", fun: "measure_verdict", bad: "SharedTarget", good: "IsolatedTarget", bad_verdict: "Refuse" },
    Row { law: "record_only_artifacts_are_nonempty", typ: "RecordArtifact", fun: "record_verdict", bad: "RecordEmpty", good: "RecordNonempty", bad_verdict: "Refuse" },
    Row { law: "slice_completes_with_killtest", typ: "SliceComplete", fun: "slice_verdict", bad: "SliceNoTest", good: "SliceKilltest", bad_verdict: "Refuse" },
    Row { law: "missing_tool_not_green", typ: "ToolPresence", fun: "tool_verdict", bad: "ToolMissing", good: "ToolPresent", bad_verdict: "Refuse" },
    Row { law: "mutant_kill_without_scar", typ: "ScarKill", fun: "scar_verdict", bad: "ScarSplit", good: "MutantKilled", bad_verdict: "Refuse" },
    Row { law: "report_names_its_argv", typ: "NamedArgv", fun: "argv_verdict", bad: "ArgvUnnamed", good: "ArgvNamed", bad_verdict: "Refuse" },
    Row { law: "arg_group_becomes_a_name", typ: "ArgGroup", fun: "arg_group_verdict", bad: "Part2Extract", good: "DomainType", bad_verdict: "Refuse" },
    Row { law: "scaffold_runs_once", typ: "Scaffold", fun: "scaffold_verdict", bad: "ScaffoldCollide", good: "ScaffoldOnce", bad_verdict: "Refuse" },
    Row { law: "canonical_form_is_a_fixed_point", typ: "CanonicalForm", fun: "canonical_verdict", bad: "NotFixedPoint", good: "FixedPoint", bad_verdict: "Refuse" },
    Row { law: "crap_wires_coverage", typ: "CrapWire", fun: "crap_wire_verdict", bad: "CrapUnwired", good: "CrapWired", bad_verdict: "Refuse" },
    Row { law: "density_extract_has_domain_name", typ: "ExtractName", fun: "extract_verdict", bad: "ExtractArgs2", good: "ExtractDomain", bad_verdict: "Refuse" },
    Row { law: "coverage_features_match_workspace", typ: "CoverageFeat", fun: "coverage_feat_verdict", bad: "CoverageNarrow", good: "CoverageMatch", bad_verdict: "Refuse" },
    Row { law: "perf_without_runner_is_not_gate", typ: "PerfRunner", fun: "perf_verdict", bad: "PerfNoRunner", good: "PerfMeasured", bad_verdict: "Refuse" },
    Row { law: "sysdeps_before_all_features", typ: "Sysdeps", fun: "sysdeps_verdict", bad: "SysdepsMissing", good: "SysdepsInstalled", bad_verdict: "Refuse" },
    Row { law: "every_cfg_target_has_a_lane", typ: "CfgTarget", fun: "cfg_target_verdict", bad: "LinuxOnlyCfg", good: "EveryTargetLane", bad_verdict: "Refuse" },
    Row { law: "msrv_is_lock_max_including_target_gated", typ: "MsrvLock", fun: "msrv_lock_verdict", bad: "MsrvNotLockMax", good: "MsrvLockMax", bad_verdict: "Refuse" },
    Row { law: "artifact_check_is_its_own_step", typ: "ArtifactStep", fun: "artifact_step_verdict", bad: "ArtifactFolded", good: "ArtifactOwnStep", bad_verdict: "Refuse" },
    Row { law: "job_declares_merge_semantics", typ: "MergeSemantics", fun: "merge_verdict", bad: "MergeSemanticsMixed", good: "MergeSemanticsDeclared", bad_verdict: "Refuse" },
    Row { law: "drop_in_is_a_floor_not_a_ceiling", typ: "DropInRole", fun: "drop_in_verdict", bad: "DropInCeiling", good: "DropInFloor", bad_verdict: "Refuse" },
    Row { law: "single_deny_site", typ: "DenySite", fun: "deny_verdict", bad: "DenyDouble", good: "DenySingle", bad_verdict: "Refuse" },
];

/// The `fuzz_untrusted_ingress` block, emitted before `roundtrip_property`.
/// Verbatim from the catalog generator: it is the one law whose verifier is a
/// 2-argument match, so it does not fit [`emit_binary_block`].
const FUZZ_BLOCK: &str = "type CrateClass is Data:\n  ParserCodec{}\n  OtherClass{}\n\ntype Ingress is Data:\n  UntrustedNoFuzz{}\n  UntrustedFuzzed{}\n  Trusted{}\n\ndef fuzz_verdict(class: CrateClass, ingress: Ingress) -> Verdict:\n  match class:\n    case ParserCodec{}:\n      match ingress:\n        case UntrustedNoFuzz{}:\n          Refuse{}\n        case UntrustedFuzzed{}:\n          Allow{}\n        case Trusted{}:\n          Allow{}\n    case OtherClass{}:\n      match ingress:\n        case UntrustedNoFuzz{}:\n          Allow{}\n        case UntrustedFuzzed{}:\n          Allow{}\n        case Trusted{}:\n          Allow{}\n\nlaw fuzz_untrusted_ingress:\n  {fuzz_verdict(ParserCodec{}, UntrustedNoFuzz{}) == Refuse{} : Verdict}\n\ndef fuzz_untrusted_ingress():\n  {==}\n";

/// `law fuzz_untrusted_ingress` is inserted into `NAMES` after
/// `artifact_asserted_by_structure` (see [`main`]).
const FUZZ_LAW: &str = "fuzz_untrusted_ingress";
/// The insert anchor, and the expected total after insertion.
const ANCHOR: &str = "artifact_asserted_by_structure";
const EXPECTED_LAWS: usize = 53;

/// One `type` + `match` + `law` + `def` quadruple.
fn emit_binary_block(row: &Row) -> String {
    format!(
        "type {typ} is Data:\n  {bad}{{}}\n  {good}{{}}\n\ndef {fun}(x: {typ}) -> Verdict:\n  match x:\n    case {bad}{{}}:\n      {verdict}{{}}\n    case {good}{{}}:\n      Allow{{}}\n\nlaw {law}:\n  {{{fun}({bad}{{}}) == {verdict}{{}} : Verdict}}\n\ndef {law}():\n  {{==}}\n",
        typ = row.typ,
        bad = row.bad,
        good = row.good,
        fun = row.fun,
        verdict = row.bad_verdict,
        law = row.law,
    )
}

/// `a & (b & (c & …))` — right-nested, starting from the last item.
///
/// Mirrors the catalog fold exactly: the first prop is the leftmost term and
/// each earlier item wraps everything to its right.
fn fold_product(items: &[String]) -> String {
    let mut acc = items[items.len() - 1].clone();
    for item in items[..items.len() - 1].iter().rev() {
        acc = format!("{item} & {acc}");
    }
    acc
}

/// `(a, (b, (c, …)))` — same shape as [`fold_product`], for the call tuple.
fn fold_tuple(items: &[String]) -> String {
    let mut acc = items[items.len() - 1].clone();
    for item in items[..items.len() - 1].iter().rev() {
        acc = format!("({item}, {acc})");
    }
    acc
}

/// Where `laws/` lives: the parent of the directory holding this file.
///
/// The Python generator used `Path(__file__).resolve().parents[1]`. Under
/// cargo-script (measured on citadel 1.100.0-nightly) `file!()` is only the
/// **basename** and `std::env::current_exe()` points into the build cache, but
/// `CARGO_MANIFEST_DIR` is the directory holding the script — so its parent is
/// the workspace root. Canonicalized to match Python's `.resolve()`.
fn laws_dir() -> PathBuf {
    let script_dir = match option_env!("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => {
            let arg = std::env::args().next().unwrap_or_default();
            let exe = PathBuf::from(arg);
            exe.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        }
    };
    let script_dir = fs::canonicalize(&script_dir).unwrap_or(script_dir);
    script_dir
        .parent()
        .unwrap_or(&script_dir)
        .join("laws")
}

fn write_or_exit(path: &Path, body: &str) {
    if let Err(e) = fs::write(path, body) {
        eprintln!("write {}: {e}", path.display());
        exit(1);
    }
}

fn main() {
    let laws_dir = laws_dir();

    // Constructor collision check: every `type`'s variants must be unique
    // across the whole pack, or a `match` in one law would capture another's.
    let mut ctors: Vec<&str> = Vec::new();
    for row in BINARY {
        ctors.push(row.bad);
        ctors.push(row.good);
    }
    ctors.extend([
        "ParserCodec",
        "OtherClass",
        "UntrustedNoFuzz",
        "UntrustedFuzzed",
        "Trusted",
    ]);
    let mut seen: HashSet<&str> = HashSet::with_capacity(ctors.len());
    for ctor in &ctors {
        if !seen.insert(ctor) {
            eprintln!("constructor collision: {ctors:?}");
            exit(1);
        }
    }

    let mut law_names: Vec<&str> = BINARY.iter().map(|row| row.law).collect();
    let anchor = law_names.iter().position(|law| *law == ANCHOR).unwrap_or_else(|| {
        eprintln!("catalog is missing {ANCHOR}");
        exit(1);
    });
    law_names.insert(anchor + 1, FUZZ_LAW);
    if law_names.len() != EXPECTED_LAWS {
        eprintln!("expected {EXPECTED_LAWS} laws, got {}", law_names.len());
        exit(1);
    }

    let mut parts: Vec<String> = vec![
        "import Base\n".to_string(),
        "type Verdict is Data:\n  Refuse{}\n  Ack{}\n  Allow{}\n".to_string(),
    ];
    for row in BINARY {
        if row.law == "roundtrip_property" {
            parts.push(FUZZ_BLOCK.to_string());
        }
        parts.push(emit_binary_block(row));
    }

    if let Err(e) = fs::create_dir_all(&laws_dir) {
        eprintln!("mkdir {}: {e}", laws_dir.display());
        exit(1);
    }
    let laws_body = format!("{}\n", parts.join("\n").trim_end());
    write_or_exit(&laws_dir.join("LAWS.bend"), &laws_body);

    // PROOF: one property per law, and the calls as one right-nested tuple.
    let mut props: Vec<String> = Vec::new();
    let mut calls: Vec<String> = Vec::new();
    for row in BINARY {
        if row.law == "roundtrip_property" {
            props.push(
                "{Laws.fuzz_verdict(Laws.ParserCodec{}, Laws.UntrustedNoFuzz{}) == Laws.Refuse{} : Laws.Verdict}"
                    .to_string(),
            );
            calls.push("Laws.fuzz_untrusted_ingress()".to_string());
        }
        props.push(format!(
            "{{Laws.{}(Laws.{}{{}}) == Laws.{}{{}} : Laws.Verdict}}",
            row.fun, row.bad, row.bad_verdict
        ));
        calls.push(format!("Laws.{}()", row.law));
    }
    if props.len() != EXPECTED_LAWS || calls.len() != EXPECTED_LAWS {
        eprintln!("proof size {} {}", props.len(), calls.len());
        exit(1);
    }

    let proof = format!(
        "import ./LAWS.bend as Laws\n\ndef main() -> {}:\n  {}\n",
        fold_product(&props),
        fold_tuple(&calls)
    );
    write_or_exit(&laws_dir.join("PROOF.bend"), &proof);

    let names = format!("{}\n", law_names.join("\n"));
    write_or_exit(&laws_dir.join("NAMES"), &names);

    println!("wrote {} laws -> {}", law_names.len(), laws_dir.display());
}
