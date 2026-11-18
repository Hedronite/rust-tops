//! Style fixture. A `//@ fire ID` line marks the next line as one the rule must
//! flag. A `//@ near ID` line marks the next line as one it must leave alone.

pub fn first(v: &[i32]) -> i32 {
    //@ fire PANIC-1
    *v.first().unwrap()
}

pub fn first_or_zero(v: &[i32]) -> i32 {
    //@ near PANIC-1
    v.first().copied().unwrap_or(0)
}

//@ fire PANIC-2
pub fn quit() { std::process::exit(1) }

//@ near PANIC-2
// std::process::exit( is only named in this comment.

//@ fire DOC-1
// TODO tidy this
//@ near DOC-1
// TODO(#12) tidy this

//@ fire ERR-1
pub fn load() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

//@ near ERR-1
fn private_load() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

//@ fire PANIC-2 col=42
pub fn accent() { let _word = "é→"; std::process::exit(1) }
