//! Card codes encrypted at rest, and findable anyway (#108).
//!
//! `user_cards.code` was a plaintext column, so every card in the space was
//! readable from a database dump — and this deployment ships nightly
//! age-encrypted dumps to a NAS, which means every card identifier exists in
//! fourteen retained copies at any time, their confidentiality resting entirely
//! on custody of the age keys.
//!
//! **What this is not.** Not a defence against cloning. The platform
//! deliberately accepts whatever card a member already has — that is a product
//! requirement, not an oversight — so UID-only cards are in scope and a UID is
//! readable by anyone standing near the holder with a cheap reader. The card
//! identifier is not a secret and cannot be made one. This is about disclosure
//! of *the store*: a leaked dump, a restored copy on a laptop, a support
//! export, a future read-only replica.
//!
//! **Why encryption and not a hash.** The obvious move is argon2 with a pepper,
//! and it is wrong here, because the platform needs the value *back*:
//!
//! * `GET /api/toolguard/sync` sends card values to devices so they can
//!   authorize offline — which `FIRMWARE.md` states as the reason the
//!   allow-list is cached at all;
//! * the admin roster displays a card so somebody can tell which physical
//!   credential they are revoking.
//!
//! A digest serves neither. Keeping offline verification with a hash means
//! shipping the pepper to every device, and the pepper is the single thing
//! making a 4-byte UID's 2^32 expensive — so a plug unscrewed from a wall would
//! hand over the secret protecting the whole database. That trades a
//! dump-disclosure risk for a device-theft risk, against hardware that lives in
//! a workshop.
//!
//! So: reversible, with the key outside the database. A dump on its own
//! discloses nothing. **A live server compromise still discloses everything,**
//! which a hash would have limited; that is the trade this makes, and it is the
//! right one for a threat model whose subject is the store rather than the
//! server.
//!
//! **Why a blind index.** Authenticated encryption is non-deterministic by
//! construction — the same card sealed twice gives different ciphertext — so
//! `resolve_card` cannot select by it. A keyed, deterministic digest stored
//! alongside restores the single-lookup path. It is HMAC-SHA256 and not a slow
//! KDF on purpose: slowing it would only help an attacker who held the index
//! key but *not* the encryption key, and both live in the same configuration,
//! so that attacker does not exist. If the keys are ever split across custody
//! boundaries, revisit this.

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, Key, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Bytes of key material each purpose needs.
pub const KEY_LEN: usize = 32;
/// XChaCha20's nonce width.
pub const NONCE_LEN: usize = 24;

/// What can go wrong, kept narrow so callers cannot accidentally log a code.
#[derive(Debug, PartialEq, Eq)]
pub enum CardCryptoError {
    /// A key was not exactly [`KEY_LEN`] bytes after hex decoding.
    BadKeyLength { which: &'static str, got: usize },
    /// A key was not valid hex.
    KeyNotHex { which: &'static str },
    /// The stored nonce was not [`NONCE_LEN`] bytes.
    BadNonceLength { got: usize },
    /// Decryption failed: wrong key, or the row was tampered with.
    ///
    /// Deliberately carries nothing. The plaintext is a person's credential and
    /// the ciphertext is the evidence of tampering; neither belongs in an error
    /// that will be formatted into a log line.
    Undecryptable,
}

impl std::fmt::Display for CardCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadKeyLength { which, got } => write!(
                f,
                "cards.{which} must be {KEY_LEN} bytes ({} hex characters), got {got}",
                KEY_LEN * 2
            ),
            Self::KeyNotHex { which } => write!(f, "cards.{which} is not valid hex"),
            Self::BadNonceLength { got } => {
                write!(f, "stored nonce must be {NONCE_LEN} bytes, got {got}")
            }
            Self::Undecryptable => write!(
                f,
                "card could not be decrypted: wrong cards.encryption_key, or the row was altered"
            ),
        }
    }
}

impl std::error::Error for CardCryptoError {}

/// Seals and finds card codes. Holds key material, so it is never `Debug`.
pub struct CardCipher {
    cipher: XChaCha20Poly1305,
    index_key: [u8; KEY_LEN],
}

/// One sealed card, as the two columns it occupies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedCard {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    /// Deterministic, keyed, indexable. This is what `resolve_card` selects on.
    pub blind_index: Vec<u8>,
}

impl CardCipher {
    /// Build from two hex-encoded 32-byte keys.
    pub fn from_hex(encryption_key: &str, index_key: &str) -> Result<Self, CardCryptoError> {
        let enc = decode_key(encryption_key, "encryption_key")?;
        let idx = decode_key(index_key, "index_key")?;
        Ok(Self {
            cipher: XChaCha20Poly1305::new(Key::from_slice(&enc)),
            index_key: idx,
        })
    }

    /// Encrypt a card code and compute its blind index.
    ///
    /// A fresh random nonce every time, which is why two seals of the same card
    /// differ — and exactly why the blind index has to exist.
    pub fn seal(&self, code: &str) -> Result<SealedCard, CardCryptoError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, code.as_bytes())
            .map_err(|_| CardCryptoError::Undecryptable)?;
        Ok(SealedCard {
            ciphertext,
            nonce: nonce.to_vec(),
            blind_index: self.blind_index(code),
        })
    }

    /// Recover a card code from its two columns.
    pub fn open(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<String, CardCryptoError> {
        if nonce.len() != NONCE_LEN {
            return Err(CardCryptoError::BadNonceLength { got: nonce.len() });
        }
        let plaintext = self
            .cipher
            .decrypt(XNonce::from_slice(nonce), ciphertext)
            .map_err(|_| CardCryptoError::Undecryptable)?;
        String::from_utf8(plaintext).map_err(|_| CardCryptoError::Undecryptable)
    }

    /// The deterministic lookup value for a presented code.
    ///
    /// Byte-exact over the code as given: the column it replaces was matched
    /// with SQL equality, and quietly folding case or trimming here would
    /// change which cards open which doors, which is not a change to smuggle
    /// into an encryption patch.
    pub fn blind_index(&self, code: &str) -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.index_key)
            .expect("HMAC accepts any key length");
        mac.update(code.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

fn decode_key(hex_key: &str, which: &'static str) -> Result<[u8; KEY_LEN], CardCryptoError> {
    let raw = hex::decode(hex_key.trim()).map_err(|_| CardCryptoError::KeyNotHex { which })?;
    if raw.len() != KEY_LEN {
        return Err(CardCryptoError::BadKeyLength {
            which,
            got: raw.len(),
        });
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&raw);
    Ok(out)
}

/// A fresh random key, hex-encoded, for `css-cli` to print when an operator is
/// setting a deployment up.
pub fn generate_key_hex() -> String {
    use rand::RngCore;
    let mut key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    hex::encode(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENC: &str = "0101010101010101010101010101010101010101010101010101010101010101";
    const IDX: &str = "0202020202020202020202020202020202020202020202020202020202020202";
    const OTHER: &str = "0303030303030303030303030303030303030303030303030303030303030303";

    fn cipher() -> CardCipher {
        CardCipher::from_hex(ENC, IDX).expect("test keys are well formed")
    }

    #[test]
    fn a_sealed_card_comes_back_unchanged() {
        let c = cipher();
        let sealed = c.seal("04A2B3C4").unwrap();
        assert_eq!(
            "04A2B3C4",
            c.open(&sealed.ciphertext, &sealed.nonce).unwrap()
        );
    }

    /// The property that forces the blind index to exist. If this ever passes
    /// as equality, the cipher has become deterministic and is leaking which
    /// members share a card value straight out of a dump.
    #[test]
    fn the_same_card_seals_differently_every_time() {
        let c = cipher();
        let a = c.seal("04A2B3C4").unwrap();
        let b = c.seal("04A2B3C4").unwrap();
        assert_ne!(
            a.ciphertext, b.ciphertext,
            "ciphertext must not repeat, or a dump reveals which rows hold the same card"
        );
        assert_ne!(a.nonce, b.nonce, "nonces must not repeat");
    }

    /// And the property that makes lookup possible at all.
    #[test]
    fn the_blind_index_is_stable_across_seals() {
        let c = cipher();
        let a = c.seal("04A2B3C4").unwrap();
        let b = c.seal("04A2B3C4").unwrap();
        assert_eq!(
            a.blind_index, b.blind_index,
            "the same card must always index to the same value, or resolve_card cannot find it"
        );
        assert_eq!(a.blind_index, c.blind_index("04A2B3C4"));
    }

    #[test]
    fn different_cards_index_differently() {
        let c = cipher();
        assert_ne!(c.blind_index("04A2B3C4"), c.blind_index("04A2B3C5"));
    }

    /// The index is keyed, not a bare digest. An attacker holding a dump and a
    /// dictionary of candidate UIDs must also hold the key; a plain SHA-256
    /// would be enumerable offline in seconds against 2^32.
    #[test]
    fn the_blind_index_depends_on_the_key() {
        let a = CardCipher::from_hex(ENC, IDX).unwrap();
        let b = CardCipher::from_hex(ENC, OTHER).unwrap();
        assert_ne!(
            a.blind_index("04A2B3C4"),
            b.blind_index("04A2B3C4"),
            "two deployments must not produce the same index for the same card"
        );
    }

    #[test]
    fn the_wrong_key_cannot_open_a_card() {
        let sealed = cipher().seal("04A2B3C4").unwrap();
        let wrong = CardCipher::from_hex(OTHER, IDX).unwrap();
        assert_eq!(
            Err(CardCryptoError::Undecryptable),
            wrong.open(&sealed.ciphertext, &sealed.nonce)
        );
    }

    /// Authenticated, not merely encrypted: a flipped bit must be refused
    /// rather than decrypted into a different card that might match somebody.
    #[test]
    fn a_tampered_ciphertext_is_refused_rather_than_decrypted() {
        let c = cipher();
        let mut sealed = c.seal("04A2B3C4").unwrap();
        sealed.ciphertext[0] ^= 0x01;
        assert_eq!(
            Err(CardCryptoError::Undecryptable),
            c.open(&sealed.ciphertext, &sealed.nonce)
        );
    }

    #[test]
    fn a_truncated_nonce_is_refused_by_length_rather_than_by_luck() {
        let c = cipher();
        let sealed = c.seal("04A2B3C4").unwrap();
        assert_eq!(
            Err(CardCryptoError::BadNonceLength { got: 4 }),
            c.open(&sealed.ciphertext, &sealed.nonce[..4])
        );
    }

    #[test]
    fn keys_are_validated_rather_than_padded() {
        assert_eq!(
            CardCryptoError::BadKeyLength {
                which: "encryption_key",
                got: 2,
            },
            CardCipher::from_hex("abcd", IDX)
                .err()
                .expect("short key must be refused")
        );
        assert_eq!(
            CardCryptoError::KeyNotHex { which: "index_key" },
            CardCipher::from_hex(ENC, "not hex at all")
                .err()
                .expect("non-hex key must be refused")
        );
    }

    /// And the message names which key, because an operator staring at a
    /// refusal to start needs to know which of the two they got wrong.
    #[test]
    fn the_refusal_says_which_key_is_wrong() {
        let e = CardCipher::from_hex("abcd", IDX).err().unwrap().to_string();
        assert!(e.contains("encryption_key"), "unhelpful message: {e}");
        let e = CardCipher::from_hex(ENC, "zz").err().unwrap().to_string();
        assert!(e.contains("index_key"), "unhelpful message: {e}");
    }

    /// Codes are matched byte-exactly, as the plaintext column was. Folding
    /// case here would silently change which cards open which tools.
    #[test]
    fn matching_is_byte_exact_and_not_quietly_normalised() {
        let c = cipher();
        assert_ne!(c.blind_index("04a2b3c4"), c.blind_index("04A2B3C4"));
        assert_ne!(c.blind_index(" 04A2B3C4"), c.blind_index("04A2B3C4"));
    }

    #[test]
    fn a_generated_key_is_accepted_by_the_thing_that_will_use_it() {
        let k = generate_key_hex();
        assert_eq!(KEY_LEN * 2, k.len());
        assert!(CardCipher::from_hex(&k, &generate_key_hex()).is_ok());
    }

    #[test]
    fn two_generated_keys_differ() {
        assert_ne!(generate_key_hex(), generate_key_hex());
    }
}
