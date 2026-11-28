//! RS-07 timing-leaky tag comparison, and RS-03 a typestate for verified payloads.

use std::fmt;
use std::marker::PhantomData;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// A webhook signing key, checked once when it is loaded.
///
/// The secret is wiped from memory when the key is dropped. The MAC state built
/// from it during [`Payload::verify`] is not: `hmac` 0.13 does not wipe `Hmac`
/// on drop, so that state lives only for the length of one call.
#[derive(Clone)]
pub struct WebhookKey(Zeroizing<Box<[u8]>>);

impl fmt::Debug for WebhookKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebhookKey(..)")
    }
}

/// Why a key could not be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    /// The key was empty.
    Empty,
    /// The MAC rejected the key.
    Rejected,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "webhook key is empty",
            Self::Rejected => "webhook key was rejected by HMAC-SHA256",
        })
    }
}

impl std::error::Error for KeyError {}

impl WebhookKey {
    /// Prepares an HMAC-SHA256 key.
    ///
    /// # Errors
    /// [`KeyError::Empty`] for an empty key, [`KeyError::Rejected`] if the MAC refuses it.
    pub fn new(secret: &[u8]) -> Result<Self, KeyError> {
        if secret.is_empty() {
            return Err(KeyError::Empty);
        }
        HmacSha256::new_from_slice(secret).map_err(|_| KeyError::Rejected)?;
        Ok(Self(Zeroizing::new(secret.into())))
    }

    fn mac(&self) -> Result<HmacSha256, BadSignature> {
        // `new` already accepted this secret, and HMAC takes any length, so this
        // cannot fail. If it ever did, refusing the payload is the safe answer.
        HmacSha256::new_from_slice(&self.0).map_err(|_| BadSignature)
    }
}

/// Marker: the payload's signature has not been checked.
#[derive(Debug)]
pub struct Unverified;

/// Marker: the payload's signature matched.
#[derive(Debug)]
pub struct Verified;

/// A webhook body, tagged in its type with whether its signature was checked.
#[derive(Debug)]
pub struct Payload<State = Unverified> {
    body: Box<[u8]>,
    state: PhantomData<State>,
}

/// The tag did not match the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadSignature;

impl fmt::Display for BadSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("webhook signature does not match")
    }
}

impl std::error::Error for BadSignature {}

impl Payload<Unverified> {
    /// Wraps a body that has not been checked.
    #[must_use]
    pub fn new(body: Vec<u8>) -> Self {
        Self {
            body: body.into_boxed_slice(),
            state: PhantomData,
        }
    }

    /// Checks the HMAC-SHA256 tag over the body.
    ///
    /// The comparison runs in constant time (`verify_slice`), so the time it
    /// takes says nothing about how many leading bytes of `tag` were right.
    ///
    /// # Errors
    /// [`BadSignature`] when the tag does not match, whatever its length.
    pub fn verify(self, key: &WebhookKey, tag: &[u8]) -> Result<Payload<Verified>, BadSignature> {
        let mut mac = key.mac()?;
        mac.update(&self.body);
        mac.verify_slice(tag).map_err(|_| BadSignature)?;
        Ok(Payload {
            body: self.body,
            state: PhantomData,
        })
    }
}

impl Payload<Verified> {
    /// The body. Only a verified payload has this method:
    ///
    /// ```compile_fail,E0599
    /// use poire_rust::webhook::Payload;
    /// let raw = Payload::new(b"{}".to_vec());
    /// let _ = raw.body();
    /// ```
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(secret: &[u8], body: &[u8]) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(body);
        mac.finalize().into_bytes().to_vec()
    }

    #[test]
    fn accepts_the_right_tag_only() {
        let key = WebhookKey::new(b"k3y").unwrap();
        let good = tag(b"k3y", b"{\"ok\":1}");

        let verified = Payload::new(b"{\"ok\":1}".to_vec()).verify(&key, &good);
        assert_eq!(
            verified.map(|p| p.body().to_vec()),
            Ok(b"{\"ok\":1}".to_vec()),
            "the right tag verifies"
        );

        let tampered = Payload::new(b"{\"ok\":2}".to_vec()).verify(&key, &good);
        assert!(tampered.is_err(), "a changed body fails");

        let short = good.get(..8).unwrap();
        let truncated = Payload::new(b"{\"ok\":1}".to_vec()).verify(&key, short);
        assert!(truncated.is_err(), "a truncated tag fails");
    }

    #[test]
    fn the_secret_never_reaches_a_log() {
        let key = WebhookKey::new(b"hunter2").unwrap();
        assert_eq!(
            format!("{key:?}"),
            "WebhookKey(..)",
            "Debug hides the secret"
        );
        assert_eq!(
            BadSignature.to_string(),
            "webhook signature does not match",
            "tag"
        );
        assert_eq!(KeyError::Empty.to_string(), "webhook key is empty", "empty");
        assert_eq!(
            KeyError::Rejected.to_string(),
            "webhook key was rejected by HMAC-SHA256",
            "rejected"
        );
    }

    #[test]
    fn rejects_an_empty_key() {
        assert_eq!(
            WebhookKey::new(b"").map(|_| ()),
            Err(KeyError::Empty),
            "an empty key is refused at load"
        );
    }

    #[test]
    fn the_secret_is_wiped_on_drop() {
        fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
        let key = WebhookKey::new(b"k3y").unwrap();
        wiped_on_drop(&key.0);
    }
}
