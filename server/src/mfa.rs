//! Multi-factor authentication.
//!
//! Owns the in-memory challenge store used between login steps and during
//! WebAuthn ceremonies, plus utilities for TOTP and recovery codes. The
//! `webauthn_rs` instance is built from `[auth.mfa]` config at startup.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use totp_rs::{Algorithm, Secret, TOTP};
use url::Url;
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::auth::PasswordHashUtil;
use crate::config::AuthMfaConfig;

/// Per-token TTL for pending login and registration ceremonies.
const CHALLENGE_TTL: Duration = Duration::from_secs(10 * 60);

/// MFA method names exchanged with the frontend.
pub mod methods {
    pub const TOTP: &str = "totp";
    pub const WEBAUTHN: &str = "webauthn";
    pub const RECOVERY: &str = "recovery";
}

/// State held between a successful password check and the MFA verification
/// step. The challenge token returned to the client is the key into the map.
#[derive(Clone)]
pub struct LoginChallenge {
    pub user_id: Uuid,
    pub methods: Vec<&'static str>,
    /// Held when the user has at least one WebAuthn credential; allows the
    /// client to immediately invoke `navigator.credentials.get(...)`.
    pub webauthn_auth: Option<PasskeyAuthentication>,
}

/// State held between WebAuthn `register/begin` and `register/finish`.
#[derive(Clone)]
pub struct WebauthnRegistration {
    pub user_id: Uuid,
    pub label: String,
    pub state: PasskeyRegistration,
}

enum Entry {
    Login(LoginChallenge),
    Register(WebauthnRegistration),
}

struct StoredChallenge {
    entry: Entry,
    expires_at: Instant,
}

/// In-memory, single-instance MFA challenge store + ceremony helper.
#[derive(Clone)]
pub struct MfaService {
    inner: Arc<MfaServiceInner>,
}

struct MfaServiceInner {
    config: AuthMfaConfig,
    webauthn: Option<Webauthn>,
    challenges: RwLock<HashMap<String, StoredChallenge>>,
}

impl MfaService {
    pub fn new(config: AuthMfaConfig) -> Self {
        let webauthn = if config.enabled && config.allow_webauthn {
            match build_webauthn(&config) {
                Ok(w) => Some(w),
                Err(e) => {
                    tracing::error!("WebAuthn disabled: failed to build instance from config: {e}");
                    None
                }
            }
        } else {
            None
        };

        Self {
            inner: Arc::new(MfaServiceInner {
                config,
                webauthn,
                challenges: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn config(&self) -> &AuthMfaConfig {
        &self.inner.config
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.inner.webauthn.as_ref()
    }

    // -- Challenge store --------------------------------------------------

    pub fn put_login(&self, challenge: LoginChallenge) -> String {
        self.put(Entry::Login(challenge))
    }

    pub fn take_login(&self, token: &str) -> Option<LoginChallenge> {
        match self.take(token)? {
            Entry::Login(c) => Some(c),
            Entry::Register(_) => None,
        }
    }

    pub fn put_registration(&self, registration: WebauthnRegistration) -> String {
        self.put(Entry::Register(registration))
    }

    pub fn take_registration(&self, token: &str) -> Option<WebauthnRegistration> {
        match self.take(token)? {
            Entry::Register(r) => Some(r),
            Entry::Login(_) => None,
        }
    }

    fn put(&self, entry: Entry) -> String {
        let token: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        let mut map = self.inner.challenges.write().expect("MFA store poisoned");
        prune_expired(&mut map);
        map.insert(
            token.clone(),
            StoredChallenge {
                entry,
                expires_at: Instant::now() + CHALLENGE_TTL,
            },
        );
        token
    }

    fn take(&self, token: &str) -> Option<Entry> {
        let mut map = self.inner.challenges.write().expect("MFA store poisoned");
        prune_expired(&mut map);
        let stored = map.remove(token)?;
        if stored.expires_at <= Instant::now() {
            return None;
        }
        Some(stored.entry)
    }

    // -- TOTP helpers -----------------------------------------------------

    /// Build a TOTP instance for a given base32 secret. Used both for
    /// `otpauth_uri` generation and for code verification.
    pub fn totp(&self, secret_base32: &str, account: &str) -> Result<TOTP, String> {
        let secret = Secret::Encoded(secret_base32.to_string())
            .to_bytes()
            .map_err(|e| format!("invalid TOTP secret: {e:?}"))?;
        TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret,
            Some(self.inner.config.issuer.clone()),
            account.to_string(),
        )
        .map_err(|e| format!("failed to build TOTP: {e}"))
    }

    /// Verify a 6-digit TOTP code with ±1 step tolerance for clock drift.
    pub fn verify_totp(&self, secret_base32: &str, account: &str, code: &str) -> bool {
        match self.totp(secret_base32, account) {
            Ok(t) => t.check_current(code).unwrap_or(false),
            Err(e) => {
                tracing::warn!("TOTP verify failed to build totp: {e}");
                false
            }
        }
    }
}

fn prune_expired(map: &mut HashMap<String, StoredChallenge>) {
    let now = Instant::now();
    map.retain(|_, v| v.expires_at > now);
}

fn build_webauthn(config: &AuthMfaConfig) -> Result<Webauthn, String> {
    let origin = Url::parse(&config.relying_party_origin)
        .map_err(|e| format!("invalid relying_party_origin: {e}"))?;
    let builder = WebauthnBuilder::new(&config.relying_party_id, &origin)
        .map_err(|e| format!("WebauthnBuilder error: {e}"))?
        .rp_name(&config.relying_party_name);
    builder.build().map_err(|e| format!("Webauthn build: {e}"))
}

// ---------------------------------------------------------------------------
// Recovery codes
// ---------------------------------------------------------------------------

/// Generate `n` user-facing recovery codes of the form `XXXX-XXXX-XXXX`,
/// using uppercase alphanumerics from an unambiguous alphabet.
pub fn generate_recovery_codes(n: u32) -> Vec<String> {
    // Drop visually ambiguous characters (0/O, 1/I, etc.).
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = thread_rng();
    (0..n)
        .map(|_| {
            let mut chunks = Vec::with_capacity(3);
            for _ in 0..3 {
                let chunk: String = (0..4)
                    .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
                    .collect();
                chunks.push(chunk);
            }
            chunks.join("-")
        })
        .collect()
}

/// Argon2 hash a recovery code (so a DB read can't yield usable codes).
pub fn hash_recovery_code(code: &str) -> Result<String, String> {
    PasswordHashUtil::hash(code).map_err(|e| format!("argon2 hash: {e}"))
}

/// Verify a candidate code against a stored Argon2 hash.
pub fn verify_recovery_code(candidate: &str, hash: &str) -> bool {
    PasswordHashUtil::verify(candidate, hash).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Random TOTP secret
// ---------------------------------------------------------------------------

/// Generate a fresh base32-encoded TOTP secret (160 bits, RFC 6238 friendly).
pub fn generate_totp_secret_base32() -> String {
    // 20 random bytes encoded as base32 (no padding) — standard authenticator
    // app format.
    let mut bytes = [0u8; 20];
    thread_rng().fill(&mut bytes);
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &bytes)
}
