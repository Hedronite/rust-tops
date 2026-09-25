#!/usr/bin/env python3
"""Generate laws/LAWS.bend and laws/PROOF.bend from the H-01..H-53 harvest.

SoT for *names* is this catalog (must match BEND2-INTEGRATION.md).
Regenerate: python3 scripts/gen-laws.py
Do not weaken a law to make a proof pass.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LAWS_DIR = ROOT / "laws"

# (law_name, type_name, fn_name, bad_ctor, good_ctor, bad_verdict)
# bad_verdict: Refuse (hard / soft→hard) or Ack (soft)
BINARY = [
    ("evidence_not_self_served", "Evidence", "evidence_verdict", "FromImpl", "FromContract", "Refuse"),
    ("vector_replay_required", "VectorReplay", "vector_verdict", "NeverReplayed", "Replayed", "Refuse"),
    ("primitive_port_knows_its_source", "PrimitivePort", "port_verdict", "UnnamedPort", "NamedPort", "Refuse"),
    ("no_source_text_assertions", "SourceAssert", "source_assert_verdict", "SourceGrep", "StructuralAssert", "Refuse"),
    ("every_cfg_is_seen_by_ci", "CfgLane", "cfg_verdict", "CfgUnseen", "CfgSeen", "Refuse"),
    ("ci_matrix_matches_class", "ClassJob", "class_job_verdict", "ClassJobMissing", "ClassJobPresent", "Refuse"),
    ("no_sealed_class_data_in_side_channels", "SideChannel", "side_channel_verdict", "SecretInSideChannel", "SecretSealed", "Refuse"),
    ("no_unowned_hardening_todo", "HardeningTodo", "hardening_verdict", "HardeningUnowned", "HardeningOwned", "Refuse"),
    ("cap_is_named_or_it_is_a_bug", "Cap", "cap_verdict", "MagicCap", "NamedCap", "Refuse"),
    ("class_declared_before_edit", "ClassDecl", "class_decl_verdict", "ClassMissing", "ClassDeclared", "Refuse"),
    ("contract_lives_near_its_owner", "ContractSite", "contract_verdict", "ContractFar", "ContractNear", "Refuse"),
    ("no_write_only_state", "WriteOnly", "write_only_verdict", "WriteOnlyField", "FieldRead", "Ack"),
    ("artifact_asserted_by_structure", "ArtifactAssert", "artifact_assert_verdict", "ArtifactUnguarded", "ArtifactStructural", "Refuse"),
    ("roundtrip_property", "Roundtrip", "roundtrip_verdict", "FixturesOnly", "RoundtripProperty", "Refuse"),
    ("spec_schema_shipped", "Schema", "schema_verdict", "SchemaMissing", "SchemaShipped", "Refuse"),
    ("constant_time_tag_cmp", "TagCmp", "tag_cmp_verdict", "TagEqCmp", "TagConstantTime", "Refuse"),
    ("bounded_range_io", "RangeIo", "range_verdict", "RangeUnbounded", "RangeBounded", "Ack"),
    ("zeroize_confidential_buffers", "SecretBuf", "zeroize_verdict", "BufferPlain", "BufferZeroized", "Refuse"),
    ("pub_contract_doc_example", "DocExample", "doc_verdict", "DocMissing", "DocExampleOrSkip", "Ack"),
    ("library_public_oracle", "PublicOracle", "oracle_verdict", "CliOnlyOracle", "LibraryOracle", "Refuse"),
    ("fmt_is_tier0", "FmtGate", "fmt_verdict", "FmtUngated", "FmtTier0", "Refuse"),
    ("lock_honesty", "LockState", "lock_verdict", "LockDrift", "LockHonest", "Refuse"),
    ("msrv_enforced", "MsrvJob", "msrv_job_verdict", "MsrvJobMissing", "MsrvJobPresent", "Refuse"),
    ("clippy_toolchain_pinned", "ClippyPin", "clippy_pin_verdict", "ClippyUnpinned", "ClippyPinned", "Refuse"),
    ("no_pedantic_group", "Pedantic", "pedantic_verdict", "PedanticGroup", "PerLint", "Refuse"),
    ("fallible_integrity_root", "IntegrityRoot", "integrity_verdict", "IntegrityUnwrap", "IntegrityResult", "Refuse"),
    ("event_safe_emission", "EventEmit", "event_verdict", "EventLeaksSecret", "EventSafe", "Refuse"),
    ("apply_proves_the_gates", "ApplyClaim", "apply_verdict", "ApplyClaimed", "ApplyProved", "Refuse"),
    ("baseline_green_before_mutants", "MutantsBaseline", "baseline_verdict", "BaselineSkip", "BaselineGreen", "Refuse"),
    ("marker_sweep_is_not_a_sweep", "MarkerSweep", "marker_verdict", "MarkerCleared", "BugClosed", "Refuse"),
    ("security_primitive_has_a_unit_oracle", "PrimitiveOracle", "primitive_oracle_verdict", "PrimitiveTransitive", "PrimitiveUnitOracle", "Refuse"),
    ("cap_constants_have_boundary_tests", "CapBoundary", "cap_bound_verdict", "CapUntested", "CapBoundaryTested", "Refuse"),
    ("measurement_is_isolated", "MeasureDir", "measure_verdict", "SharedTarget", "IsolatedTarget", "Refuse"),
    ("record_only_artifacts_are_nonempty", "RecordArtifact", "record_verdict", "RecordEmpty", "RecordNonempty", "Refuse"),
    ("slice_completes_with_killtest", "SliceComplete", "slice_verdict", "SliceNoTest", "SliceKilltest", "Refuse"),
    ("missing_tool_not_green", "ToolPresence", "tool_verdict", "ToolMissing", "ToolPresent", "Refuse"),
    ("mutant_kill_without_scar", "ScarKill", "scar_verdict", "ScarSplit", "MutantKilled", "Refuse"),
    ("report_names_its_argv", "NamedArgv", "argv_verdict", "ArgvUnnamed", "ArgvNamed", "Refuse"),
    ("arg_group_becomes_a_name", "ArgGroup", "arg_group_verdict", "Part2Extract", "DomainType", "Refuse"),
    ("scaffold_runs_once", "Scaffold", "scaffold_verdict", "ScaffoldCollide", "ScaffoldOnce", "Refuse"),
    ("canonical_form_is_a_fixed_point", "CanonicalForm", "canonical_verdict", "NotFixedPoint", "FixedPoint", "Refuse"),
    ("crap_wires_coverage", "CrapWire", "crap_wire_verdict", "CrapUnwired", "CrapWired", "Refuse"),
    ("density_extract_has_domain_name", "ExtractName", "extract_verdict", "ExtractArgs2", "ExtractDomain", "Refuse"),
    ("coverage_features_match_workspace", "CoverageFeat", "coverage_feat_verdict", "CoverageNarrow", "CoverageMatch", "Refuse"),
    ("perf_without_runner_is_not_gate", "PerfRunner", "perf_verdict", "PerfNoRunner", "PerfMeasured", "Refuse"),
    ("sysdeps_before_all_features", "Sysdeps", "sysdeps_verdict", "SysdepsMissing", "SysdepsInstalled", "Refuse"),
    ("every_cfg_target_has_a_lane", "CfgTarget", "cfg_target_verdict", "LinuxOnlyCfg", "EveryTargetLane", "Refuse"),
    ("msrv_is_lock_max_including_target_gated", "MsrvLock", "msrv_lock_verdict", "MsrvNotLockMax", "MsrvLockMax", "Refuse"),
    ("artifact_check_is_its_own_step", "ArtifactStep", "artifact_step_verdict", "ArtifactFolded", "ArtifactOwnStep", "Refuse"),
    ("job_declares_merge_semantics", "MergeSemantics", "merge_verdict", "MergeSemanticsMixed", "MergeSemanticsDeclared", "Refuse"),
    ("drop_in_is_a_floor_not_a_ceiling", "DropInRole", "drop_in_verdict", "DropInCeiling", "DropInFloor", "Refuse"),
    ("single_deny_site", "DenySite", "deny_verdict", "DenyDouble", "DenySingle", "Refuse"),
]


def emit_binary_block(law, typ, fn, bad, good, verdict) -> str:
    return f"""type {typ} is Data:
  {bad}{{}}
  {good}{{}}

def {fn}(x: {typ}) -> Verdict:
  match x:
    case {bad}{{}}:
      {verdict}{{}}
    case {good}{{}}:
      Allow{{}}

law {law}:
  {{{fn}({bad}{{}}) == {verdict}{{}} : Verdict}}

def {law}():
  {{==}}
"""


def emit_fuzz_block() -> str:
    return """type CrateClass is Data:
  ParserCodec{}
  OtherClass{}

type Ingress is Data:
  UntrustedNoFuzz{}
  UntrustedFuzzed{}
  Trusted{}

def fuzz_verdict(class: CrateClass, ingress: Ingress) -> Verdict:
  match class:
    case ParserCodec{}:
      match ingress:
        case UntrustedNoFuzz{}:
          Refuse{}
        case UntrustedFuzzed{}:
          Allow{}
        case Trusted{}:
          Allow{}
    case OtherClass{}:
      match ingress:
        case UntrustedNoFuzz{}:
          Allow{}
        case UntrustedFuzzed{}:
          Allow{}
        case Trusted{}:
          Allow{}

law fuzz_untrusted_ingress:
  {fuzz_verdict(ParserCodec{}, UntrustedNoFuzz{}) == Refuse{} : Verdict}

def fuzz_untrusted_ingress():
  {==}
"""


def fold_product(items):
    acc = items[-1]
    for item in reversed(items[:-1]):
        acc = f"{item} & {acc}"
    return acc


def fold_tuple(items):
    acc = items[-1]
    for item in reversed(items[:-1]):
        acc = f"({item}, {acc})"
    return acc


def main() -> None:
    ctors = []
    for _law, _typ, _fn, bad, good, _v in BINARY:
        ctors.extend([bad, good])
    ctors.extend(["ParserCodec", "OtherClass", "UntrustedNoFuzz", "UntrustedFuzzed", "Trusted"])
    if len(ctors) != len(set(ctors)):
        raise SystemExit(f"constructor collision: {ctors}")

    law_names = [row[0] for row in BINARY]
    insert_at = law_names.index("artifact_asserted_by_structure") + 1
    law_names.insert(insert_at, "fuzz_untrusted_ingress")
    if len(law_names) != 53:
        raise SystemExit(f"expected 53 laws, got {len(law_names)}")

    parts = [
        "import Base\n",
        "type Verdict is Data:\n  Refuse{}\n  Ack{}\n  Allow{}\n",
    ]
    for row in BINARY:
        if row[0] == "roundtrip_property":
            parts.append(emit_fuzz_block())
        parts.append(emit_binary_block(*row))

    LAWS_DIR.mkdir(parents=True, exist_ok=True)
    (LAWS_DIR / "LAWS.bend").write_text("\n".join(parts).rstrip() + "\n")

    props = []
    calls = []
    for law, _typ, fn, bad, _good, verdict in BINARY:
        if law == "roundtrip_property":
            props.append(
                "{Laws.fuzz_verdict(Laws.ParserCodec{}, Laws.UntrustedNoFuzz{}) == Laws.Refuse{} : Laws.Verdict}"
            )
            calls.append("Laws.fuzz_untrusted_ingress()")
        props.append(f"{{Laws.{fn}(Laws.{bad}{{}}) == Laws.{verdict}{{}} : Laws.Verdict}}")
        calls.append(f"Laws.{law}()")

    if len(props) != 53 or len(calls) != 53:
        raise SystemExit(f"proof size {len(props)} {len(calls)}")

    proof = (
        "import ./LAWS.bend as Laws\n\n"
        f"def main() -> {fold_product(props)}:\n"
        f"  {fold_tuple(calls)}\n"
    )
    (LAWS_DIR / "PROOF.bend").write_text(proof)
    (LAWS_DIR / "NAMES").write_text("\n".join(law_names) + "\n")
    print(f"wrote {len(law_names)} laws -> {LAWS_DIR}")


if __name__ == "__main__":
    main()
