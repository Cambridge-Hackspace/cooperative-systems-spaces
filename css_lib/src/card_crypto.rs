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
    /// `index_key` and `device_pepper` are the same value.
    ///
    /// Refused rather than tolerated: the pepper is handed to every device and
    /// the index key must never leave the server, so making them equal quietly
    /// hands out the one key whose disclosure makes a dump trivially
    /// enumerable.
    KeysMustDiffer,
    /// The KDF refused its parameters. Not expected at runtime.
    DigestFailed,
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
            Self::KeysMustDiffer => write!(
                f,
                "cards.index_key and cards.device_pepper must be different: the pepper is \
                 distributed to devices and the index key must never leave the server"
            ),
            Self::DigestFailed => write!(f, "the card digest KDF refused its parameters"),
            Self::Undecryptable => write!(
                f,
                "card could not be decrypted: wrong cards.encryption_key, or the row was altered"
            ),
        }
    }
}

impl std::error::Error for CardCryptoError {}

/// Turns a card into the digest a *device* holds for it, and nothing else.
///
/// This type exists to make the separation structural rather than a matter of
/// discipline. An edge needs to hash a swipe; it does not need to decrypt a
/// card or compute a server-side lookup index, and it must not be able to. So
/// the edge is handed one of these -- which holds only the distributed pepper
/// -- and there is no method on it that could do the other two things.
///
/// Holds key material, so it is never `Debug`.
pub struct CardDigester {
    device_pepper: [u8; KEY_LEN],
}

impl CardDigester {
    pub fn from_hex(device_pepper: &str) -> Result<Self, CardCryptoError> {
        Ok(Self {
            device_pepper: decode_key(device_pepper, "device_pepper")?,
        })
    }

    /// The value a *device* holds for one card (#109).
    ///
    /// Devices authorize offline, so they must hold something to compare a
    /// swipe against -- and that used to be the card code itself, in a file on
    /// a plug screwed to a wall. This replaces it.
    ///
    /// **Why this is not [`CardCipher::blind_index`].** That one is HMAC: fast
    /// on purpose, because it is a lookup accelerator and its key never leaves
    /// the server. Sending *it* to devices would mean a stolen plug yields the
    /// index key, and index key plus a database dump recovers every card in the
    /// space at 2^32 HMAC operations -- seconds -- undoing the at-rest work
    /// entirely. So the device-facing digest gets its own pepper and a
    /// deliberately slow KDF.
    ///
    /// **What it costs and buys.** One argon2id invocation per swipe on the
    /// edge, which its latency budget absorbs comfortably. An attacker holding
    /// a device holds the pepper and the digests of that device's own members;
    /// recovering the codes behind them is 2^32 argon2 invocations, a multi-year
    /// campaign, and it differs per deployment.
    ///
    /// **What it does not hide,** so nobody assumes otherwise: the
    /// authorization matrix, the size of the membership, and correlation -- a
    /// digest is a stable identifier, so a compromised device still records
    /// that the same person used these tools at these times, and two
    /// compromised devices still link them across the estate.
    ///
    /// Deterministic by construction: the salt is derived from the pepper
    /// rather than random, because a device must arrive at the same digest
    /// from the card alone. That makes this a pepper-only scheme, which is the
    /// most a scheme can be when the verifier does not know who it is holding
    /// until after the lookup.
    pub fn wire_digest(&self, code: &str) -> Result<Vec<u8>, CardCryptoError> {
        let salt = {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.device_pepper)
                .expect("HMAC accepts any key length");
            mac.update(b"css-card-wire-digest-salt-v1");
            mac.finalize().into_bytes().to_vec()
        };
        let argon = argon2::Argon2::new_with_secret(
            &self.device_pepper,
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::default(),
        )
        .map_err(|_| CardCryptoError::DigestFailed)?;
        let mut out = vec![0u8; 32];
        argon
            .hash_password_into(code.as_bytes(), &salt, &mut out)
            .map_err(|_| CardCryptoError::DigestFailed)?;
        Ok(out)
    }
}

/// Seals and finds card codes. Holds key material, so it is never `Debug`.
pub struct CardCipher {
    cipher: XChaCha20Poly1305,
    index_key: [u8; KEY_LEN],
    digester: CardDigester,
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
    /// Build from three hex-encoded 32-byte keys.
    ///
    /// The third is the **device pepper**, and it is separate from
    /// `index_key` for a reason that matters: the pepper goes out to every
    /// device, and the index key must never leave the server. They protect
    /// different things against different attackers and cannot be the same
    /// value -- see [`Self::wire_digest`].
    pub fn from_hex(
        encryption_key: &str,
        index_key: &str,
        device_pepper: &str,
    ) -> Result<Self, CardCryptoError> {
        let enc = decode_key(encryption_key, "encryption_key")?;
        let idx = decode_key(index_key, "index_key")?;
        let pepper = decode_key(device_pepper, "device_pepper")?;
        if idx == pepper {
            return Err(CardCryptoError::KeysMustDiffer);
        }
        Ok(Self {
            cipher: XChaCha20Poly1305::new(Key::from_slice(&enc)),
            index_key: idx,
            digester: CardDigester {
                device_pepper: pepper,
            },
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

    /// The device-facing digest. Delegates to the pepper-only half.
    pub fn wire_digest(&self, code: &str) -> Result<Vec<u8>, CardCryptoError> {
        self.digester.wire_digest(code)
    }

    /// The pepper-only half, for handing to the device side. There is no way
    /// back from this to the encryption or index keys, which is the point.
    pub fn digester(&self) -> &CardDigester {
        &self.digester
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
    const PEP: &str = "0404040404040404040404040404040404040404040404040404040404040404";

    fn cipher() -> CardCipher {
        CardCipher::from_hex(ENC, IDX, PEP).expect("test keys are well formed")
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
        let a = CardCipher::from_hex(ENC, IDX, PEP).unwrap();
        let b = CardCipher::from_hex(ENC, OTHER, PEP).unwrap();
        assert_ne!(
            a.blind_index("04A2B3C4"),
            b.blind_index("04A2B3C4"),
            "two deployments must not produce the same index for the same card"
        );
    }

    #[test]
    fn the_wrong_key_cannot_open_a_card() {
        let sealed = cipher().seal("04A2B3C4").unwrap();
        let wrong = CardCipher::from_hex(OTHER, IDX, PEP).unwrap();
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
            CardCipher::from_hex("abcd", IDX, PEP)
                .err()
                .expect("short key must be refused")
        );
        assert_eq!(
            CardCryptoError::KeyNotHex { which: "index_key" },
            CardCipher::from_hex(ENC, "not hex at all", PEP)
                .err()
                .expect("non-hex key must be refused")
        );
    }

    /// And the message names which key, because an operator staring at a
    /// refusal to start needs to know which of the two they got wrong.
    #[test]
    fn the_refusal_says_which_key_is_wrong() {
        let e = CardCipher::from_hex("abcd", IDX, PEP)
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("encryption_key"), "unhelpful message: {e}");
        let e = CardCipher::from_hex(ENC, "zz", PEP)
            .err()
            .unwrap()
            .to_string();
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
        assert!(CardCipher::from_hex(&k, &generate_key_hex(), &generate_key_hex()).is_ok());
    }

    // ── the device-facing digest (#109) ──────────────────────────────────

    #[test]
    fn the_wire_digest_is_deterministic() {
        let c = cipher();
        assert_eq!(
            c.wire_digest("04A2B3C4").unwrap(),
            c.wire_digest("04A2B3C4").unwrap(),
            "a device derives this from the card alone, so it must not vary"
        );
    }

    #[test]
    fn different_cards_digest_differently() {
        let c = cipher();
        assert_ne!(
            c.wire_digest("04A2B3C4").unwrap(),
            c.wire_digest("04A2B3C5").unwrap()
        );
    }

    #[test]
    fn the_wire_digest_depends_on_the_pepper() {
        let a = CardCipher::from_hex(ENC, IDX, PEP).unwrap();
        let b = CardCipher::from_hex(ENC, IDX, OTHER).unwrap();
        assert_ne!(
            a.wire_digest("04A2B3C4").unwrap(),
            b.wire_digest("04A2B3C4").unwrap(),
            "two deployments must not produce the same device-facing digest"
        );
    }

    /// The property the whole separation exists for. If these ever coincide,
    /// the value handed to every device is the one derived with the key that
    /// must never leave the server.
    #[test]
    fn the_wire_digest_is_not_the_blind_index() {
        let c = cipher();
        assert_ne!(
            c.wire_digest("04A2B3C4").unwrap(),
            c.blind_index("04A2B3C4"),
            "the device-facing digest must not equal the server-only index"
        );
    }

    /// And the keys behind them must not be made equal by configuration, which
    /// would achieve the same collapse from the outside.
    #[test]
    fn an_index_key_reused_as_the_pepper_is_refused() {
        assert_eq!(
            CardCryptoError::KeysMustDiffer,
            CardCipher::from_hex(ENC, IDX, IDX)
                .err()
                .expect("reusing the index key as the device pepper must be refused")
        );
    }

    /// Slow on purpose. Not a timing assertion -- those are flaky -- but a
    /// statement that the digest is not the same primitive as the index: if
    /// somebody swaps argon2 for the HMAC to make syncs cheaper, the digest
    /// becomes 2^32-enumerable in seconds by anyone holding a device, and this
    /// is the test that notices.
    #[test]
    fn the_wire_digest_is_not_a_bare_hmac_of_the_code() {
        let c = cipher();
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&c.digester.device_pepper).unwrap();
        mac.update(b"04A2B3C4");
        assert_ne!(
            c.wire_digest("04A2B3C4").unwrap(),
            mac.finalize().into_bytes().to_vec(),
            "the device-facing digest must be the slow KDF, not a fast keyed hash"
        );
    }

    /// The edge holds a `CardDigester` and the server a `CardCipher`; if they
    /// ever disagreed, every offline swipe would be refused while every online
    /// one worked -- a failure that looks like a broken reader.
    #[test]
    fn the_pepper_only_digester_agrees_with_the_full_cipher() {
        let c = cipher();
        let d = CardDigester::from_hex(PEP).expect("pepper parses");
        assert_eq!(
            c.wire_digest("04A2B3C4").unwrap(),
            d.wire_digest("04A2B3C4").unwrap(),
            "server and device must derive the same digest from the same card"
        );
        assert_eq!(
            c.digester().wire_digest("04A2B3C4").unwrap(),
            d.wire_digest("04A2B3C4").unwrap()
        );
    }

    #[test]
    fn two_generated_keys_differ() {
        assert_ne!(generate_key_hex(), generate_key_hex());
    }
}
