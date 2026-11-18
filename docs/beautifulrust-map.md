---
title: "BeautifulRust bullet map"
type: reference
status: current
source: "BeautifulRust v1.5.0 (40db61c6), MIT, Copyright (c) 2026 Morgan Linton"
---

# BeautifulRust bullet map

<!-- MIT. Distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt. -->

Every bullet in scope from the three upstream skills, with the catalog rule that carries it in `crates/cargo-tops/templates/skills/rust-tops-style/rules.toml`, or the reason it was dropped. The gist column is our own summary, not upstream text.

Scope: `rust-best-practices` §2 and §13 (refs `S2.n`, `S13.n`), `rust-review` §3 (`R3.n`), and `rust-port` §1 to §6 (`P<section>.n`). A bullet is a list item or a table row, counted in file order within its section. A bullet that holds several checks is mapped to the rule that fits it best. The other rules it implies name the same section in their `source` field.

| Upstream | Gist | Rule |
|---|---|---|
| S2.1 | Fix lints instead of muting them; mute narrowly with expect | IDIOM-8 |
| S2.2 | No crate-level deny of the warnings group in source | IDIOM-1 |
| S2.3 | Set lint levels once in the manifest | IDIOM-9 |
| S13.1 | fmt, clippy and tests come back clean | dropped: the gate already runs fmt, clippy and the tests |
| S13.2 | No unwrap outside tests and main; panics carry values | PANIC-1 |
| S13.3 | Public parameters borrow the general form | API-1 |
| S13.4 | Public types derive Debug and the common traits; errors are full error types | API-9 |
| S13.5 | No public bool parameters; Ord consistent with Eq and Hash | API-3 |
| S13.6 | No clone that only calms the borrow checker | OWN-2 |
| S13.7 | No glob imports, no home-made preludes, no leaked foreign types | IDIOM-3 |
| S13.8 | Unsafe blocks and functions carry their safety text | UNSAFE-1 |
| S13.9 | Public items documented with a summary, Errors and Panics | DOC-5 |
| S13.10 | Applications stay simple; libraries follow the API rules | API-17 |
| R3.1 | Undefined behavior: missing safety text, transmute, forced Send or Sync, unsound safe fns | UNSAFE-3 |
| R3.2 | Reachable panics: unwrap, expect, indexing, wrapping casts, overflow | PANIC-2 |
| R3.3 | Ord, PartialEq and Hash that disagree | TRAIT-3 |
| R3.4 | Locks across await, guards returned, callbacks under a lock, poison recovery | CONC-1 |
| R3.5 | Bevy scheduling and query conflicts | dropped: Bevy is outside this slice |
| R3.6 | Destructors that panic, block or fail | PANIC-7 |
| R3.7 | Error types callers cannot use | ERR-2 |
| R3.8 | Errors that lose the cause or the context | ERR-6 |
| R3.9 | Panics or catch_unwind used for expected failures | PANIC-6 |
| R3.10 | Parameter shapes, bool parameters, needless ownership and cloning | API-4 |
| R3.11 | Returning a built Vec where an iterator would do | API-6 |
| R3.12 | Public fields, shared-ownership types and foreign types in signatures | API-7 |
| R3.13 | Missing standard trait impls | API-8 |
| R3.14 | Getter and conversion method naming, vague type names | API-12 |
| R3.15 | Enums, structs and traits that need to stay open to change | API-14 |
| R3.16 | Clones kept only for the borrow checker | OWN-1 |
| R3.17 | Test then unwrap; match versus combinator | IDIOM-4 |
| R3.18 | String plus, loops that want iterators, chains that want loops | IDIOM-5 |
| R3.19 | Hash map order leaking into output | IDIOM-6 |
| R3.20 | Deny-warnings, bare allow, glob imports of external crates | IDIOM-2 |
| R3.21 | Undocumented public items and docs that echo the name | DOC-1 |
| R3.22 | Missing Errors, Panics or Safety sections | DOC-2 |
| R3.23 | Doc examples that unwrap | DOC-4 |
| R3.24 | Untested logic and tests that restate constants | TEST-1 |
| P1.1 | Read the original code, its tests and its callers | PORT-1 |
| P1.2 | List every observable behavior, quiet cases included | PORT-1 |
| P1.3 | Mark what is only an artifact of the source language | PORT-2 |
| P1.4 | Keep an original bug and report it | PORT-3 |
| P3.1 | C: allocation pairs become owned types and Drop | PORT-5 |
| P3.2 | C: NULL becomes Option | PORT-5 |
| P3.3 | C: error codes and errno become a Result with an enum | PORT-5 |
| P3.4 | C: out-parameters become return values | PORT-5 |
| P3.5 | C: flag bitmasks become an options struct or bitflags | PORT-5 |
| P3.6 | C: char pointers and string compares become str operations | PORT-5 |
| P3.7 | C: pointer and count become slices and iterators | PORT-5 |
| P3.8 | C: fixed buffers become growable types | PORT-5 |
| P3.9 | C: defines and macros become const, enums and functions | PORT-5 |
| P3.10 | C: global mutable state moves into a caller-owned struct | PORT-5 |
| P3.11 | C: strtol family becomes parse with a mapped error | PORT-5 |
| P3.12 | Python: exception classes become an error enum | PORT-6 |
| P3.13 | Python: None becomes Option | PORT-6 |
| P3.14 | Python: string sets become an enum with FromStr and Display | PORT-6 |
| P3.15 | Python: dicts and classes as records become structs | PORT-6 |
| P3.16 | Python: observable dict order needs an ordered structure | PORT-6 |
| P3.17 | Python: duck typing and isinstance become traits or enums | PORT-6 |
| P3.18 | Python: unbounded int needs a chosen size and checked math | PORT-6 |
| P3.19 | Python: sorted with a key becomes a stable sort | PORT-6 |
| P3.20 | Python: string helpers need edge cases rechecked | PORT-6 |
| P3.21 | Python: try and except around parsing becomes ? or match | PORT-6 |
| P3.22 | Go: value-error pairs, nil, interfaces and goroutines | PORT-7 |
| P3.23 | JavaScript: null, thrown errors, objects and numbers | PORT-7 |
| P4.1 | Do not rebuild the source language's runtime | PORT-8 |
| P4.2 | Do not grow the public surface | PORT-9 |
| P4.3 | Keep the algorithm unless it is wrong | PORT-10 |
| P4.4 | Compare code size with the original | PORT-11 |
| P5.1 | Run build, test and clippy; ported tests pass; compare with the original where it can run | PORT-13 |
| P6.1 | Notes list the API map | PORT-12 |
| P6.2 | Notes list the test map | PORT-4 |
| P6.3 | Notes list deliberate differences | PORT-12 |
| P6.4 | Notes list preserved quirks | PORT-12 |
| P6.5 | Notes record verification | PORT-12 |
