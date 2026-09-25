import Base

type Verdict is Data:
  Refuse{}
  Ack{}
  Allow{}

type Evidence is Data:
  FromImpl{}
  FromContract{}

def evidence_verdict(x: Evidence) -> Verdict:
  match x:
    case FromImpl{}:
      Refuse{}
    case FromContract{}:
      Allow{}

law evidence_not_self_served:
  {evidence_verdict(FromImpl{}) == Refuse{} : Verdict}

def evidence_not_self_served():
  {==}

type VectorReplay is Data:
  NeverReplayed{}
  Replayed{}

def vector_verdict(x: VectorReplay) -> Verdict:
  match x:
    case NeverReplayed{}:
      Refuse{}
    case Replayed{}:
      Allow{}

law vector_replay_required:
  {vector_verdict(NeverReplayed{}) == Refuse{} : Verdict}

def vector_replay_required():
  {==}

type PrimitivePort is Data:
  UnnamedPort{}
  NamedPort{}

def port_verdict(x: PrimitivePort) -> Verdict:
  match x:
    case UnnamedPort{}:
      Refuse{}
    case NamedPort{}:
      Allow{}

law primitive_port_knows_its_source:
  {port_verdict(UnnamedPort{}) == Refuse{} : Verdict}

def primitive_port_knows_its_source():
  {==}

type SourceAssert is Data:
  SourceGrep{}
  StructuralAssert{}

def source_assert_verdict(x: SourceAssert) -> Verdict:
  match x:
    case SourceGrep{}:
      Refuse{}
    case StructuralAssert{}:
      Allow{}

law no_source_text_assertions:
  {source_assert_verdict(SourceGrep{}) == Refuse{} : Verdict}

def no_source_text_assertions():
  {==}

type CfgLane is Data:
  CfgUnseen{}
  CfgSeen{}

def cfg_verdict(x: CfgLane) -> Verdict:
  match x:
    case CfgUnseen{}:
      Refuse{}
    case CfgSeen{}:
      Allow{}

law every_cfg_is_seen_by_ci:
  {cfg_verdict(CfgUnseen{}) == Refuse{} : Verdict}

def every_cfg_is_seen_by_ci():
  {==}

type ClassJob is Data:
  ClassJobMissing{}
  ClassJobPresent{}

def class_job_verdict(x: ClassJob) -> Verdict:
  match x:
    case ClassJobMissing{}:
      Refuse{}
    case ClassJobPresent{}:
      Allow{}

law ci_matrix_matches_class:
  {class_job_verdict(ClassJobMissing{}) == Refuse{} : Verdict}

def ci_matrix_matches_class():
  {==}

type SideChannel is Data:
  SecretInSideChannel{}
  SecretSealed{}

def side_channel_verdict(x: SideChannel) -> Verdict:
  match x:
    case SecretInSideChannel{}:
      Refuse{}
    case SecretSealed{}:
      Allow{}

law no_sealed_class_data_in_side_channels:
  {side_channel_verdict(SecretInSideChannel{}) == Refuse{} : Verdict}

def no_sealed_class_data_in_side_channels():
  {==}

type HardeningTodo is Data:
  HardeningUnowned{}
  HardeningOwned{}

def hardening_verdict(x: HardeningTodo) -> Verdict:
  match x:
    case HardeningUnowned{}:
      Refuse{}
    case HardeningOwned{}:
      Allow{}

law no_unowned_hardening_todo:
  {hardening_verdict(HardeningUnowned{}) == Refuse{} : Verdict}

def no_unowned_hardening_todo():
  {==}

type Cap is Data:
  MagicCap{}
  NamedCap{}

def cap_verdict(x: Cap) -> Verdict:
  match x:
    case MagicCap{}:
      Refuse{}
    case NamedCap{}:
      Allow{}

law cap_is_named_or_it_is_a_bug:
  {cap_verdict(MagicCap{}) == Refuse{} : Verdict}

def cap_is_named_or_it_is_a_bug():
  {==}

type ClassDecl is Data:
  ClassMissing{}
  ClassDeclared{}

def class_decl_verdict(x: ClassDecl) -> Verdict:
  match x:
    case ClassMissing{}:
      Refuse{}
    case ClassDeclared{}:
      Allow{}

law class_declared_before_edit:
  {class_decl_verdict(ClassMissing{}) == Refuse{} : Verdict}

def class_declared_before_edit():
  {==}

type ContractSite is Data:
  ContractFar{}
  ContractNear{}

def contract_verdict(x: ContractSite) -> Verdict:
  match x:
    case ContractFar{}:
      Refuse{}
    case ContractNear{}:
      Allow{}

law contract_lives_near_its_owner:
  {contract_verdict(ContractFar{}) == Refuse{} : Verdict}

def contract_lives_near_its_owner():
  {==}

type WriteOnly is Data:
  WriteOnlyField{}
  FieldRead{}

def write_only_verdict(x: WriteOnly) -> Verdict:
  match x:
    case WriteOnlyField{}:
      Ack{}
    case FieldRead{}:
      Allow{}

law no_write_only_state:
  {write_only_verdict(WriteOnlyField{}) == Ack{} : Verdict}

def no_write_only_state():
  {==}

type ArtifactAssert is Data:
  ArtifactUnguarded{}
  ArtifactStructural{}

def artifact_assert_verdict(x: ArtifactAssert) -> Verdict:
  match x:
    case ArtifactUnguarded{}:
      Refuse{}
    case ArtifactStructural{}:
      Allow{}

law artifact_asserted_by_structure:
  {artifact_assert_verdict(ArtifactUnguarded{}) == Refuse{} : Verdict}

def artifact_asserted_by_structure():
  {==}

type CrateClass is Data:
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

type Roundtrip is Data:
  FixturesOnly{}
  RoundtripProperty{}

def roundtrip_verdict(x: Roundtrip) -> Verdict:
  match x:
    case FixturesOnly{}:
      Refuse{}
    case RoundtripProperty{}:
      Allow{}

law roundtrip_property:
  {roundtrip_verdict(FixturesOnly{}) == Refuse{} : Verdict}

def roundtrip_property():
  {==}

type Schema is Data:
  SchemaMissing{}
  SchemaShipped{}

def schema_verdict(x: Schema) -> Verdict:
  match x:
    case SchemaMissing{}:
      Refuse{}
    case SchemaShipped{}:
      Allow{}

law spec_schema_shipped:
  {schema_verdict(SchemaMissing{}) == Refuse{} : Verdict}

def spec_schema_shipped():
  {==}

type TagCmp is Data:
  TagEqCmp{}
  TagConstantTime{}

def tag_cmp_verdict(x: TagCmp) -> Verdict:
  match x:
    case TagEqCmp{}:
      Refuse{}
    case TagConstantTime{}:
      Allow{}

law constant_time_tag_cmp:
  {tag_cmp_verdict(TagEqCmp{}) == Refuse{} : Verdict}

def constant_time_tag_cmp():
  {==}

type RangeIo is Data:
  RangeUnbounded{}
  RangeBounded{}

def range_verdict(x: RangeIo) -> Verdict:
  match x:
    case RangeUnbounded{}:
      Ack{}
    case RangeBounded{}:
      Allow{}

law bounded_range_io:
  {range_verdict(RangeUnbounded{}) == Ack{} : Verdict}

def bounded_range_io():
  {==}

type SecretBuf is Data:
  BufferPlain{}
  BufferZeroized{}

def zeroize_verdict(x: SecretBuf) -> Verdict:
  match x:
    case BufferPlain{}:
      Refuse{}
    case BufferZeroized{}:
      Allow{}

law zeroize_confidential_buffers:
  {zeroize_verdict(BufferPlain{}) == Refuse{} : Verdict}

def zeroize_confidential_buffers():
  {==}

type DocExample is Data:
  DocMissing{}
  DocExampleOrSkip{}

def doc_verdict(x: DocExample) -> Verdict:
  match x:
    case DocMissing{}:
      Ack{}
    case DocExampleOrSkip{}:
      Allow{}

law pub_contract_doc_example:
  {doc_verdict(DocMissing{}) == Ack{} : Verdict}

def pub_contract_doc_example():
  {==}

type PublicOracle is Data:
  CliOnlyOracle{}
  LibraryOracle{}

def oracle_verdict(x: PublicOracle) -> Verdict:
  match x:
    case CliOnlyOracle{}:
      Refuse{}
    case LibraryOracle{}:
      Allow{}

law library_public_oracle:
  {oracle_verdict(CliOnlyOracle{}) == Refuse{} : Verdict}

def library_public_oracle():
  {==}

type FmtGate is Data:
  FmtUngated{}
  FmtTier0{}

def fmt_verdict(x: FmtGate) -> Verdict:
  match x:
    case FmtUngated{}:
      Refuse{}
    case FmtTier0{}:
      Allow{}

law fmt_is_tier0:
  {fmt_verdict(FmtUngated{}) == Refuse{} : Verdict}

def fmt_is_tier0():
  {==}

type LockState is Data:
  LockDrift{}
  LockHonest{}

def lock_verdict(x: LockState) -> Verdict:
  match x:
    case LockDrift{}:
      Refuse{}
    case LockHonest{}:
      Allow{}

law lock_honesty:
  {lock_verdict(LockDrift{}) == Refuse{} : Verdict}

def lock_honesty():
  {==}

type MsrvJob is Data:
  MsrvJobMissing{}
  MsrvJobPresent{}

def msrv_job_verdict(x: MsrvJob) -> Verdict:
  match x:
    case MsrvJobMissing{}:
      Refuse{}
    case MsrvJobPresent{}:
      Allow{}

law msrv_enforced:
  {msrv_job_verdict(MsrvJobMissing{}) == Refuse{} : Verdict}

def msrv_enforced():
  {==}

type ClippyPin is Data:
  ClippyUnpinned{}
  ClippyPinned{}

def clippy_pin_verdict(x: ClippyPin) -> Verdict:
  match x:
    case ClippyUnpinned{}:
      Refuse{}
    case ClippyPinned{}:
      Allow{}

law clippy_toolchain_pinned:
  {clippy_pin_verdict(ClippyUnpinned{}) == Refuse{} : Verdict}

def clippy_toolchain_pinned():
  {==}

type Pedantic is Data:
  PedanticGroup{}
  PerLint{}

def pedantic_verdict(x: Pedantic) -> Verdict:
  match x:
    case PedanticGroup{}:
      Refuse{}
    case PerLint{}:
      Allow{}

law no_pedantic_group:
  {pedantic_verdict(PedanticGroup{}) == Refuse{} : Verdict}

def no_pedantic_group():
  {==}

type IntegrityRoot is Data:
  IntegrityUnwrap{}
  IntegrityResult{}

def integrity_verdict(x: IntegrityRoot) -> Verdict:
  match x:
    case IntegrityUnwrap{}:
      Refuse{}
    case IntegrityResult{}:
      Allow{}

law fallible_integrity_root:
  {integrity_verdict(IntegrityUnwrap{}) == Refuse{} : Verdict}

def fallible_integrity_root():
  {==}

type EventEmit is Data:
  EventLeaksSecret{}
  EventSafe{}

def event_verdict(x: EventEmit) -> Verdict:
  match x:
    case EventLeaksSecret{}:
      Refuse{}
    case EventSafe{}:
      Allow{}

law event_safe_emission:
  {event_verdict(EventLeaksSecret{}) == Refuse{} : Verdict}

def event_safe_emission():
  {==}

type ApplyClaim is Data:
  ApplyClaimed{}
  ApplyProved{}

def apply_verdict(x: ApplyClaim) -> Verdict:
  match x:
    case ApplyClaimed{}:
      Refuse{}
    case ApplyProved{}:
      Allow{}

law apply_proves_the_gates:
  {apply_verdict(ApplyClaimed{}) == Refuse{} : Verdict}

def apply_proves_the_gates():
  {==}

type MutantsBaseline is Data:
  BaselineSkip{}
  BaselineGreen{}

def baseline_verdict(x: MutantsBaseline) -> Verdict:
  match x:
    case BaselineSkip{}:
      Refuse{}
    case BaselineGreen{}:
      Allow{}

law baseline_green_before_mutants:
  {baseline_verdict(BaselineSkip{}) == Refuse{} : Verdict}

def baseline_green_before_mutants():
  {==}

type MarkerSweep is Data:
  MarkerCleared{}
  BugClosed{}

def marker_verdict(x: MarkerSweep) -> Verdict:
  match x:
    case MarkerCleared{}:
      Refuse{}
    case BugClosed{}:
      Allow{}

law marker_sweep_is_not_a_sweep:
  {marker_verdict(MarkerCleared{}) == Refuse{} : Verdict}

def marker_sweep_is_not_a_sweep():
  {==}

type PrimitiveOracle is Data:
  PrimitiveTransitive{}
  PrimitiveUnitOracle{}

def primitive_oracle_verdict(x: PrimitiveOracle) -> Verdict:
  match x:
    case PrimitiveTransitive{}:
      Refuse{}
    case PrimitiveUnitOracle{}:
      Allow{}

law security_primitive_has_a_unit_oracle:
  {primitive_oracle_verdict(PrimitiveTransitive{}) == Refuse{} : Verdict}

def security_primitive_has_a_unit_oracle():
  {==}

type CapBoundary is Data:
  CapUntested{}
  CapBoundaryTested{}

def cap_bound_verdict(x: CapBoundary) -> Verdict:
  match x:
    case CapUntested{}:
      Refuse{}
    case CapBoundaryTested{}:
      Allow{}

law cap_constants_have_boundary_tests:
  {cap_bound_verdict(CapUntested{}) == Refuse{} : Verdict}

def cap_constants_have_boundary_tests():
  {==}

type MeasureDir is Data:
  SharedTarget{}
  IsolatedTarget{}

def measure_verdict(x: MeasureDir) -> Verdict:
  match x:
    case SharedTarget{}:
      Refuse{}
    case IsolatedTarget{}:
      Allow{}

law measurement_is_isolated:
  {measure_verdict(SharedTarget{}) == Refuse{} : Verdict}

def measurement_is_isolated():
  {==}

type RecordArtifact is Data:
  RecordEmpty{}
  RecordNonempty{}

def record_verdict(x: RecordArtifact) -> Verdict:
  match x:
    case RecordEmpty{}:
      Refuse{}
    case RecordNonempty{}:
      Allow{}

law record_only_artifacts_are_nonempty:
  {record_verdict(RecordEmpty{}) == Refuse{} : Verdict}

def record_only_artifacts_are_nonempty():
  {==}

type SliceComplete is Data:
  SliceNoTest{}
  SliceKilltest{}

def slice_verdict(x: SliceComplete) -> Verdict:
  match x:
    case SliceNoTest{}:
      Refuse{}
    case SliceKilltest{}:
      Allow{}

law slice_completes_with_killtest:
  {slice_verdict(SliceNoTest{}) == Refuse{} : Verdict}

def slice_completes_with_killtest():
  {==}

type ToolPresence is Data:
  ToolMissing{}
  ToolPresent{}

def tool_verdict(x: ToolPresence) -> Verdict:
  match x:
    case ToolMissing{}:
      Refuse{}
    case ToolPresent{}:
      Allow{}

law missing_tool_not_green:
  {tool_verdict(ToolMissing{}) == Refuse{} : Verdict}

def missing_tool_not_green():
  {==}

type ScarKill is Data:
  ScarSplit{}
  MutantKilled{}

def scar_verdict(x: ScarKill) -> Verdict:
  match x:
    case ScarSplit{}:
      Refuse{}
    case MutantKilled{}:
      Allow{}

law mutant_kill_without_scar:
  {scar_verdict(ScarSplit{}) == Refuse{} : Verdict}

def mutant_kill_without_scar():
  {==}

type NamedArgv is Data:
  ArgvUnnamed{}
  ArgvNamed{}

def argv_verdict(x: NamedArgv) -> Verdict:
  match x:
    case ArgvUnnamed{}:
      Refuse{}
    case ArgvNamed{}:
      Allow{}

law report_names_its_argv:
  {argv_verdict(ArgvUnnamed{}) == Refuse{} : Verdict}

def report_names_its_argv():
  {==}

type ArgGroup is Data:
  Part2Extract{}
  DomainType{}

def arg_group_verdict(x: ArgGroup) -> Verdict:
  match x:
    case Part2Extract{}:
      Refuse{}
    case DomainType{}:
      Allow{}

law arg_group_becomes_a_name:
  {arg_group_verdict(Part2Extract{}) == Refuse{} : Verdict}

def arg_group_becomes_a_name():
  {==}

type Scaffold is Data:
  ScaffoldCollide{}
  ScaffoldOnce{}

def scaffold_verdict(x: Scaffold) -> Verdict:
  match x:
    case ScaffoldCollide{}:
      Refuse{}
    case ScaffoldOnce{}:
      Allow{}

law scaffold_runs_once:
  {scaffold_verdict(ScaffoldCollide{}) == Refuse{} : Verdict}

def scaffold_runs_once():
  {==}

type CanonicalForm is Data:
  NotFixedPoint{}
  FixedPoint{}

def canonical_verdict(x: CanonicalForm) -> Verdict:
  match x:
    case NotFixedPoint{}:
      Refuse{}
    case FixedPoint{}:
      Allow{}

law canonical_form_is_a_fixed_point:
  {canonical_verdict(NotFixedPoint{}) == Refuse{} : Verdict}

def canonical_form_is_a_fixed_point():
  {==}

type CrapWire is Data:
  CrapUnwired{}
  CrapWired{}

def crap_wire_verdict(x: CrapWire) -> Verdict:
  match x:
    case CrapUnwired{}:
      Refuse{}
    case CrapWired{}:
      Allow{}

law crap_wires_coverage:
  {crap_wire_verdict(CrapUnwired{}) == Refuse{} : Verdict}

def crap_wires_coverage():
  {==}

type ExtractName is Data:
  ExtractArgs2{}
  ExtractDomain{}

def extract_verdict(x: ExtractName) -> Verdict:
  match x:
    case ExtractArgs2{}:
      Refuse{}
    case ExtractDomain{}:
      Allow{}

law density_extract_has_domain_name:
  {extract_verdict(ExtractArgs2{}) == Refuse{} : Verdict}

def density_extract_has_domain_name():
  {==}

type CoverageFeat is Data:
  CoverageNarrow{}
  CoverageMatch{}

def coverage_feat_verdict(x: CoverageFeat) -> Verdict:
  match x:
    case CoverageNarrow{}:
      Refuse{}
    case CoverageMatch{}:
      Allow{}

law coverage_features_match_workspace:
  {coverage_feat_verdict(CoverageNarrow{}) == Refuse{} : Verdict}

def coverage_features_match_workspace():
  {==}

type PerfRunner is Data:
  PerfNoRunner{}
  PerfMeasured{}

def perf_verdict(x: PerfRunner) -> Verdict:
  match x:
    case PerfNoRunner{}:
      Refuse{}
    case PerfMeasured{}:
      Allow{}

law perf_without_runner_is_not_gate:
  {perf_verdict(PerfNoRunner{}) == Refuse{} : Verdict}

def perf_without_runner_is_not_gate():
  {==}

type Sysdeps is Data:
  SysdepsMissing{}
  SysdepsInstalled{}

def sysdeps_verdict(x: Sysdeps) -> Verdict:
  match x:
    case SysdepsMissing{}:
      Refuse{}
    case SysdepsInstalled{}:
      Allow{}

law sysdeps_before_all_features:
  {sysdeps_verdict(SysdepsMissing{}) == Refuse{} : Verdict}

def sysdeps_before_all_features():
  {==}

type CfgTarget is Data:
  LinuxOnlyCfg{}
  EveryTargetLane{}

def cfg_target_verdict(x: CfgTarget) -> Verdict:
  match x:
    case LinuxOnlyCfg{}:
      Refuse{}
    case EveryTargetLane{}:
      Allow{}

law every_cfg_target_has_a_lane:
  {cfg_target_verdict(LinuxOnlyCfg{}) == Refuse{} : Verdict}

def every_cfg_target_has_a_lane():
  {==}

type MsrvLock is Data:
  MsrvNotLockMax{}
  MsrvLockMax{}

def msrv_lock_verdict(x: MsrvLock) -> Verdict:
  match x:
    case MsrvNotLockMax{}:
      Refuse{}
    case MsrvLockMax{}:
      Allow{}

law msrv_is_lock_max_including_target_gated:
  {msrv_lock_verdict(MsrvNotLockMax{}) == Refuse{} : Verdict}

def msrv_is_lock_max_including_target_gated():
  {==}

type ArtifactStep is Data:
  ArtifactFolded{}
  ArtifactOwnStep{}

def artifact_step_verdict(x: ArtifactStep) -> Verdict:
  match x:
    case ArtifactFolded{}:
      Refuse{}
    case ArtifactOwnStep{}:
      Allow{}

law artifact_check_is_its_own_step:
  {artifact_step_verdict(ArtifactFolded{}) == Refuse{} : Verdict}

def artifact_check_is_its_own_step():
  {==}

type MergeSemantics is Data:
  MergeSemanticsMixed{}
  MergeSemanticsDeclared{}

def merge_verdict(x: MergeSemantics) -> Verdict:
  match x:
    case MergeSemanticsMixed{}:
      Refuse{}
    case MergeSemanticsDeclared{}:
      Allow{}

law job_declares_merge_semantics:
  {merge_verdict(MergeSemanticsMixed{}) == Refuse{} : Verdict}

def job_declares_merge_semantics():
  {==}

type DropInRole is Data:
  DropInCeiling{}
  DropInFloor{}

def drop_in_verdict(x: DropInRole) -> Verdict:
  match x:
    case DropInCeiling{}:
      Refuse{}
    case DropInFloor{}:
      Allow{}

law drop_in_is_a_floor_not_a_ceiling:
  {drop_in_verdict(DropInCeiling{}) == Refuse{} : Verdict}

def drop_in_is_a_floor_not_a_ceiling():
  {==}

type DenySite is Data:
  DenyDouble{}
  DenySingle{}

def deny_verdict(x: DenySite) -> Verdict:
  match x:
    case DenyDouble{}:
      Refuse{}
    case DenySingle{}:
      Allow{}

law single_deny_site:
  {deny_verdict(DenyDouble{}) == Refuse{} : Verdict}

def single_deny_site():
  {==}
