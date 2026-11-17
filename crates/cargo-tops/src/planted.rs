//! Scratch code that no test reaches, planted to show `tops-gate` failing a
//! coverage floor. Reverted before the step is ready.

/// A branchy function nothing calls under test.
pub(crate) fn planted_uncovered(n: u32) -> u32 {
    match n {
        1 => n.wrapping_mul(4).rotate_left(2),
        2 => n.wrapping_mul(5).rotate_left(3),
        3 => n.wrapping_mul(6).rotate_left(4),
        4 => n.wrapping_mul(7).rotate_left(5),
        5 => n.wrapping_mul(8).rotate_left(6),
        6 => n.wrapping_mul(9).rotate_left(7),
        7 => n.wrapping_mul(10).rotate_left(1),
        8 => n.wrapping_mul(11).rotate_left(2),
        9 => n.wrapping_mul(12).rotate_left(3),
        10 => n.wrapping_mul(13).rotate_left(4),
        11 => n.wrapping_mul(14).rotate_left(5),
        12 => n.wrapping_mul(15).rotate_left(6),
        13 => n.wrapping_mul(16).rotate_left(7),
        14 => n.wrapping_mul(17).rotate_left(1),
        15 => n.wrapping_mul(18).rotate_left(2),
        16 => n.wrapping_mul(19).rotate_left(3),
        17 => n.wrapping_mul(20).rotate_left(4),
        18 => n.wrapping_mul(21).rotate_left(5),
        19 => n.wrapping_mul(22).rotate_left(6),
        20 => n.wrapping_mul(23).rotate_left(7),
        21 => n.wrapping_mul(24).rotate_left(1),
        22 => n.wrapping_mul(25).rotate_left(2),
        23 => n.wrapping_mul(26).rotate_left(3),
        24 => n.wrapping_mul(27).rotate_left(4),
        25 => n.wrapping_mul(28).rotate_left(5),
        26 => n.wrapping_mul(29).rotate_left(6),
        27 => n.wrapping_mul(30).rotate_left(7),
        28 => n.wrapping_mul(31).rotate_left(1),
        29 => n.wrapping_mul(32).rotate_left(2),
        30 => n.wrapping_mul(33).rotate_left(3),
        _ => n,
    }
}
