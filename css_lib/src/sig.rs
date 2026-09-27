//! HMAC-SHA256 message signing for the server<->edge command channel (#120/#121).
//!
//! Per-device symmetric keys, established at device registration, authenticate
//! individual messages -- a signed `doors/unlock` command the edge verifies, an
//! inbound `doors/event` the server verifies -- as defence-in-depth atop the
//! broker's per-device authentication and topic ACLs. Even a compromised or
//! misconfigured broker then cannot forge a command the edge will act on, or an
//! event the server will trust, without the device's key.
//!
//! The MAC is computed over a caller-provided canonical byte string, NOT over
//! re-serialized JSON, so it does not depend on serde field ordering surviving a
//! round-trip on either side. Each signed message type builds its own canonical
//! string from its semantic fields (see the `signing_bytes` helpers on the wire
//! types).

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Hex-encoded HMAC-SHA256 of `message` under `key`.
pub fn sign(key: &[u8], message: &[u8]) -> String {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts a key of any length");
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

/// Whether `mac_hex` is a valid HMAC-SHA256 of `message` under `key`. The
/// comparison is constant-time and tolerates a malformed/empty `mac_hex`
/// (returns `false` rather than erroring).
pub fn verify(key: &[u8], message: &[u8], mac_hex: &str) -> bool {
    let expected = sign(key, message);
    // Compare hex encodings in constant time; both are fixed-length and a digest
    // reveals nothing exploitable about the key.
    crate::ct::constant_time_str_eq(&expected, mac_hex)
}

/// Canonical signed message for a `doors/unlock` command (#121). Built
/// identically by the server (before signing) and the edge (before verifying),
/// from the fields that decide the action -- not from re-serialized JSON. Only
/// round-trip-stable field types (UUID string, i32, string), so no datetime or
/// float formatting can differ between the two sides.
pub fn doors_unlock_message(door_id: &str, duration_ms: i32, reason: &str) -> Vec<u8> {
    format!("doors/unlock|{door_id}|{duration_ms}|{reason}").into_bytes()
}

/// Canonical signed message for an inbound `doors/event` (#121). Covers the
/// fields an attacker would forge to fake an unlock in the audit trail
/// (which door, whose card, granted or not, which source); the timestamp and
/// human-readable reason are deliberately excluded so serde round-tripping of a
/// datetime cannot break verification.
pub fn doors_event_message(door_id: &str, card_id: &str, granted: bool, source: &str) -> Vec<u8> {
    format!("doors/event|{door_id}|{card_id}|{granted}|{source}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_round_trips() {
        let key = b"per-device-secret-key";
        let mac = sign(key, b"doors/unlock|door-1|4200|qr");
        assert!(verify(key, b"doors/unlock|door-1|4200|qr", &mac));
    }

    #[test]
    fn a_tampered_message_is_rejected() {
        let key = b"per-device-secret-key";
        let mac = sign(key, b"doors/unlock|door-1|4200|qr");
        // A different duration is a different message, so the MAC must not verify.
        assert!(!verify(key, b"doors/unlock|door-1|9999|qr", &mac));
    }

    #[test]
    fn the_wrong_key_is_rejected() {
        let mac = sign(b"key-A", b"message");
        assert!(!verify(b"key-B", b"message", &mac));
    }

    #[test]
    fn a_garbage_or_empty_mac_is_rejected_not_panicked() {
        assert!(!verify(b"k", b"m", "not-hex-!!"));
        assert!(!verify(b"k", b"m", ""));
    }
}
