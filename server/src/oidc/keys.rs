//! Signing keys for CS:S as an OpenID Connect provider (#225).
//!
//! RS256, because OpenID Connect Discovery requires every provider to offer
//! it and every relying-party library accepts it. Keys are generated and used
//! through `aws-lc-rs` (already in the tree through rustls), whose RSA private
//! operations are constant-time; the `rsa` crate's are not.
//!
//! A key's private half is stored sealed: ChaCha20-Poly1305 under a 32-byte key
//! from configuration, or derived from `auth.jwt_secret` when none is given. A
//! row this server can't open (the secret changed) is treated as absent, and a
//! new key is made, which ends every token signed with the old one -- the same
//! consequence as rotating the session secret.
//!
//! This file depends on nothing else in the crate, so it can be compiled and
//! tested on its own.

use aws_lc_rs::encoding::AsDer;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{self, KeyPair as _, RsaKeyPair, RsaPublicKeyComponents};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use serde_json::{json, Value};

/// The JOSE algorithm every key here signs with.
pub const ALG: &str = "RS256";

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    /// Key generation or signing failed inside the crypto library.
    Crypto,
    /// A sealed key could not be opened: wrong sealing key, or corrupt.
    Sealed,
    /// A token was not a well-formed JWT.
    Malformed,
    /// A token's signature does not verify against the named key.
    BadSignature,
    /// A token names a key that isn't published.
    UnknownKey,
}

/// A signing key with its id.
pub struct SigningKey {
    pub kid: String,
    pair: RsaKeyPair,
}

impl SigningKey {
    /// A fresh 2048-bit key. The id is the RFC 7638 thumbprint of the public
    /// key, so it is derived from the key rather than chosen.
    pub fn generate() -> Result<Self, KeyError> {
        let pair = RsaKeyPair::generate(KeySize::Rsa2048).map_err(|_| KeyError::Crypto)?;
        Ok(Self::from_pair(pair))
    }

    fn from_pair(pair: RsaKeyPair) -> Self {
        let kid = thumbprint(&public_jwk_parts(&pair));
        SigningKey { kid, pair }
    }

    /// The key's private half, PKCS#8 DER, sealed under `sealing_key`.
    pub fn seal(&self, sealing_key: &[u8; 32]) -> Result<String, KeyError> {
        let der = self.pair.as_der().map_err(|_| KeyError::Crypto)?;
        let cipher = ChaCha20Poly1305::new(sealing_key.into());
        let mut nonce = [0u8; 12];
        aws_lc_rs::rand::fill(&mut nonce).map_err(|_| KeyError::Crypto)?;
        let sealed = cipher
            .encrypt(Nonce::from_slice(&nonce), der.as_ref())
            .map_err(|_| KeyError::Crypto)?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Ok(B64.encode(out))
    }

    /// Open a key sealed by [`SigningKey::seal`].
    pub fn unseal(sealed: &str, sealing_key: &[u8; 32]) -> Result<Self, KeyError> {
        let bytes = B64.decode(sealed).map_err(|_| KeyError::Sealed)?;
        if bytes.len() < 12 + 16 {
            return Err(KeyError::Sealed);
        }
        let (nonce, body) = bytes.split_at(12);
        let cipher = ChaCha20Poly1305::new(sealing_key.into());
        let der = cipher
            .decrypt(Nonce::from_slice(nonce), body)
            .map_err(|_| KeyError::Sealed)?;
        let pair = RsaKeyPair::from_pkcs8(&der).map_err(|_| KeyError::Sealed)?;
        Ok(Self::from_pair(pair))
    }

    /// The public key as a JWK, for the JWKS document.
    pub fn public_jwk(&self) -> Value {
        let (n, e) = public_jwk_parts(&self.pair);
        json!({ "kty": "RSA", "use": "sig", "alg": ALG, "kid": self.kid, "n": n, "e": e })
    }

    /// Sign `claims` as a compact JWS with this key's id in the header.
    pub fn sign(&self, claims: &Value) -> Result<String, KeyError> {
        let header = json!({ "alg": ALG, "typ": "JWT", "kid": self.kid });
        let signing_input = format!(
            "{}.{}",
            B64.encode(serde_json::to_vec(&header).map_err(|_| KeyError::Crypto)?),
            B64.encode(serde_json::to_vec(claims).map_err(|_| KeyError::Crypto)?)
        );
        let mut sig = vec![0u8; self.pair.public_modulus_len()];
        self.pair
            .sign(
                &signature::RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signing_input.as_bytes(),
                &mut sig,
            )
            .map_err(|_| KeyError::Crypto)?;
        Ok(format!("{signing_input}.{}", B64.encode(sig)))
    }
}

fn public_jwk_parts(pair: &RsaKeyPair) -> (String, String) {
    let public = pair.public_key();
    (
        B64.encode(public.modulus().big_endian_without_leading_zero()),
        B64.encode(public.exponent().big_endian_without_leading_zero()),
    )
}

/// RFC 7638: SHA-256 over the required members in lexicographic order.
fn thumbprint((n, e): &(String, String)) -> String {
    let canonical = format!(r#"{{"e":"{e}","kty":"RSA","n":"{n}"}}"#);
    B64.encode(aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA256,
        canonical.as_bytes(),
    ))
}

/// The key id a compact JWS names in its header, without verifying anything.
pub fn kid_of(token: &str) -> Result<String, KeyError> {
    let header = token.split('.').next().ok_or(KeyError::Malformed)?;
    let header: Value =
        serde_json::from_slice(&B64.decode(header).map_err(|_| KeyError::Malformed)?)
            .map_err(|_| KeyError::Malformed)?;
    if header.get("alg").and_then(Value::as_str) != Some(ALG) {
        return Err(KeyError::Malformed);
    }
    header
        .get("kid")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(KeyError::Malformed)
}

/// Verify a compact JWS against one of the `published` JWKs and return its
/// claims. Expiry, issuer and audience are the caller's to check: this answers
/// only "did one of our published keys sign exactly this".
pub fn verify(token: &str, published: &[Value]) -> Result<Value, KeyError> {
    let kid = kid_of(token)?;
    let Some(jwk) = published
        .iter()
        .find(|j| j.get("kid").and_then(Value::as_str) == Some(kid.as_str()))
    else {
        return Err(KeyError::UnknownKey);
    };
    let component = |name: &str| {
        jwk.get(name)
            .and_then(Value::as_str)
            .and_then(|v| B64.decode(v).ok())
            .ok_or(KeyError::Malformed)
    };
    let public = RsaPublicKeyComponents {
        n: component("n")?,
        e: component("e")?,
    };
    let mut parts = token.rsplitn(2, '.');
    let sig = parts.next().ok_or(KeyError::Malformed)?;
    let signing_input = parts.next().ok_or(KeyError::Malformed)?;
    let sig = B64.decode(sig).map_err(|_| KeyError::Malformed)?;
    public
        .verify(
            &signature::RSA_PKCS1_2048_8192_SHA256,
            signing_input.as_bytes(),
            &sig,
        )
        .map_err(|_| KeyError::BadSignature)?;
    let payload = signing_input.split('.').nth(1).ok_or(KeyError::Malformed)?;
    serde_json::from_slice(&B64.decode(payload).map_err(|_| KeyError::Malformed)?)
        .map_err(|_| KeyError::Malformed)
}

/// The sealing key: the configured one, or one derived from the session
/// secret so a deployment that sets nothing still seals its keys.
pub fn sealing_key(configured_hex: Option<&str>, jwt_secret: &str) -> Result<[u8; 32], String> {
    if let Some(hex_key) = configured_hex {
        let bytes = hex::decode(hex_key.trim())
            .map_err(|_| "oidc.key_encryption_key must be hex".to_string())?;
        return bytes.try_into().map_err(|_| {
            "oidc.key_encryption_key must be 32 bytes (64 hex characters)".to_string()
        });
    }
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, jwt_secret.as_bytes());
    let tag = aws_lc_rs::hmac::sign(&key, b"css-oidc-signing-key-seal/v1");
    let mut out = [0u8; 32];
    out.copy_from_slice(tag.as_ref());
    Ok(out)
}

/// Which keys a JWKS publishes, as of `now`: every key not yet retired, and
/// every key retired less than `keep_secs` ago, so a token signed just before
/// a rotation still verifies until it expires. `keys` is (kid, retired_at in
/// unix seconds). Pure, so rotation is testable without a database.
pub fn published_kids(keys: &[(String, Option<i64>)], now: i64, keep_secs: i64) -> Vec<String> {
    keys.iter()
        .filter(|(_, retired)| retired.is_none_or(|at| now - at < keep_secs))
        .map(|(kid, _)| kid.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEAL: [u8; 32] = [7u8; 32];

    #[test]
    fn a_signed_token_verifies_against_its_own_key() {
        let k = SigningKey::generate().unwrap();
        let t = k.sign(&json!({ "sub": "a" })).unwrap();
        let claims = verify(&t, &[k.public_jwk()]).unwrap();
        assert_eq!(claims["sub"], "a");
    }

    #[test]
    fn a_tampered_payload_does_not_verify() {
        let k = SigningKey::generate().unwrap();
        let t = k.sign(&json!({ "sub": "a" })).unwrap();
        let mut parts: Vec<&str> = t.split('.').collect();
        let forged = B64.encode(br#"{"sub":"b"}"#);
        parts[1] = &forged;
        let forged = parts.join(".");
        assert_eq!(
            verify(&forged, &[k.public_jwk()]),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn a_token_from_an_unpublished_key_is_refused() {
        let ours = SigningKey::generate().unwrap();
        let theirs = SigningKey::generate().unwrap();
        let t = theirs.sign(&json!({ "sub": "a" })).unwrap();
        assert_eq!(verify(&t, &[ours.public_jwk()]), Err(KeyError::UnknownKey));
        // And naming our kid doesn't help: the signature is checked.
        let header = B64.encode(format!(
            r#"{{"alg":"RS256","typ":"JWT","kid":"{}"}}"#,
            ours.kid
        ));
        let mut parts: Vec<String> = t.split('.').map(str::to_string).collect();
        parts[0] = header;
        assert_eq!(
            verify(&parts.join("."), &[ours.public_jwk()]),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn a_token_with_another_algorithm_is_malformed() {
        let k = SigningKey::generate().unwrap();
        let t = k.sign(&json!({})).unwrap();
        let mut parts: Vec<String> = t.split('.').map(str::to_string).collect();
        parts[0] = B64.encode(format!(r#"{{"alg":"none","kid":"{}"}}"#, k.kid));
        assert_eq!(
            verify(&parts.join("."), &[k.public_jwk()]),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn a_sealed_key_round_trips_and_keeps_its_id() {
        let k = SigningKey::generate().unwrap();
        let back = SigningKey::unseal(&k.seal(&SEAL).unwrap(), &SEAL).unwrap();
        assert_eq!(back.kid, k.kid);
        let t = back.sign(&json!({ "x": 1 })).unwrap();
        assert!(verify(&t, &[k.public_jwk()]).is_ok());
    }

    #[test]
    fn a_sealed_key_does_not_open_under_another_key() {
        let k = SigningKey::generate().unwrap();
        let sealed = k.seal(&SEAL).unwrap();
        assert!(matches!(
            SigningKey::unseal(&sealed, &[8u8; 32]),
            Err(KeyError::Sealed)
        ));
    }

    #[test]
    fn the_jwk_carries_the_kid_and_rs256() {
        let k = SigningKey::generate().unwrap();
        let jwk = k.public_jwk();
        assert_eq!(jwk["kid"], k.kid.as_str());
        assert_eq!(jwk["alg"], "RS256");
        assert_eq!(jwk["e"], "AQAB");
    }

    #[test]
    fn a_retired_key_stays_published_until_its_tokens_have_expired() {
        let keys = vec![("new".to_string(), None), ("old".to_string(), Some(1_000))];
        assert_eq!(
            published_kids(&keys, 1_000 + 3_599, 3_600),
            vec!["new", "old"]
        );
        assert_eq!(published_kids(&keys, 1_000 + 3_600, 3_600), vec!["new"]);
    }

    #[test]
    fn the_derived_sealing_key_follows_the_secret() {
        let a = sealing_key(None, "one").unwrap();
        assert_eq!(a, sealing_key(None, "one").unwrap());
        assert_ne!(a, sealing_key(None, "two").unwrap());
        assert!(sealing_key(Some("00"), "x").is_err());
        assert_eq!(
            sealing_key(Some(&"ab".repeat(32)), "x").unwrap(),
            [0xab; 32]
        );
    }
}
