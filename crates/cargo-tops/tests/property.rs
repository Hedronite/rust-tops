//! Properties of two rules in `PROTOCOL.md`, written from the spec and not
//! from the code: an exemption `expires` is a real `YYYY-MM-DD` day and a
//! record counts only until that day (§19), and an exemption `path` glob
//! matches a path the way the exclude globs do (§19, `scope`).
//!
//! The crate is a binary, so this file compiles the three leaf modules it
//! tests from `src/`. Every test name holds `property`: that is what the
//! kit's `property` layer asks nextest for, and it is how the gate knows the
//! layer ran a test and not just the coverage run.

#[path = "../src/exemptions.rs"]
mod exemptions;
#[path = "../src/scope.rs"]
mod scope;
#[path = "../src/steps.rs"]
mod steps;

use exemptions::{civil_date, Exemption};
use proptest::prelude::*;
use scope::glob_matches;

/// The days in `month` of `year`, from the Gregorian rule, not from the code.
fn days_in(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        _ => 28,
    }
}

/// A path component with no glob character, from a small alphabet so a
/// component can repeat.
fn component() -> impl Strategy<Value = String> {
    "[a-c0-9_.]{1,4}"
}

proptest! {
    // A date is valid exactly when its month and day name a real day.
    #[test]
    fn property_an_expiry_is_valid_exactly_when_it_is_a_real_day(
        year in 1u32..=9999,
        month in 0u32..=13,
        day in 0u32..=32,
    ) {
        let text = format!("{year:04}-{month:02}-{day:02}");
        let real = (1..=12).contains(&month) && (1..=days_in(year, month)).contains(&day);
        prop_assert_eq!(steps::valid_expiry(&text), real, "{}", text);
    }

    // Text that is not `YYYY-MM-DD` is never a day.
    #[test]
    fn property_a_text_that_is_not_the_date_shape_is_never_a_day(
        text in "[0-9a-z/ .-]{0,12}",
    ) {
        let shaped = text.len() == 10
            && text.char_indices().all(|(at, ch)| {
                if at == 4 || at == 7 { ch == '-' } else { ch.is_ascii_digit() }
            });
        if !shaped {
            prop_assert!(!steps::valid_expiry(&text), "{}", text);
        }
    }

    // Every day the calendar counts is a real day, and a later day sorts
    // after an earlier one, which is how an expiry is compared with today.
    #[test]
    fn property_the_calendar_gives_real_days_in_order(days in 0u64..2_000_000) {
        let today = civil_date(days);
        let tomorrow = civil_date(days + 1);
        prop_assert!(steps::valid_expiry(&today), "{}", today);
        prop_assert!(tomorrow > today, "{} then {}", today, tomorrow);
    }

    // The record in §19 counts until its expiry day and not on or after it.
    #[test]
    fn property_an_exemption_is_live_exactly_before_its_expiry_day(
        expiry in 0u64..2_000_000,
        today in 0u64..2_000_000,
    ) {
        let record = Exemption {
            path: "src/**".to_string(),
            symbol: None,
            expires: civil_date(expiry),
        };
        prop_assert_eq!(record.live(&civil_date(today)), expiry > today);
    }

    // A glob made of a path's own components matches that path.
    #[test]
    fn property_a_glob_equal_to_a_path_matches_it(
        parts in prop::collection::vec(component(), 1..5),
    ) {
        let path = parts.join("/");
        prop_assert!(glob_matches(&path, &path), "{}", path);
    }

    // A glob also matches the tail of a path that starts at a directory.
    #[test]
    fn property_a_glob_matches_a_path_that_ends_in_it(
        dirs in prop::collection::vec(component(), 0..4),
        glob_parts in prop::collection::vec(component(), 1..4),
    ) {
        let glob = glob_parts.join("/");
        let path = dirs.iter().chain(glob_parts.iter()).cloned().collect::<Vec<_>>().join("/");
        prop_assert!(glob_matches(&glob, &path), "{} in {}", glob, path);
    }

    // `**` stands for any number of directories.
    #[test]
    fn property_a_double_star_matches_any_depth(
        dirs in prop::collection::vec(component(), 0..5),
        leaf in "[a-c]{1,4}",
    ) {
        let path = dirs.iter().cloned().chain(std::iter::once(leaf.clone()))
            .collect::<Vec<_>>().join("/");
        let glob = format!("**/{leaf}");
        prop_assert!(glob_matches(&glob, &path), "{}", path);
        prop_assert!(glob_matches("**", &path), "{}", path);
    }

    // `*` is any run inside one component and never crosses a `/`. The
    // alphabets differ so `dir` cannot also start a later part of the path.
    #[test]
    fn property_a_star_stays_inside_one_path_component(
        dir in "d[a-c]{1,3}",
        first in "x[a-c]{0,3}",
        second in "x[a-c]{0,3}",
    ) {
        let glob = format!("{dir}/*");
        let one = format!("{dir}/{first}");
        let two = format!("{dir}/{first}/{second}");
        prop_assert!(glob_matches(&glob, &one));
        prop_assert!(!glob_matches(&glob, &two));
    }

    // `?` is exactly one byte.
    #[test]
    fn property_a_question_mark_is_exactly_one_byte(
        head in "[a-c]{0,3}",
        mid in "[a-z]",
        tail in "[a-c]{0,3}",
    ) {
        let glob = format!("{head}?{tail}");
        let one = format!("{head}{mid}{tail}");
        let none = format!("{head}{tail}");
        prop_assert!(glob_matches(&glob, &one));
        prop_assert!(!glob_matches(&glob, &none));
    }
}
