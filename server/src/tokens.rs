//! Generation and hashing for single-use account tokens.
//!
//! Shared by password reset and email confirmation, which keep separate tables
//! but have no reason to generate their secrets differently.
//!
//! ## Why SHA-256 and not Argon2
//!
//! `user_mfa_recovery_codes` stores an Argon2 hash, and copying that here would
//! be a mistake worth explaining rather than a style choice.
//!
//! A KDF exists to make a *low-entropy* secret expensive to guess. A recovery
//! code is twelve characters from a 32-symbol alphabet, chosen so a human can
//! read it off paper; slowing an attacker down is exactly the point. A token
//! here is 256 bits from the operating system's CSPRNG. There is no dictionary,
//! no keyspace an attacker can walk, and nothing for a KDF to slow.
//!
//! The salted hash also actively costs something. A salted digest cannot be
//! looked up by value, which is why consuming a recovery code is an O(n) verify
//! loop over every unused row for that user (`api/mfa.rs:570-599`). That is
//! tolerable when n is ten and the user is already identified by their session.
//! Neither holds here: a reset token arrives with no user attached, so
//! lookup-by-value is the entire mechanism, and the scan would be over every
//! live token in the instance.
//!
//! What hashing buys, and the only thing it needs to buy, is that a database
//! read -- a backup, a stray `SELECT`, a log spill -- does not hand the reader
//! a working reset link for every pending request. SHA-256 gives that
//! completely.
//!
//! A `selector.verifier` split is not used, and would not help. Its purpose is
//! to permit a slow hash while keeping lookup indexed. With a fast hash the
//! hash column *is* the index, so the split buys nothing and adds a parsing
//! step and a new failure mode.

use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

/// Bytes of entropy in a token. 256 bits, matching the digest that stores it.
const TOKEN_BYTES: usize = 32;

/// How long a password reset link is good for.
///
/// A module constant rather than a config field on purpose: adding one means
/// touching `config.sample.toml`, `e2e/stack-config.toml`, the tracked-config
/// check and the `PublicConfig` question, for a number no operator has asked to
/// change. If one does, it is a small follow-up.
pub const RESET_TOKEN_TTL_MINUTES: i64 = 60;

/// How long an email confirmation link is good for. Longer than a reset,
/// because confirming an address is not urgent and the mail may sit unread.
pub const VERIFICATION_TOKEN_TTL_HOURS: i64 = 24;

/// A fresh token: the secret to put in the email, and the digest to store.
///
/// Returned as a pair so a caller cannot accidentally store the secret -- there
/// is no method that turns one back into the other, and the plaintext is never
/// written anywhere but the message body.
pub fn generate_token() -> (String, String) {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let plaintext = hex::encode(bytes);
    let digest = hash_token(&plaintext);
    (plaintext, digest)
}

/// The stored form of a token: lowercase hex SHA-256.
pub fn hash_token(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_token_is_sixty_four_hex_characters() {
        let (plaintext, _) = generate_token();
        assert_eq!(plaintext.len(), TOKEN_BYTES * 2);
        assert!(
            plaintext.chars().all(|c| c.is_ascii_hexdigit()),
            "a token must survive a URL and an email client without encoding: {plaintext}"
        );
    }

    #[test]
    fn tokens_do_not_repeat() {
        // Not a serious test of the CSPRNG -- it is a test that we are calling
        // one at all. A constant, a counter, or a seeded generator reused
        // across calls all show up here immediately.
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            assert!(
                seen.insert(generate_token().0),
                "a token repeated within 1000 draws"
            );
        }
    }

    #[test]
    fn the_stored_form_is_not_the_token() {
        // The whole security property of the table. If these are ever equal,
        // reading the database is reading the reset links.
        let (plaintext, digest) = generate_token();
        assert_ne!(plaintext, digest);
        assert!(!digest.contains(&plaintext));
        assert_eq!(digest.len(), 64);
    }

    #[test]
    fn hashing_is_stable_and_distinguishing() {
        // Stable: a token emailed now must still match its row later.
        // Distinguishing: two tokens must not collapse to one row.
        assert_eq!(hash_token("abc"), hash_token("abc"));
        assert_ne!(hash_token("abc"), hash_token("abd"));
    }

    #[test]
    fn the_digest_matches_the_token_it_was_made_from() {
        // Guards the pair: a `generate_token` that returned a digest of
        // something else would satisfy every test above and be unusable.
        let (plaintext, digest) = generate_token();
        assert_eq!(hash_token(&plaintext), digest);
    }

    #[test]
    fn the_known_answer_is_the_real_sha256() {
        // A published vector, so a future refactor cannot quietly swap the
        // digest for something weaker while every self-consistency test above
        // keeps passing.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
