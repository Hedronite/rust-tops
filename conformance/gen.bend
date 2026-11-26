import Base
import ../spec/tops_core.bend as Spec

# Corpus printer for conformance/tops_core.json (core-spec G2).
# Enumerators and printers only: every verdict comes from Spec.*.
# One line per case, fields separated by "|":
#   runs_verdict|<outcomes, each followed by a comma>|<Row>
#   layer_verdict|<outcomes, each followed by a comma>|<missed>|<Row>
#   live|<day, or Invalid>|<today>|<Bool>

def outcome_name(o: Spec.Outcome) -> String:
  match o:
    case Spec.Passed{}:
      "Passed"
    case Spec.Failed{}:
      "Failed"
    case Spec.ToolMissing{}:
      "ToolMissing"
    case Spec.ToolMismatch{}:
      "ToolMismatch"
    case Spec.Vacuous{}:
      "Vacuous"

def row_name(r: Spec.Row) -> String:
  match r:
    case Spec.RowPass{}:
      "RowPass"
    case Spec.RowFail{}:
      "RowFail"
    case Spec.RowToolMissing{}:
      "RowToolMissing"
    case Spec.RowToolMismatch{}:
      "RowToolMismatch"
    case Spec.RowVacuous{}:
      "RowVacuous"
    case Spec.RowNotRun{}:
      "RowNotRun"

def bool_name(b: Bool) -> String:
  match b:
    case True{}:
      "True"
    case False{}:
      "False"

def runs_text(rs: +List<Spec.Outcome>) -> String:
  match rs:
    case Nil{}:
      ""
    case h <> t:
      outcome_name(h) ++ "," ++ runs_text(t)

def runs_lines(+rs: +List<Spec.Outcome>) -> String:
  "runs_verdict|" ++ runs_text(rs) ++ "|" ++ row_name(Spec.runs_verdict(rs)) ++ "\n"

def layer_line(+rs: +List<Spec.Outcome>, +missed: Bool) -> String:
  "layer_verdict|" ++ runs_text(rs) ++ "|" ++ bool_name(missed) ++ "|" ++ row_name(Spec.layer_verdict(rs, missed)) ++ "\n"

def list_lines(+rs: +List<Spec.Outcome>) -> String:
  runs_lines(rs) ++ layer_line(rs, False{}) ++ layer_line(rs, True{})

# Every list of length 0..fuel, built by consing onto the front.
def walk(fuel: Nat, +rs: +List<Spec.Outcome>) -> String:
  match fuel:
    case 0n:
      list_lines(rs)
    case 1n++f:
      list_lines(rs)
        ++ walk(f, Spec.Passed{} <> rs)
        ++ walk(f, Spec.Failed{} <> rs)
        ++ walk(f, Spec.ToolMissing{} <> rs)
        ++ walk(f, Spec.ToolMismatch{} <> rs)
        ++ walk(f, Spec.Vacuous{} <> rs)

def day_text(e: Spec.Expiry) -> String:
  match e:
    case Spec.Invalid{}:
      "Invalid"
    case Spec.Valid{d}:
      Nat.show(d)

# today = n-1, ..., 0 for fuel n.
def live_todays(fuel: Nat, +e: Spec.Expiry) -> String:
  match fuel:
    case 0n:
      ""
    case 1n++t:
      "live|" ++ day_text(e) ++ "|" ++ Nat.show(t) ++ "|" ++ bool_name(Spec.live(e, t)) ++ "\n" ++ live_todays(t, e)

# Valid(n-1), ..., Valid(0) for fuel n, each against today 0..30.
def live_days(fuel: Nat) -> String:
  match fuel:
    case 0n:
      ""
    case 1n++d:
      live_todays(31n, Spec.Valid{d}) ++ live_days(d)

def main() -> IO(Unit):
  IO.print(walk(4n, Nil{}) ++ live_todays(31n, Spec.Invalid{}) ++ live_days(31n))
