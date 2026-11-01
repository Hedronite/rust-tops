pub fn trick_unwrap(value: Result<u8, ()>) -> u8 { value.unwrap() } // expect: clippy::unwrap_used // expect: clippy::missing_panics_doc // expect: missing_docs
pub fn trick_expect(value: Option<u8>) -> u8 { value.expect("") } // expect: clippy::expect_used // expect: clippy::missing_panics_doc // expect: missing_docs
pub fn trick_pass_by_value(value: String) -> usize { value.len() } // expect: clippy::needless_pass_by_value // expect: missing_docs
pub fn trick_ptr_arg(value: &Vec<u8>) -> usize { value.len() } // expect: clippy::ptr_arg // expect: missing_docs
pub fn trick_rc(value: &std::rc::Rc<u8>) -> std::rc::Rc<u8> { value.clone() } // expect: clippy::clone_on_ref_ptr // expect: missing_docs
pub fn trick_index(value: &[u8]) -> u8 { value[2] } // expect: clippy::indexing_slicing // expect: missing_docs
pub fn trick_cast(value: u16) -> u8 { value as u8 } // expect: clippy::cast_possible_truncation // expect: missing_docs
pub fn trick_allow() { #[allow(clippy::unwrap_used)] let _ = Some(1).unwrap(); } // expect: clippy::allow_attributes // expect: clippy::allow_attributes_without_reason // expect: clippy::missing_panics_doc // expect: clippy::unnecessary_literal_unwrap // expect: missing_docs
pub fn trick_underscore() { let _ = std::fs::write("tops-strict-probe-nope", b"x"); } // expect: clippy::let_underscore_must_use // expect: missing_docs
pub fn trick_todo() { todo!() } // expect: clippy::todo // expect: missing_docs
pub fn trick_collect(value: &[u8]) { for _item in value.iter().collect::<Vec<_>>() {} } // expect: clippy::needless_collect // expect: missing_docs
pub fn trick_to_string(value: String) -> String { value.to_string() } // expect: clippy::implicit_clone // expect: clippy::needless_pass_by_value // expect: clippy::redundant_clone // expect: missing_docs
/// Never fails.
pub fn trick_never_err() -> Result<u8, ()> { Ok(1) } // expect: clippy::missing_errors_doc // expect: clippy::result_unit_err
pub fn trick_unwrap_or_default(value: &str) -> u8 { value.parse().unwrap_or_default() } // expect: missing_docs
pub fn trick_leak(value: u8) -> &'static u8 { Box::leak(Box::new(value)) } // expect: missing_docs
