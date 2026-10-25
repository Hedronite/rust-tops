# Rust-TOPS review gate

Additive guidance. It sits on top of [`RUST_TOPS.md`](../RUST_TOPS.md) §14.2: tests come from the contract, and a green run is not evidence the change is correct. It does not change CRAP, coverage floors, or mutation gates, and it is not a protocol section.

Source: Tomas Vykruta, 2026-10-02, [the write-up](https://x.com/tvykruta/status/2106138620780261842) and [the rules reply](https://x.com/tvykruta/status/2106138761511727451). The numbers below are his, from two production PRs and twelve reviews, re-checked by him on the PR and on head. They are the reason for the rules.

## What the source measured

- The session that wrote the code found 3 of 23 issues and recommended merging. A fresh agent on the same model, given only the diff, found 17 of 23.
- A reviewer that had not written the feature, but had spent a day in the repo, found 0 of 8 logic bugs. Fresh agents found 2 to 4 of 8.
- Reviewers that only read the diff found 0 of 8 logic bugs. Every logic bug came from a reviewer that ran the code.
- Writing the test inputs before reading the implementation moved one reviewer from 4 of 8 logic bugs to 7 of 8, plus one bug nobody else had found. Same model, about two cents more.
- The two strongest reviewers overlapped on 10 of 20 findings.
- A later feature had 272 passing tests and green CI. A fresh reviewer found 11 blocking bugs. A fix then created a new bug that only the next review caught. The tool that was supposed to enforce review needed three rounds before it worked, and installing it would have turned off hooks that were already there.

## 1. Who reviews

1. The reviewer is a fresh agent. It did not write the code, and it is not the session that holds the plan for this change. It receives only the diff, the repo, and this file.
2. The writer's own pass is a checklist, not a review. A reviewer that shares the writer's mental model inherits the writer's blind spots, whether or not it typed the diff.
3. This gate does not change who owns the change. The source's best cost and recall was a fresh reviewer on a different model from the writer. That is evidence for a fresh reviewer. The reviewer is whichever model did not write the diff.
4. High-risk changes get a second reviewer on a different model. High-risk means business rules, thresholds, scoring, pricing, or a validation gate. The two strongest reviewers in the source agreed on only half their findings.
5. Every fix commit gets its own fresh review. A fix can create the next bug. The first approval is not the end.

## 2. How to review

1. Before reading the implementation, write the inputs you will run. Inputs written after reading the code follow the code's own assumptions and miss what the code misses.
2. Run those inputs. Build a throwaway environment. Call the changed code. Watch what comes back. Reading the diff finds style problems. Running the code finds logic bugs.
3. Compare results with the base commit every time, including when the head is green.
4. Say what you executed. A green test suite is not evidence. The source's 272 passing tests covered none of the 11 blocking bugs.

## 3. What to probe

- Every form of an input, including two forms combined.
- Every unit, and the input with no unit.
- Two or more entities in one input, in every order.
- Every consumer of a changed enum, status, or constant.
- Every error, traced to the message the user actually sees.
- Every place the same fact is stored, computed, or shown. Server and client must still agree.
- Boundaries: empty, one, exactly at the threshold, past the top.
- Dates: leap days, year boundaries, time zones.

## 4. Honesty

Unknown stays unknown. The UI does not invent, guess, or silently fill data it does not have. A label must be true for every record it is shown on.

## 5. Design report

The review names each of these, with OK or a violation and a file and line. A principle left unmentioned means the review is incomplete.

- Separation of concerns
- Programming by intention
- Encapsulation
- High cohesion
- Low coupling

Blocking on this list: a domain with two owners, duplicated logic, or business logic in a template or UI. Rust-TOPS density, CRAP, and mutation gates still apply and are not replaced by this list.

## 6. Gates

1. Review is enforced by a gate, not by a line in the writer's prompt. The source's agent had "always get a code review" in its instructions and skipped it on the first PR.
2. The gate is per commit. No approved review for that exact commit, no push. Prefer a platform primitive (a git pre-push hook) over parsing the command.
3. Attack every new check, hook, or gate with inputs it should reject, including inputs phrased to evade it.
4. Every exemption must point at something that still exists, and no exemption may be a way to skip review.
5. Installing a hook must not disable hooks already in place.
6. Edits to this file, to `AGENTS.md`, and to the gate itself are reviewed too. Review the gate, not only the feature.

Do not install that hook from this file. Installing it is a separate change, and that change is reviewed too.

## 7. Docs and moves

After a search-and-replace or a move, re-read every edited sentence and ask whether it is still true. Dated notes keep their old paths and facts.

## 8. The report

- Each finding is BLOCKING or NIT. Blocking must be fixed. Nits are optional.
- Each finding has a reproduction: input, expected, actual.
- Report "N of the K found so far." The pool is never complete.
- End with exactly one line: `VERDICT: APPROVE` or `VERDICT: CHANGES`.

## What this does not do

- The writer does not grind the review. A fresh reviewer runs it. The owner of the change grades the report.
- It does not retune a crate against the questions that just failed. A re-test uses a fresh set.
- It does not change a threshold or the CRAP formula.
