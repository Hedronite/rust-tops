//! Style fixture. A trailing marker comment (slashes, at-sign, then fire or near and
//! a rule ID) names a line the rule must flag or must leave alone; `col=N` pins the
//! column. The probe lines sit in a module that `cfg` removes, so they are read but
//! never built; only the lint lines compile.

/// Returns the first element.
pub fn first(v: &[i32]) -> i32 {
    *v.first().unwrap() //@ fire PANIC-1
}

/// Returns the first element or zero.
pub fn first_or_zero(v: &[i32]) -> i32 {
    v.first().copied().unwrap_or(0) //@ near PANIC-1
}

#[cfg(any())]
mod probe_lines {
    #![allow(dead_code)] //@ fire IDIOM-9
    #[allow(dead_code)] //@ near IDIOM-9
    pub struct Handle;

    pub fn load() -> Result<(), Box<dyn std::error::Error>> { Ok(()) } //@ fire ERR-2
    fn private_load() -> Result<(), Box<dyn std::error::Error>> { Ok(()) } //@ near ERR-2
    pub fn accent(_w: &str /* é→ */) -> Result<(), Box<dyn std::error::Error>> { Ok(()) } //@ fire ERR-2 col=52

    pub fn wrapped() -> anyhow::Result<()> { Ok(()) } //@ fire ERR-3
    pub(crate) fn wrapped_inside() -> anyhow::Result<()> { Ok(()) } //@ near ERR-3

    pub fn guarded() {
        let _ = std::panic::catch_unwind(|| 1); //@ fire ERR-7
        // The foreign callback may unwind, so the boundary catches it.
        let _ = std::panic::catch_unwind(|| 2); //@ near ERR-7
    }

    pub fn set_flag(on: bool) {} //@ fire API-3
    pub fn is_flag() -> bool { true } //@ near API-3

    impl Handle {
        pub fn get_len(&self) -> usize { 0 } //@ fire API-11
        pub fn get_item(&self, index: usize) -> usize { index } //@ near API-11
    }

    pub struct ConnectionManager; //@ fire API-13
    pub struct HelperConfig; //@ near API-13

    pub fn bits(x: u32) -> f32 {
        unsafe { std::mem::transmute(x) } //@ fire UNSAFE-3
    }
    // transmute( is only named in this comment. //@ near UNSAFE-3

    unsafe impl Send for Handle {} //@ fire UNSAFE-4
    // SAFETY: the handle owns its pointer and nothing else aliases it.
    unsafe impl Sync for Handle {} //@ near UNSAFE-4

    /// Reads a number.
    ///
    /// ```
    /// let n: u8 = "7".parse().unwrap(); //@ fire DOC-4
    /// let n: u8 = "7".parse()?; //@ near DOC-4
    /// ```
    pub fn read() {}
    pub fn plain() { let _n: u8 = "7".parse().unwrap(); } //@ near DOC-4

    #[deny(warnings)] //@ fire IDIOM-1
    pub fn quiet() {}
    #[deny(missing_docs)] //@ near IDIOM-1
    pub fn loud() {}

    pub fn recover(m: &std::sync::Mutex<u8>) -> u8 {
        *m.lock().unwrap_or_else(|e| e.into_inner()) //@ fire CONC-4
    }
    pub fn plain_inner(c: std::cell::Cell<u8>) -> u8 { c.into_inner() } //@ near CONC-4
}
