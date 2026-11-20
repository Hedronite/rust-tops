# mutants-27.1.0

`outcomes.json` is the file `cargo mutants --timeout 4 -j 1` 27.1.0 wrote for the
crate below, with the toolchain path in each `argv` shortened to `cargo`.
Nothing else was edited. It holds 11 caught, 10 missed, 1 timeout and 1
unviable mutant, and the baseline run.

```rust
pub struct Opaque(u8);

pub fn add(a: i32, b: i32) -> i32 { a + b }
pub fn double(x: i32) -> i32 { x * 2 }
pub fn opaque() -> Opaque { Opaque(1) }
pub fn spin(n: u32) -> u32 { let mut i = 0; while i < n { i += 1; } i }

pub mod inner {
    pub fn triple(x: i32) -> i32 { x * 3 }
}
```

The tests call `add` (three asserts), `spin(3)` and `opaque()`. `double` and
`inner::triple` have no test, so every mutant in them is missed. `opaque` has no
`Default`, so its mutant is unviable. `+=` to `*=` in `spin` never ends, so it
times out.

The file is real output, not a hand-built shape. To re-record it, run the same
command over the same crate with cargo-mutants 27.1.0 and shorten the paths.
