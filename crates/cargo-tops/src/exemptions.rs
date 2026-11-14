//! The §19 exemption records the coverage and crap judges honor.
//!
//! An exemption names one `layer`, a `path` glob (matched the way
//! [`crate::scope`] matches exclude globs), and for `crap` an optional
//! `symbol`. It counts until its `expires` date: on that day and after, it
//! exempts nothing and the judge reports it, because an expired exemption
//! fails CI (§19). A coverage exemption is per file; its `symbol`, if any,
//! is not read.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::scope::glob_matches;
use crate::steps;

/// One `exemptions` entry for the layer being judged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Exemption {
    pub path: String,
    pub symbol: Option<String>,
    pub expires: String,
}

impl Exemption {
    /// Whether the record exempts `function` (or the whole file, for `None`)
    /// in the file `rel`, relative to the crate root.
    pub(crate) fn covers(&self, rel: &str, function: Option<&str>) -> bool {
        glob_matches(&self.path, rel) && self.symbol_covers(function)
    }

    fn symbol_covers(&self, function: Option<&str>) -> bool {
        match (self.symbol.as_deref(), function) {
            (None, _) | (Some(_), None) => true,
            (Some(symbol), Some(name)) => name == symbol || name.ends_with(&format!("::{symbol}")),
        }
    }

    /// Whether the record still counts on `today` (`YYYY-MM-DD`). A date
    /// that is not a real day never counts.
    pub(crate) fn live(&self, today: &str) -> bool {
        steps::valid_expiry(&self.expires) && self.expires.as_str() > today
    }

    /// The problem line for a record that no longer counts.
    pub(crate) fn expired_problem(&self, layer: &str) -> String {
        format!(
            "{layer}: the exemption for {}{} expired on {} (or its date is not a day); \
             renew it with a reason or remove it (RUST_TOPS.md §19)",
            self.path,
            self.symbol
                .as_deref()
                .map(|symbol| format!(" ({symbol})"))
                .unwrap_or_default(),
            self.expires
        )
    }
}

/// The records in `exemptions` whose `layer` is `layer`. A record without a
/// string `path` or `expires` is skipped; the schema requires both.
pub(crate) fn for_layer(exemptions: Option<&Value>, layer: &str) -> Vec<Exemption> {
    exemptions
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("layer").and_then(Value::as_str) == Some(layer))
        .filter_map(|entry| {
            Some(Exemption {
                path: entry.get("path")?.as_str()?.to_string(),
                symbol: entry
                    .get("symbol")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                expires: entry.get("expires")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// Today's UTC date as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    civil_date(secs / 86_400)
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Hinnant's
/// `civil_from_days`, for days on or after the epoch).
pub(crate) fn civil_date(days: u64) -> String {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::{civil_date, for_layer, today, Exemption};
    use serde_json::json;

    fn exemption(path: &str, symbol: Option<&str>, expires: &str) -> Exemption {
        Exemption {
            path: path.to_string(),
            symbol: symbol.map(str::to_string),
            expires: expires.to_string(),
        }
    }

    #[test]
    fn civil_date_matches_known_days() {
        let cases: &[(u64, &str)] = &[
            (0, "1970-01-01"),
            (31, "1970-02-01"),
            (59, "1970-03-01"),
            (365, "1971-01-01"),
            (10_957, "2000-01-01"),
            (11_016, "2000-02-29"),
            (11_017, "2000-03-01"),
            (19_782, "2024-02-29"),
            (20_771, "2026-11-14"),
            (36_890, "2071-01-01"),
        ];
        for &(days, want) in cases {
            assert_eq!(civil_date(days), want, "{days}");
        }
        assert_eq!(today().len(), 10, "today is YYYY-MM-DD");
        assert!(today().as_str() > "2026-01-01", "the clock is after 2026");
    }

    #[test]
    fn only_the_named_layer_with_a_path_and_expiry_is_read() {
        let records = json!([
            {"path": "src/gen/**", "layer": "crap", "reason": "r", "owner": "o", "expires": "2030-01-01"},
            {"path": "src/a.rs", "symbol": "main", "layer": "crap", "reason": "r", "owner": "o", "expires": "2030-01-01"},
            {"path": "src/b.rs", "layer": "coverage", "reason": "r", "owner": "o", "expires": "2030-01-01"},
            {"layer": "crap", "expires": "2030-01-01"},
            {"path": "src/c.rs", "layer": "crap"},
        ]);
        assert_eq!(
            for_layer(Some(&records), "crap"),
            [
                exemption("src/gen/**", None, "2030-01-01"),
                exemption("src/a.rs", Some("main"), "2030-01-01"),
            ]
        );
        assert_eq!(for_layer(Some(&records), "mutation"), []);
        assert_eq!(for_layer(None, "crap"), []);
        assert_eq!(for_layer(Some(&json!({})), "crap"), []);
    }

    #[test]
    fn a_record_covers_its_path_and_symbol() {
        let file = exemption("src/gen/**", None, "2030-01-01");
        assert!(file.covers("src/gen/x.rs", Some("parse")));
        assert!(file.covers("src/gen/x.rs", None));
        assert!(!file.covers("src/lib.rs", Some("parse")));
        let symbol = exemption("src/main.rs", Some("run"), "2030-01-01");
        assert!(symbol.covers("src/main.rs", Some("run")));
        assert!(symbol.covers("src/main.rs", Some("Cli::run")));
        assert!(!symbol.covers("src/main.rs", Some("rerun")));
        assert!(!symbol.covers("src/main.rs", Some("run_all")));
        assert!(!symbol.covers("src/lib.rs", Some("run")));
        assert!(symbol.covers("src/main.rs", None));
    }

    #[test]
    fn a_record_counts_until_its_expiry_day() {
        let record = exemption("src/a.rs", None, "2027-01-01");
        assert!(record.live("2026-12-31"));
        assert!(!record.live("2027-01-01"));
        assert!(!record.live("2027-01-02"));
        assert!(!exemption("src/a.rs", None, "2027-02-30").live("2026-01-01"));
        assert!(!exemption("src/a.rs", None, "soon").live("2026-01-01"));
        assert_eq!(
            exemption("src/a.rs", Some("run"), "2027-01-01").expired_problem("crap"),
            "crap: the exemption for src/a.rs (run) expired on 2027-01-01 (or its date is not a day); \
             renew it with a reason or remove it (RUST_TOPS.md §19)"
        );
        assert_eq!(
            record.expired_problem("coverage"),
            "coverage: the exemption for src/a.rs expired on 2027-01-01 (or its date is not a day); \
             renew it with a reason or remove it (RUST_TOPS.md §19)"
        );
    }
}
