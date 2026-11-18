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
pub unsafe fn probe_unsafe_op(value: *const u8) -> u8 { *value } // expect: unsafe_op_in_unsafe_fn // expect: clippy::missing_safety_doc // expect: missing_docs
pub struct ProbeNoDebug; // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
mod probe_hidden { pub fn probe_hidden_fn() {} } // expect: unreachable_pub // expect: dead_code
pub fn probe_result_panic(value: u8) -> Result<u8, u8> { assert!(value > 0, "positive"); Ok(value) } // expect: clippy::panic_in_result_fn // expect: clippy::missing_panics_doc // expect: clippy::missing_errors_doc // expect: missing_docs
pub fn probe_panic() { panic!("never") } // expect: clippy::panic // expect: clippy::missing_panics_doc // expect: missing_docs
pub fn probe_assert(value: u8) { assert!(value > 0); } // expect: clippy::missing_assert_message // expect: clippy::missing_panics_doc // expect: missing_docs
pub fn probe_unsafe_block() -> u8 { let value = 1u8; let ptr = &raw const value; unsafe { *ptr } } // expect: clippy::undocumented_unsafe_blocks // expect: missing_docs
pub fn probe_two_unsafe() -> u8 { let value = 1u8; let ptr = &raw const value; unsafe { *ptr + *ptr } } // expect: clippy::multiple_unsafe_ops_per_block // expect: clippy::undocumented_unsafe_blocks // expect: missing_docs
pub fn probe_bool_param(flag: bool) -> bool { flag } // expect: clippy::fn_params_excessive_bools // expect: missing_docs
pub struct ProbeBools { pub a: bool, pub b: bool, pub c: bool, pub d: bool } // expect: clippy::struct_excessive_bools // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
struct ProbeRc { value: std::rc::Rc<String> } // expect: clippy::rc_buffer // expect: dead_code
pub enum ProbeEnum { A, B } // expect: clippy::exhaustive_enums // expect: missing_debug_implementations // expect: missing_docs
pub fn probe_string_add(left: String, right: &str) -> String { left + right } // expect: clippy::string_add // expect: missing_docs
pub fn probe_wildcard() -> usize { use std::collections::*; HashMap::<u8, u8>::new().len() } // expect: clippy::wildcard_imports // expect: missing_docs
pub fn probe_dbg() -> u8 { dbg!(1) } // expect: clippy::dbg_macro // expect: missing_docs
pub fn probe_print() { println!("x") } // expect: clippy::print_stdout // expect: missing_docs
pub fn probe_eprint() { eprintln!("x") } // expect: clippy::print_stderr // expect: missing_docs
pub fn probe_cast_sign(value: i8) -> u8 { value as u8 } // expect: clippy::cast_sign_loss // expect: missing_docs
pub fn probe_long() { // expect: clippy::too_many_lines // expect: missing_docs
    let _line0 = 0u8;
    let _line1 = 1u8;
    let _line2 = 2u8;
    let _line3 = 3u8;
    let _line4 = 4u8;
    let _line5 = 5u8;
    let _line6 = 6u8;
    let _line7 = 7u8;
    let _line8 = 8u8;
    let _line9 = 9u8;
    let _line10 = 10u8;
    let _line11 = 11u8;
    let _line12 = 12u8;
    let _line13 = 13u8;
    let _line14 = 14u8;
    let _line15 = 15u8;
    let _line16 = 16u8;
    let _line17 = 17u8;
    let _line18 = 18u8;
    let _line19 = 19u8;
    let _line20 = 20u8;
    let _line21 = 21u8;
    let _line22 = 22u8;
    let _line23 = 23u8;
    let _line24 = 24u8;
    let _line25 = 25u8;
    let _line26 = 26u8;
    let _line27 = 27u8;
    let _line28 = 28u8;
    let _line29 = 29u8;
    let _line30 = 30u8;
    let _line31 = 31u8;
    let _line32 = 32u8;
    let _line33 = 33u8;
    let _line34 = 34u8;
    let _line35 = 35u8;
    let _line36 = 36u8;
    let _line37 = 37u8;
    let _line38 = 38u8;
    let _line39 = 39u8;
    let _line40 = 40u8;
    let _line41 = 41u8;
}
fn probe_borrowed_box(value: &Box<u8>) -> u8 { **value } // expect: clippy::borrowed_box // expect: dead_code
pub struct ProbeNew; impl ProbeNew { pub fn new() -> Self { Self } } // expect: clippy::new_without_default // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
pub struct ProbeAdd; impl ProbeAdd { pub fn add(self, _other: ProbeAdd) -> ProbeAdd { self } } // expect: clippy::should_implement_trait // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
struct ProbeSelf; impl ProbeSelf { fn into_value(&self) -> u8 { 1 } } // expect: clippy::wrong_self_convention // expect: dead_code
#[derive(Hash)] pub struct ProbeHash; impl PartialEq for ProbeHash { fn eq(&self, _other: &Self) -> bool { true } } // expect: clippy::derived_hash_with_manual_eq // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
#[derive(PartialEq, Eq, PartialOrd)] pub struct ProbeOrd; impl Ord for ProbeOrd { fn cmp(&self, _other: &Self) -> std::cmp::Ordering { std::cmp::Ordering::Equal } } // expect: clippy::derive_ord_xor_partial_ord // expect: missing_debug_implementations // expect: missing_docs // expect: clippy::exhaustive_structs
pub fn probe_unnecessary_unwrap(value: Option<u8>) -> u8 { if value.is_some() { value.unwrap() } else { 0 } } // expect: clippy::unnecessary_unwrap // expect: clippy::unwrap_used // expect: clippy::missing_panics_doc // expect: missing_docs
pub fn probe_range_loop(values: &[u8]) -> u8 { let mut sum = 0; for index in 0..values.len() { sum += values[index]; } sum } // expect: clippy::needless_range_loop // expect: clippy::indexing_slicing // expect: missing_docs
pub async fn probe_hold_lock(lock: &std::sync::Mutex<u8>) { let guard = lock.lock().unwrap(); std::future::ready(()).await; drop(guard); } // expect: clippy::await_holding_lock // expect: clippy::unwrap_used // expect: clippy::missing_panics_doc // expect: missing_docs
pub async fn probe_hold_refcell(cell: &std::cell::RefCell<u8>) { let guard = cell.borrow(); std::future::ready(()).await; drop(guard); } // expect: clippy::await_holding_refcell_ref // expect: missing_docs
pub fn probe_map_err(value: &str) -> Result<u8, String> { value.parse::<u8>().map_err(|_| String::new()) } // expect: clippy::map_err_ignore // expect: clippy::missing_errors_doc // expect: missing_docs
pub struct ProbeFields { pub a: u8 } // expect: clippy::exhaustive_structs // expect: missing_debug_implementations // expect: missing_docs
pub fn probe_hash_iter(map: &std::collections::HashMap<u8, u8>) -> u8 { let mut last = 0; for (key, value) in map { last = *key + *value; } last } // expect: clippy::iter_over_hash_type // expect: missing_docs
