import Base

# Spec of three cargo-tops decisions (rust-tops core-spec, G1).
# Enums and Nat only; no F32. Rust counterparts are mapped in spec/README.md.

# The outcome of one run (mirrors steps::Outcome).
type Outcome is Data:
  Passed{}
  Failed{}
  ToolMissing{}
  ToolMismatch{}
  Vacuous{}

# The row a verdict names (the strings runs_verdict and layer_verdict return).
# Bend has one constructor namespace (and Base owns RowFail), so every Row
# constructor carries a Row prefix: RowPass, RowFail, RowNotRun, and so on.
type Row is Data:
  RowPass{}
  RowFail{}
  RowToolMissing{}
  RowToolMismatch{}
  RowVacuous{}
  RowNotRun{}

# An exemption's expiry: a day number, or a string that is not a real day.
type Expiry is Data:
  Valid{day: Nat}
  Invalid{}

# ---------------------------------------------------------------- decisions

def to_row(o: Outcome) -> Row:
  match o:
    case Passed{}:
      RowPass{}
    case Failed{}:
      RowFail{}
    case ToolMissing{}:
      RowToolMissing{}
    case ToolMismatch{}:
      RowToolMismatch{}
    case Vacuous{}:
      RowVacuous{}

# Rank of a row: RowFail > ToolMissing > ToolMismatch > Vacuous > RowPass.
# RowNotRun is never returned by runs_verdict; it ranks 0 and only types the total fn.
def rank(r: Row) -> Nat:
  match r:
    case RowPass{}:
      0n
    case RowVacuous{}:
      1n
    case RowToolMismatch{}:
      2n
    case RowToolMissing{}:
      3n
    case RowFail{}:
      4n
    case RowNotRun{}:
      0n

# The worse of two outcomes by rank Failed > ToolMissing > ToolMismatch > Vacuous > Passed.
def worse(+a: Outcome, +b: Outcome) -> Outcome:
  match a:
    case Failed{}:
      Failed{}
    case Passed{}:
      b
    case ToolMissing{}:
      match b:
        case Failed{}:
          Failed{}
        case Passed{}:
          ToolMissing{}
        case ToolMissing{}:
          ToolMissing{}
        case ToolMismatch{}:
          ToolMissing{}
        case Vacuous{}:
          ToolMissing{}
    case ToolMismatch{}:
      match b:
        case Failed{}:
          Failed{}
        case Passed{}:
          ToolMismatch{}
        case ToolMissing{}:
          ToolMissing{}
        case ToolMismatch{}:
          ToolMismatch{}
        case Vacuous{}:
          ToolMismatch{}
    case Vacuous{}:
      match b:
        case Failed{}:
          Failed{}
        case Passed{}:
          Vacuous{}
        case ToolMissing{}:
          ToolMissing{}
        case ToolMismatch{}:
          ToolMismatch{}
        case Vacuous{}:
          Vacuous{}

# The worst outcome of a list of runs; the empty list is Passed (as the Rust).
def worst_of(rs: +List<Outcome>) -> Outcome:
  match rs:
    case Nil{}:
      Passed{}
    case h <> t:
      worse(h, worst_of(t))

def runs_verdict(rs: +List<Outcome>) -> Row:
  to_row(worst_of(rs))

# A passing row still fails when a floor or the survivor maximum was missed.
def gate(+v: Row, +missed: Bool) -> Row:
  match v:
    case RowPass{}:
      match missed:
        case True{}:
          RowFail{}
        case False{}:
          RowPass{}
    case RowFail{}:
      RowFail{}
    case RowToolMissing{}:
      RowToolMissing{}
    case RowToolMismatch{}:
      RowToolMismatch{}
    case RowVacuous{}:
      RowVacuous{}
    case RowNotRun{}:
      RowNotRun{}

def layer_verdict(+rs: +List<Outcome>, +missed: Bool) -> Row:
  match rs:
    case Nil{}:
      RowNotRun{}
    case h <> t:
      gate(runs_verdict(h <> t), missed)

def live(e: Expiry, today: Nat) -> Bool:
  match e:
    case Invalid{}:
      False{}
    case Valid{d}:
      Nat.is_gt(d, today)

# ------------------------------------------------------- predicates for laws

def has_failed(rs: +List<Outcome>) -> Bool:
  match rs:
    case Nil{}:
      False{}
    case h <> t:
      match h:
        case Failed{}:
          True{}
        case Passed{}:
          has_failed(t)
        case ToolMissing{}:
          has_failed(t)
        case ToolMismatch{}:
          has_failed(t)
        case Vacuous{}:
          has_failed(t)

def all_passed(rs: +List<Outcome>) -> Bool:
  match rs:
    case Nil{}:
      True{}
    case h <> t:
      match h:
        case Passed{}:
          all_passed(t)
        case Failed{}:
          False{}
        case ToolMissing{}:
          False{}
        case ToolMismatch{}:
          False{}
        case Vacuous{}:
          False{}

def no_runs(rs: +List<Outcome>) -> Bool:
  match rs:
    case Nil{}:
      True{}
    case h <> t:
      False{}

def cat(xs: +List<Outcome>, +ys: +List<Outcome>) -> +List<Outcome>:
  match xs:
    case Nil{}:
      ys
    case h <> t:
      h <> cat(t, ys)

# ------------------------------------------------------------ proof helpers

def IsFalse(b: Bool) -> Type:
  match b:
    case False{}:
      Unit
    case True{}:
      Empty

def NotPassedO(o: Outcome) -> Type:
  match o:
    case Passed{}:
      Empty
    case Failed{}:
      Unit
    case ToolMissing{}:
      Unit
    case ToolMismatch{}:
      Unit
    case Vacuous{}:
      Unit

def NotPassR(r: Row) -> Type:
  match r:
    case RowPass{}:
      Empty
    case RowFail{}:
      Unit
    case RowToolMissing{}:
      Unit
    case RowToolMismatch{}:
      Unit
    case RowVacuous{}:
      Unit
    case RowNotRun{}:
      Unit

# False is not True.
def bool_ne(h: {False{} == True{} : Bool}) -> Empty:
  %h : IsFalse(_)
  Unit{}

# An outcome that is not Passed is not Passed (u is the Unit witness).
def out_ne_passed(o: Outcome, h: {o == Passed{} : Outcome}, u: NotPassedO(o)) -> Empty:
  %h : NotPassedO(_)
  u

def row_ne_pass(r: Row, h: {r == RowPass{} : Row}, u: NotPassR(r)) -> Empty:
  %h : NotPassR(_)
  u

def worse_comm(a: Outcome, b: Outcome) -> {worse(a, b) == worse(b, a) : Outcome}:
  match a b:
    case Passed{} Passed{}:
      {==}
    case Passed{} Vacuous{}:
      {==}
    case Passed{} ToolMismatch{}:
      {==}
    case Passed{} ToolMissing{}:
      {==}
    case Passed{} Failed{}:
      {==}
    case Vacuous{} Passed{}:
      {==}
    case Vacuous{} Vacuous{}:
      {==}
    case Vacuous{} ToolMismatch{}:
      {==}
    case Vacuous{} ToolMissing{}:
      {==}
    case Vacuous{} Failed{}:
      {==}
    case ToolMismatch{} Passed{}:
      {==}
    case ToolMismatch{} Vacuous{}:
      {==}
    case ToolMismatch{} ToolMismatch{}:
      {==}
    case ToolMismatch{} ToolMissing{}:
      {==}
    case ToolMismatch{} Failed{}:
      {==}
    case ToolMissing{} Passed{}:
      {==}
    case ToolMissing{} Vacuous{}:
      {==}
    case ToolMissing{} ToolMismatch{}:
      {==}
    case ToolMissing{} ToolMissing{}:
      {==}
    case ToolMissing{} Failed{}:
      {==}
    case Failed{} Passed{}:
      {==}
    case Failed{} Vacuous{}:
      {==}
    case Failed{} ToolMismatch{}:
      {==}
    case Failed{} ToolMissing{}:
      {==}
    case Failed{} Failed{}:
      {==}

def worse_assoc(a: Outcome, b: Outcome, c: Outcome) -> {worse(worse(a, b), c) == worse(a, worse(b, c)) : Outcome}:
  match a b c:
    case Failed{} _ _:
      {==}
    case Passed{} _ _:
      {==}
    case ToolMissing{} Failed{} _:
      {==}
    case ToolMissing{} Passed{} _:
      {==}
    case ToolMissing{} ToolMissing{} Passed{}:
      {==}
    case ToolMissing{} ToolMissing{} Vacuous{}:
      {==}
    case ToolMissing{} ToolMissing{} ToolMismatch{}:
      {==}
    case ToolMissing{} ToolMissing{} ToolMissing{}:
      {==}
    case ToolMissing{} ToolMissing{} Failed{}:
      {==}
    case ToolMissing{} ToolMismatch{} Passed{}:
      {==}
    case ToolMissing{} ToolMismatch{} Vacuous{}:
      {==}
    case ToolMissing{} ToolMismatch{} ToolMismatch{}:
      {==}
    case ToolMissing{} ToolMismatch{} ToolMissing{}:
      {==}
    case ToolMissing{} ToolMismatch{} Failed{}:
      {==}
    case ToolMissing{} Vacuous{} Passed{}:
      {==}
    case ToolMissing{} Vacuous{} Vacuous{}:
      {==}
    case ToolMissing{} Vacuous{} ToolMismatch{}:
      {==}
    case ToolMissing{} Vacuous{} ToolMissing{}:
      {==}
    case ToolMissing{} Vacuous{} Failed{}:
      {==}
    case ToolMismatch{} Failed{} _:
      {==}
    case ToolMismatch{} Passed{} _:
      {==}
    case ToolMismatch{} ToolMissing{} Passed{}:
      {==}
    case ToolMismatch{} ToolMissing{} Vacuous{}:
      {==}
    case ToolMismatch{} ToolMissing{} ToolMismatch{}:
      {==}
    case ToolMismatch{} ToolMissing{} ToolMissing{}:
      {==}
    case ToolMismatch{} ToolMissing{} Failed{}:
      {==}
    case ToolMismatch{} ToolMismatch{} Passed{}:
      {==}
    case ToolMismatch{} ToolMismatch{} Vacuous{}:
      {==}
    case ToolMismatch{} ToolMismatch{} ToolMismatch{}:
      {==}
    case ToolMismatch{} ToolMismatch{} ToolMissing{}:
      {==}
    case ToolMismatch{} ToolMismatch{} Failed{}:
      {==}
    case ToolMismatch{} Vacuous{} Passed{}:
      {==}
    case ToolMismatch{} Vacuous{} Vacuous{}:
      {==}
    case ToolMismatch{} Vacuous{} ToolMismatch{}:
      {==}
    case ToolMismatch{} Vacuous{} ToolMissing{}:
      {==}
    case ToolMismatch{} Vacuous{} Failed{}:
      {==}
    case Vacuous{} Failed{} _:
      {==}
    case Vacuous{} Passed{} _:
      {==}
    case Vacuous{} ToolMissing{} Passed{}:
      {==}
    case Vacuous{} ToolMissing{} Vacuous{}:
      {==}
    case Vacuous{} ToolMissing{} ToolMismatch{}:
      {==}
    case Vacuous{} ToolMissing{} ToolMissing{}:
      {==}
    case Vacuous{} ToolMissing{} Failed{}:
      {==}
    case Vacuous{} ToolMismatch{} Passed{}:
      {==}
    case Vacuous{} ToolMismatch{} Vacuous{}:
      {==}
    case Vacuous{} ToolMismatch{} ToolMismatch{}:
      {==}
    case Vacuous{} ToolMismatch{} ToolMissing{}:
      {==}
    case Vacuous{} ToolMismatch{} Failed{}:
      {==}
    case Vacuous{} Vacuous{} Passed{}:
      {==}
    case Vacuous{} Vacuous{} Vacuous{}:
      {==}
    case Vacuous{} Vacuous{} ToolMismatch{}:
      {==}
    case Vacuous{} Vacuous{} ToolMissing{}:
      {==}
    case Vacuous{} Vacuous{} Failed{}:
      {==}

# Adding a run never lowers the rank of the verdict (all 25 pairs).
def rank_step(r: Outcome, v: Outcome) -> {Nat.is_ge(rank(to_row(worse(r, v))), rank(to_row(v))) == True{} : Bool}:
  match r v:
    case Passed{} Passed{}:
      {==}
    case Passed{} Vacuous{}:
      {==}
    case Passed{} ToolMismatch{}:
      {==}
    case Passed{} ToolMissing{}:
      {==}
    case Passed{} Failed{}:
      {==}
    case Vacuous{} Passed{}:
      {==}
    case Vacuous{} Vacuous{}:
      {==}
    case Vacuous{} ToolMismatch{}:
      {==}
    case Vacuous{} ToolMissing{}:
      {==}
    case Vacuous{} Failed{}:
      {==}
    case ToolMismatch{} Passed{}:
      {==}
    case ToolMismatch{} Vacuous{}:
      {==}
    case ToolMismatch{} ToolMismatch{}:
      {==}
    case ToolMismatch{} ToolMissing{}:
      {==}
    case ToolMismatch{} Failed{}:
      {==}
    case ToolMissing{} Passed{}:
      {==}
    case ToolMissing{} Vacuous{}:
      {==}
    case ToolMissing{} ToolMismatch{}:
      {==}
    case ToolMissing{} ToolMissing{}:
      {==}
    case ToolMissing{} Failed{}:
      {==}
    case Failed{} Passed{}:
      {==}
    case Failed{} Vacuous{}:
      {==}
    case Failed{} ToolMismatch{}:
      {==}
    case Failed{} ToolMissing{}:
      {==}
    case Failed{} Failed{}:
      {==}

def worse_passed_l(a: Outcome, b: Outcome, h: {worse(a, b) == Passed{} : Outcome}) -> {a == Passed{} : Outcome}:
  match a b:
    case Passed{} Passed{}:
      {==}
    case Passed{} Vacuous{}:
      {==}
    case Passed{} ToolMismatch{}:
      {==}
    case Passed{} ToolMissing{}:
      {==}
    case Passed{} Failed{}:
      {==}
    case Vacuous{} Passed{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, out_ne_passed(Vacuous{}, h, Unit{}))
    case Vacuous{} Vacuous{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, out_ne_passed(Vacuous{}, h, Unit{}))
    case Vacuous{} ToolMismatch{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, out_ne_passed(ToolMismatch{}, h, Unit{}))
    case Vacuous{} ToolMissing{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case Vacuous{} Failed{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case ToolMismatch{} Passed{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, out_ne_passed(ToolMismatch{}, h, Unit{}))
    case ToolMismatch{} Vacuous{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, out_ne_passed(ToolMismatch{}, h, Unit{}))
    case ToolMismatch{} ToolMismatch{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, out_ne_passed(ToolMismatch{}, h, Unit{}))
    case ToolMismatch{} ToolMissing{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case ToolMismatch{} Failed{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case ToolMissing{} Passed{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case ToolMissing{} Vacuous{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case ToolMissing{} ToolMismatch{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case ToolMissing{} ToolMissing{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, out_ne_passed(ToolMissing{}, h, Unit{}))
    case ToolMissing{} Failed{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case Failed{} Passed{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case Failed{} Vacuous{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case Failed{} ToolMismatch{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case Failed{} ToolMissing{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))
    case Failed{} Failed{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, out_ne_passed(Failed{}, h, Unit{}))

def to_row_pass(o: Outcome, h: {to_row(o) == RowPass{} : Row}) -> {o == Passed{} : Outcome}:
  match o:
    case Passed{}:
      {==}
    case Vacuous{}:
      Empty.absurd({Vacuous{} == Passed{} : Outcome}, row_ne_pass(RowVacuous{}, h, Unit{}))
    case ToolMismatch{}:
      Empty.absurd({ToolMismatch{} == Passed{} : Outcome}, row_ne_pass(RowToolMismatch{}, h, Unit{}))
    case ToolMissing{}:
      Empty.absurd({ToolMissing{} == Passed{} : Outcome}, row_ne_pass(RowToolMissing{}, h, Unit{}))
    case Failed{}:
      Empty.absurd({Failed{} == Passed{} : Outcome}, row_ne_pass(RowFail{}, h, Unit{}))

def gate_pass(v: Row, missed: Bool, h: {gate(v, missed) == RowPass{} : Row}) -> {v == RowPass{} : Row} & {missed == False{} : Bool}:
  match v missed:
    case RowPass{} False{}:
      ({==}, {==})
    case RowPass{} True{}:
      Empty.absurd({RowPass{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowFail{}, h, Unit{}))
    case RowFail{} False{}:
      Empty.absurd({RowFail{} == RowPass{} : Row} & {False{} == False{} : Bool}, row_ne_pass(RowFail{}, h, Unit{}))
    case RowFail{} True{}:
      Empty.absurd({RowFail{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowFail{}, h, Unit{}))
    case RowToolMissing{} False{}:
      Empty.absurd({RowToolMissing{} == RowPass{} : Row} & {False{} == False{} : Bool}, row_ne_pass(RowToolMissing{}, h, Unit{}))
    case RowToolMissing{} True{}:
      Empty.absurd({RowToolMissing{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowToolMissing{}, h, Unit{}))
    case RowToolMismatch{} False{}:
      Empty.absurd({RowToolMismatch{} == RowPass{} : Row} & {False{} == False{} : Bool}, row_ne_pass(RowToolMismatch{}, h, Unit{}))
    case RowToolMismatch{} True{}:
      Empty.absurd({RowToolMismatch{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowToolMismatch{}, h, Unit{}))
    case RowVacuous{} False{}:
      Empty.absurd({RowVacuous{} == RowPass{} : Row} & {False{} == False{} : Bool}, row_ne_pass(RowVacuous{}, h, Unit{}))
    case RowVacuous{} True{}:
      Empty.absurd({RowVacuous{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowVacuous{}, h, Unit{}))
    case RowNotRun{} False{}:
      Empty.absurd({RowNotRun{} == RowPass{} : Row} & {False{} == False{} : Bool}, row_ne_pass(RowNotRun{}, h, Unit{}))
    case RowNotRun{} True{}:
      Empty.absurd({RowNotRun{} == RowPass{} : Row} & {True{} == False{} : Bool}, row_ne_pass(RowNotRun{}, h, Unit{}))

def worst_failed(+rs: +List<Outcome>, h: {has_failed(rs) == True{} : Bool}) -> {worst_of(rs) == Failed{} : Outcome}:
  match rs:
    case Nil{}:
      Empty.absurd({worst_of(Nil{}) == Failed{} : Outcome}, bool_ne(h))
    case +o <> +t:
      match o:
        case Failed{}:
          {==}
        case Passed{}:
          worst_failed(t, h)
        case ToolMissing{}:
          %Equal.sym(Outcome, worst_of(t), Failed{}, worst_failed(t, h)) : {worse(ToolMissing{}, _) == Failed{} : Outcome}
          {==}
        case ToolMismatch{}:
          %Equal.sym(Outcome, worst_of(t), Failed{}, worst_failed(t, h)) : {worse(ToolMismatch{}, _) == Failed{} : Outcome}
          {==}
        case Vacuous{}:
          %Equal.sym(Outcome, worst_of(t), Failed{}, worst_failed(t, h)) : {worse(Vacuous{}, _) == Failed{} : Outcome}
          {==}

def worst_pass(+rs: +List<Outcome>, h: {worst_of(rs) == Passed{} : Outcome}) -> {all_passed(rs) == True{} : Bool}:
  match rs:
    case Nil{}:
      {==}
    case +o <> +t:
      match o:
        case Passed{}:
          worst_pass(t, h)
        case Failed{}:
          Empty.absurd({all_passed(Failed{} <> t) == True{} : Bool}, out_ne_passed(Failed{}, worse_passed_l(Failed{}, worst_of(t), h), Unit{}))
        case ToolMissing{}:
          Empty.absurd({all_passed(ToolMissing{} <> t) == True{} : Bool}, out_ne_passed(ToolMissing{}, worse_passed_l(ToolMissing{}, worst_of(t), h), Unit{}))
        case ToolMismatch{}:
          Empty.absurd({all_passed(ToolMismatch{} <> t) == True{} : Bool}, out_ne_passed(ToolMismatch{}, worse_passed_l(ToolMismatch{}, worst_of(t), h), Unit{}))
        case Vacuous{}:
          Empty.absurd({all_passed(Vacuous{} <> t) == True{} : Bool}, out_ne_passed(Vacuous{}, worse_passed_l(Vacuous{}, worst_of(t), h), Unit{}))

def worst_cat(+a: +List<Outcome>, +b: +List<Outcome>) -> {worst_of(cat(a, b)) == worse(worst_of(a), worst_of(b)) : Outcome}:
  match a:
    case Nil{}:
      {==}
    case +o <> +t:
      %Equal.sym(Outcome, worst_of(cat(t, b)), worse(worst_of(t), worst_of(b)), worst_cat(t, b)) : {worse(o, _) == worse(worse(o, worst_of(t)), worst_of(b)) : Outcome}
      Equal.sym(Outcome, worse(worse(o, worst_of(t)), worst_of(b)), worse(o, worse(worst_of(t), worst_of(b))), worse_assoc(o, worst_of(t), worst_of(b)))

def green_split(+rs: +List<Outcome>, +missed: Bool, p: {runs_verdict(rs) == RowPass{} : Row} & {missed == False{} : Bool}) -> {all_passed(rs) == True{} : Bool} & {missed == False{} : Bool}:
  match p:
    case (hv, hm):
      (worst_pass(rs, to_row_pass(worst_of(rs), hv)), hm)

# ---------------------------------------------------------------------- laws

# C1: a failed run fails the layer.
law fail_outranks_all:
  for +rs: +List<Outcome>
  for h: {has_failed(rs) == True{} : Bool}
  {runs_verdict(rs) == RowFail{} : Row}

def fail_outranks_all(rs, h):
  Equal.cong(Outcome, Row, x => to_row(x), worst_of(rs), Failed{}, worst_failed(rs, h))

# C2: a missing layer is never green. Green needs a run, every run Passed,
# and no floor or survivor maximum missed.
law green_needs_every_run:
  for +rs: +List<Outcome>
  for +missed: Bool
  for h: {layer_verdict(rs, missed) == RowPass{} : Row}
  {no_runs(rs) == False{} : Bool} & ({all_passed(rs) == True{} : Bool} & {missed == False{} : Bool})

def green_needs_every_run(rs, missed, h):
  match rs:
    case Nil{}:
      Empty.absurd({no_runs(Nil{}) == False{} : Bool} & ({all_passed(Nil{}) == True{} : Bool} & {missed == False{} : Bool}), row_ne_pass(RowNotRun{}, h, Unit{}))
    case +o <> +t:
      ({==}, green_split(o <> t, missed, gate_pass(runs_verdict(o <> t), missed, h)))

# C3: the order of the runs does not change the verdict.
law order_does_not_matter:
  for +a: +List<Outcome>
  for +b: +List<Outcome>
  {runs_verdict(cat(a, b)) == runs_verdict(cat(b, a)) : Row}

def order_does_not_matter(a, b):
  Equal.trans(Row, runs_verdict(cat(a, b)), to_row(worse(worst_of(a), worst_of(b))), runs_verdict(cat(b, a)),
    Equal.cong(Outcome, Row, x => to_row(x), worst_of(cat(a, b)), worse(worst_of(a), worst_of(b)), worst_cat(a, b)),
    Equal.trans(Row, to_row(worse(worst_of(a), worst_of(b))), to_row(worse(worst_of(b), worst_of(a))), runs_verdict(cat(b, a)),
      Equal.cong(Outcome, Row, x => to_row(x), worse(worst_of(a), worst_of(b)), worse(worst_of(b), worst_of(a)), worse_comm(worst_of(a), worst_of(b))),
      Equal.sym(Row, runs_verdict(cat(b, a)), to_row(worse(worst_of(b), worst_of(a))),
        Equal.cong(Outcome, Row, x => to_row(x), worst_of(cat(b, a)), worse(worst_of(b), worst_of(a)), worst_cat(b, a)))))

# C4: one more run never raises the verdict's standing (never makes it better).
law more_runs_never_better:
  for +r: Outcome
  for +rs: +List<Outcome>
  {Nat.is_ge(rank(runs_verdict(r <> rs)), rank(runs_verdict(rs))) == True{} : Bool}

def more_runs_never_better(r, rs):
  rank_step(r, worst_of(rs))

# C5: an expired exemption covers nothing; an invalid date is expired.
law expired_covers_nothing:
  for +e: Expiry
  for +today: Nat
  for h: {live(e, today) == True{} : Bool}
  exs d: Nat
  {e == Valid{d} : Expiry} & {Nat.is_gt(d, today) == True{} : Bool}

def expired_covers_nothing(e, today, h):
  match e:
    case Invalid{}:
      Empty.absurd(&d: Nat -> ({Invalid{} == Valid{d} : Expiry} & {Nat.is_gt(d, today) == True{} : Bool}), bool_ne(h))
    case Valid{+d}:
      (d, ({==}, h))

