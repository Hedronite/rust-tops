//! RS-03 primitive soup: identifiers as validated newtypes.

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

/// A user identifier. Zero is not a valid id, so `Option<UserId>` costs no extra space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserId(NonZeroU64);

/// An organization identifier. A distinct type from [`UserId`], so the two cannot be swapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrgId(NonZeroU64);

/// Why a string did not parse as an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// The text was not a decimal integer.
    NotANumber,
    /// The number was zero.
    Zero,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotANumber => "identifier is not a decimal integer",
            Self::Zero => "identifier is zero",
        })
    }
}

impl std::error::Error for IdError {}

fn parse_nonzero(s: &str) -> Result<NonZeroU64, IdError> {
    let n: u64 = s.parse().map_err(|_| IdError::NotANumber)?;
    NonZeroU64::new(n).ok_or(IdError::Zero)
}

impl FromStr for UserId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_nonzero(s).map(Self)
    }
}

impl FromStr for OrgId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_nonzero(s).map(Self)
    }
}

/// A user's membership of one organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    /// The member.
    pub user: UserId,
    /// The organization.
    pub org: OrgId,
}

/// Grants `user` membership of `org`.
///
/// The argument types carry the order. Swapping them does not compile:
///
/// ```compile_fail,E0308
/// use poire_rust::ids::{grant, OrgId, UserId};
/// let user: UserId = "7".parse().unwrap();
/// let org: OrgId = "9".parse().unwrap();
/// let _ = grant(org, user);
/// ```
#[must_use]
pub fn grant(user: UserId, org: OrgId) -> Membership {
    Membership { user, org }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_once_at_the_boundary() {
        assert_eq!(
            "0".parse::<UserId>(),
            Err(IdError::Zero),
            "zero is not an id"
        );
        assert_eq!(
            "x".parse::<UserId>(),
            Err(IdError::NotANumber),
            "text is not an id"
        );
        let user: UserId = "7".parse().unwrap();
        let org: OrgId = "9".parse().unwrap();
        assert_eq!(
            grant(user, org).user,
            user,
            "grant keeps the user it was given"
        );
    }

    #[test]
    fn errors_say_what_happened() {
        assert_eq!(IdError::Zero.to_string(), "identifier is zero", "zero");
        assert_eq!(
            IdError::NotANumber.to_string(),
            "identifier is not a decimal integer",
            "text"
        );
    }

    #[test]
    fn an_absent_id_costs_nothing() {
        assert_eq!(
            size_of::<Option<UserId>>(),
            size_of::<u64>(),
            "the zero niche holds None"
        );
    }
}
