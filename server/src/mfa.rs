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

// ---------------------------------------------------------------------------
// Tier 1: the MFA primitives.
// ---------------------------------------------------------------------------
// Everything in this file is pure or in-memory, so all of it is reachable
// without a database, a network or a browser -- and none of it was tested at
// all until this module. The contract tier proves the eleven MFA routes are
// *guarded*; it cannot prove that verification is *correct*, because every
// route that would accept a valid credential reaches the database. That gap is
// what this closes.
//
// Two properties get particular attention, because both are the kind that a
// suite reports as covered while never executing:
//
//   * **Skew.** `verify_totp` calls `check_current`, which reads the system
//     clock. A test that generates a code for "now" and immediately checks it
//     passes whether the tolerance is +-1 step or +-100, so it says nothing
//     about the window. Every skew assertion below therefore goes through
//     `TOTP::check(code, time)` at a fixed instant, which is deterministic and
//     actually pins the boundary.
//
//   * **Single use.** The store is the anti-replay mechanism for the whole
//     login flow. `take` removing the entry is the entire guarantee, and it is
//     one `map.remove` away from being a `map.get`.
#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant on a step boundary, so the skew tests below do not
    /// depend on when the suite runs. 2026-01-01T00:00:00Z, and 30 divides it
    /// exactly -- which matters, because an offset of one step from a boundary
    /// lands in exactly one step and not sometimes two.
    const T0: u64 = 1_767_225_600;

    /// A valid 160-bit base32 secret, fixed so generated codes are reproducible.
    const SECRET: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";
    const ACCOUNT: &str = "user@example.invalid";

    fn cfg() -> AuthMfaConfig {
        AuthMfaConfig {
            enabled: true,
            issuer: "Test Issuer".to_string(),
            relying_party_id: "localhost".to_string(),
            relying_party_name: "Test RP".to_string(),
            relying_party_origin: "http://localhost:3000".to_string(),
            ..AuthMfaConfig::default()
        }
    }

    fn service() -> MfaService {
        MfaService::new(cfg())
    }

    fn login_challenge(user_id: Uuid) -> LoginChallenge {
        LoginChallenge {
            user_id,
            methods: vec![methods::TOTP],
            webauthn_auth: None,
        }
    }

    /// Backdate a stored challenge without sleeping. The store keys expiry off
    /// `Instant`, which cannot be injected, so reaching into the map is the
    /// only way to test expiry at all -- and a ten-minute sleep is not a test.
    fn expire(svc: &MfaService, token: &str) {
        let mut map = svc.inner.challenges.write().expect("store not poisoned");
        let stored = map.get_mut(token).expect("token is in the store");
        stored.expires_at = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("the machine has been up for at least one second");
    }

    // -- Challenge store ----------------------------------------------------

    #[test]
    fn a_login_challenge_comes_back_with_what_was_put_in() {
        let svc = service();
        let uid = Uuid::new_v4();
        let token = svc.put_login(login_challenge(uid));

        let got = svc
            .take_login(&token)
            .expect("the challenge is retrievable");
        assert_eq!(got.user_id, uid);
        assert_eq!(got.methods, vec![methods::TOTP]);
    }

    #[test]
    fn a_challenge_token_is_single_use() {
        // The whole anti-replay guarantee of the login flow. `take` removing
        // the entry is what stops a captured challenge_token being spent twice,
        // and it is one word away from being a `get`.
        let svc = service();
        let token = svc.put_login(login_challenge(Uuid::new_v4()));

        assert!(svc.take_login(&token).is_some(), "first use succeeds");
        assert!(
            svc.take_login(&token).is_none(),
            "a challenge token was accepted twice; the store is not consuming \
             the entry, so a captured token could be replayed for a second JWT"
        );
    }

    #[test]
    fn an_unknown_token_is_refused_rather_than_matching_something_else() {
        let svc = service();
        svc.put_login(login_challenge(Uuid::new_v4()));
        assert!(svc.take_login("no-such-token").is_none());
        assert!(svc.take_login("").is_none());
    }

    #[test]
    fn an_expired_challenge_is_refused() {
        let svc = service();
        let token = svc.put_login(login_challenge(Uuid::new_v4()));
        expire(&svc, &token);
        assert!(
            svc.take_login(&token).is_none(),
            "an expired challenge was still spendable"
        );
    }

    #[test]
    fn expiry_is_ten_minutes() {
        // A value assertion, and it earns its place: the window is the length
        // of time a password-only half-authentication stays spendable. Widening
        // it is a security decision and should be made deliberately, not by
        // somebody adjusting a constant to make a slow test pass.
        assert_eq!(CHALLENGE_TTL, Duration::from_secs(600));
    }

    #[test]
    fn putting_a_challenge_prunes_the_ones_that_expired() {
        // Without this the map is an unbounded leak: every abandoned login --
        // and an abandoned login is the common case for a mistyped password
        // followed by a retry -- would sit in memory for the life of the
        // process.
        let svc = service();
        let stale = svc.put_login(login_challenge(Uuid::new_v4()));
        expire(&svc, &stale);

        let fresh = svc.put_login(login_challenge(Uuid::new_v4()));

        let map = svc.inner.challenges.read().expect("store not poisoned");
        assert!(
            !map.contains_key(&stale),
            "the expired entry was not pruned"
        );
        assert!(map.contains_key(&fresh));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn two_challenges_do_not_collide() {
        let svc = service();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ta = svc.put_login(login_challenge(a));
        let tb = svc.put_login(login_challenge(b));

        assert_ne!(ta, tb);
        assert_eq!(svc.take_login(&tb).expect("b").user_id, b);
        assert_eq!(
            svc.take_login(&ta)
                .expect("a is untouched by b's use")
                .user_id,
            a
        );
    }

    #[test]
    fn tokens_are_long_alphanumeric_and_not_repeated() {
        // The token is the sole bearer credential between the password check
        // and the JWT. 48 alphanumerics is ~285 bits; anything materially
        // shorter would make guessing worth attempting, and the store has no
        // rate limit of its own.
        let svc = service();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let t = svc.put_login(login_challenge(Uuid::new_v4()));
            assert_eq!(t.len(), 48, "token length changed: {t}");
            assert!(
                t.chars().all(|c| c.is_ascii_alphanumeric()),
                "token is not alphanumeric: {t}"
            );
            assert!(seen.insert(t), "the store issued a duplicate token");
        }
    }

    #[test]
    fn a_registration_challenge_comes_back_with_what_was_put_in() {
        let svc = service();
        let uid = Uuid::new_v4();
        let webauthn = svc.webauthn().expect("built from a valid config");
        let (_ccr, state) = webauthn
            .start_passkey_registration(uid, ACCOUNT, "Test User", None)
            .expect("a registration ceremony starts without any I/O");

        let token = svc.put_registration(WebauthnRegistration {
            user_id: uid,
            label: "Yubikey".to_string(),
            state,
        });

        let got = svc.take_registration(&token).expect("retrievable");
        assert_eq!(got.user_id, uid);
        assert_eq!(got.label, "Yubikey");
    }

    #[test]
    fn asking_for_the_wrong_kind_of_challenge_destroys_it() {
        // FINDING, pinned. `take` removes the entry from the map *before* it
        // discovers the variant is not the one asked for, and then returns
        // None. So presenting a registration token to /verify -- or a login
        // token to register/finish -- does not merely fail: it consumes the
        // ceremony, and the legitimate call that follows fails too, with
        // "Unknown or expired challenge_token".
        //
        // It is not a privilege escalation: the tokens are 48 random
        // alphanumerics, so nobody is guessing one to grief a stranger. It is a
        // client-bug amplifier, and it is invisible in the logs -- the second
        // failure looks like an expiry, which is the one explanation that is
        // certainly wrong.
        //
        // Pinned rather than fixed because the fix is a decision, not a typo:
        // either `take` peeks before removing, or the two token namespaces are
        // separated. Both are defensible and neither belongs in a test commit.
        let svc = service();
        let uid = Uuid::new_v4();
        let webauthn = svc.webauthn().expect("built from a valid config");
        let (_ccr, state) = webauthn
            .start_passkey_registration(uid, ACCOUNT, "Test User", None)
            .expect("a registration ceremony starts without any I/O");
        let token = svc.put_registration(WebauthnRegistration {
            user_id: uid,
            label: "Yubikey".to_string(),
            state,
        });

        assert!(
            svc.take_login(&token).is_none(),
            "a registration token must not satisfy a login"
        );
        assert!(
            svc.take_registration(&token).is_none(),
            "the registration token survived being offered to take_login; if \
             this now passes, `take` peeks before removing and the pinned \
             finding above is fixed -- delete the pin rather than this assertion"
        );
    }

    #[test]
    fn asking_for_the_wrong_kind_of_challenge_destroys_it_in_the_other_direction() {
        // The mirror of the above, so a fix that only guards one variant is
        // still caught.
        let svc = service();
        let token = svc.put_login(login_challenge(Uuid::new_v4()));

        assert!(
            svc.take_registration(&token).is_none(),
            "a login token must not satisfy a registration finish"
        );
        assert!(
            svc.take_login(&token).is_none(),
            "the login token survived being offered to take_registration"
        );
    }

    // -- WebAuthn instance construction -------------------------------------

    #[test]
    fn webauthn_is_built_only_when_it_is_both_enabled_and_allowed() {
        assert!(
            MfaService::new(cfg()).webauthn().is_some(),
            "a valid, enabled config must produce a Webauthn instance"
        );
        assert!(
            MfaService::new(AuthMfaConfig {
                enabled: false,
                ..cfg()
            })
            .webauthn()
            .is_none(),
            "the master toggle must switch WebAuthn off"
        );
        assert!(
            MfaService::new(AuthMfaConfig {
                allow_webauthn: false,
                ..cfg()
            })
            .webauthn()
            .is_none(),
            "allow_webauthn = false must switch WebAuthn off"
        );
    }

    #[test]
    fn a_misconfigured_relying_party_disables_webauthn_instead_of_panicking() {
        // The fail-soft path. A deployment that fat-fingers the origin should
        // lose passkeys and keep TOTP, not fail to boot -- and, more to the
        // point, `MfaService::new` is called during startup where a panic is a
        // container that crash-loops with an error nobody reads.
        for origin in ["not a url", "", "http://[::1"] {
            let svc = MfaService::new(AuthMfaConfig {
                relying_party_origin: origin.to_string(),
                ..cfg()
            });
            assert!(
                svc.webauthn().is_none(),
                "origin {origin:?} should have been rejected"
            );
        }

        // An rp_id that is not a suffix of the origin's host is the other way
        // to get this wrong, and webauthn-rs rejects it at build time.
        let svc = MfaService::new(AuthMfaConfig {
            relying_party_id: "example.org".to_string(),
            relying_party_origin: "http://localhost:3000".to_string(),
            ..cfg()
        });
        assert!(
            svc.webauthn().is_none(),
            "an rp_id unrelated to the origin should have been rejected"
        );
    }

    #[test]
    fn the_config_is_returned_as_given() {
        let svc = service();
        assert!(svc.config().enabled);
        assert_eq!(svc.config().issuer, "Test Issuer");
    }

    // -- TOTP ---------------------------------------------------------------

    #[test]
    fn the_totp_parameters_are_the_rfc_defaults() {
        // Every authenticator app assumes 6 digits / 30 seconds / SHA-1 unless
        // the otpauth URI says otherwise. Changing any of these silently makes
        // every already-enrolled authenticator wrong.
        let totp = service().totp(SECRET, ACCOUNT).expect("a valid secret");
        assert_eq!(totp.digits, 6);
        assert_eq!(totp.step, 30);
        assert_eq!(totp.skew, 1);
        assert_eq!(totp.algorithm, Algorithm::SHA1);
        assert_eq!(totp.issuer.as_deref(), Some("Test Issuer"));
        assert_eq!(totp.account_name, ACCOUNT);
    }

    #[test]
    fn a_generated_code_is_six_digits() {
        let totp = service().totp(SECRET, ACCOUNT).expect("a valid secret");
        let code = totp.generate(T0);
        assert_eq!(code.len(), 6, "code was {code:?}");
        assert!(
            code.chars().all(|c| c.is_ascii_digit()),
            "code was {code:?}"
        );
    }

    #[test]
    fn a_code_is_accepted_within_one_step_and_refused_beyond_it() {
        // The skew window, pinned at a fixed instant so the assertion is about
        // the tolerance rather than about when the suite happened to run.
        //
        // `verify_totp` cannot be used here: it reads the system clock, so a
        // code minted for "now" is accepted whatever the tolerance is, and the
        // test would pass identically with skew = 0 or skew = 100.
        let totp = service().totp(SECRET, ACCOUNT).expect("a valid secret");

        for offset in [-30i64, 0, 30] {
            let minted_at = (T0 as i64 + offset) as u64;
            let code = totp.generate(minted_at);
            assert!(
                totp.check(&code, T0),
                "a code {offset}s from now was refused; the +-1 step tolerance \
                 that absorbs clock drift between a phone and the server is \
                 gone, and users with slightly-off clocks cannot log in"
            );
        }

        for offset in [-90i64, -60, 60, 90] {
            let minted_at = (T0 as i64 + offset) as u64;
            let code = totp.generate(minted_at);
            assert!(
                !totp.check(&code, T0),
                "a code {offset}s from now was accepted; the replay window is \
                 wider than one step in each direction"
            );
        }
    }

    #[test]
    fn a_code_for_a_different_secret_is_refused() {
        let svc = service();
        let mine = svc.totp(SECRET, ACCOUNT).expect("a valid secret");
        let theirs = svc
            .totp("KRSXG5CTMVRXEZLUKRSXG5CTMVRXEZLU", ACCOUNT)
            .expect("a valid secret");

        assert!(
            !mine.check(&theirs.generate(T0), T0),
            "one user's authenticator code verified against another's secret"
        );
    }

    #[test]
    fn the_account_name_does_not_change_the_code() {
        // A code is a function of the secret and the time only. If this ever
        // stopped being true, changing a user's email address would silently
        // invalidate their enrolled authenticator -- and `verify_totp` is
        // called with `user.email`, so that is exactly the shape of the bug.
        let svc = service();
        let a = svc.totp(SECRET, "one@example.invalid").expect("valid");
        let b = svc.totp(SECRET, "two@example.invalid").expect("valid");
        assert_eq!(a.generate(T0), b.generate(T0));
    }

    #[test]
    fn verify_totp_accepts_the_code_an_authenticator_would_show_right_now() {
        // The end-to-end path through the system clock, which the deterministic
        // skew test above deliberately avoids. Minting for "now" and checking
        // at "now" is safe across a step boundary: if the clock advances one
        // step between the two calls the code is still within tolerance.
        let svc = service();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs();
        let code = svc.totp(SECRET, ACCOUNT).expect("valid").generate(now);

        assert!(
            svc.verify_totp(SECRET, ACCOUNT, &code),
            "the code an authenticator app would display was refused"
        );
    }

    #[test]
    fn verify_totp_refuses_the_things_a_confused_client_sends() {
        let svc = service();
        for code in ["", "000000", "abcdef", "12345", "1234567", "  ", "null"] {
            assert!(
                !svc.verify_totp(SECRET, ACCOUNT, code),
                "{code:?} was accepted as a TOTP code"
            );
        }
    }

    #[test]
    fn an_unparseable_secret_is_an_error_rather_than_a_panic() {
        // The stored secret comes from the database. A corrupt row must refuse
        // the login, not take the process down with it.
        let svc = service();
        for secret in ["", "not valid base32!", "1111", "====", "AB"] {
            assert!(
                svc.totp(secret, ACCOUNT).is_err(),
                "{secret:?} was accepted as a TOTP secret"
            );
            assert!(
                !svc.verify_totp(secret, ACCOUNT, "123456"),
                "{secret:?} verified a code instead of failing closed"
            );
        }
    }

    #[test]
    fn the_otpauth_uri_is_what_an_authenticator_app_expects() {
        let totp = service().totp(SECRET, ACCOUNT).expect("valid");
        let url = totp.get_url();

        assert!(url.starts_with("otpauth://totp/"), "url was {url}");
        assert!(url.contains("issuer=Test%20Issuer"), "url was {url}");
        assert!(
            url.contains("Test%20Issuer:user%40example.invalid"),
            "url was {url}"
        );
        assert!(url.contains(&format!("secret={SECRET}")), "url was {url}");

        // Absence is the assertion. totp-rs omits these three parameters when
        // they hold the RFC defaults, so their absence is what tells an
        // authenticator app to assume 6 digits / SHA-1 / 30 seconds. If any of
        // the three is ever changed server-side, the parameter appears here --
        // and every already-enrolled authenticator is silently wrong.
        for defaulted in ["digits=", "algorithm=", "period="] {
            assert!(
                !url.contains(defaulted),
                "the otpauth URI now carries {defaulted}, which means a TOTP \
                 parameter moved off the RFC default: {url}"
            );
        }
    }

    // -- TOTP secret generation ---------------------------------------------

    #[test]
    fn a_generated_secret_is_160_bits_of_base32() {
        // 160 bits is the RFC 6238 recommendation, and 20 bytes encodes to
        // exactly 32 unpadded base32 characters.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            let s = generate_totp_secret_base32();
            assert_eq!(s.len(), 32, "secret was {s:?}");
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c)),
                "secret is outside the RFC 4648 alphabet: {s:?}"
            );
            assert!(!s.contains('='), "secret is padded: {s:?}");

            let decoded = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, &s)
                .expect("a generated secret decodes");
            assert_eq!(decoded.len(), 20, "secret is not 160 bits");

            assert!(
                seen.insert(s),
                "generate_totp_secret_base32 repeated itself"
            );
        }
    }

    #[test]
    fn a_generated_secret_is_usable_by_the_verifier_that_will_receive_it() {
        // The two halves are written independently -- one encodes, the other
        // decodes through totp_rs::Secret -- so their agreement is a real
        // property and not a tautology.
        let svc = service();
        let secret = generate_totp_secret_base32();
        let totp = svc
            .totp(&secret, ACCOUNT)
            .expect("a freshly generated secret must be accepted by totp()");
        assert!(svc.verify_totp(
            &secret,
            ACCOUNT,
            &totp.generate(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("the clock is after 1970")
                    .as_secs(),
            )
        ));
    }

    // -- Recovery codes -----------------------------------------------------

    #[test]
    fn recovery_codes_have_the_shape_the_ui_promises() {
        let codes = generate_recovery_codes(10);
        assert_eq!(codes.len(), 10);
        for code in &codes {
            assert_eq!(code.len(), 14, "code was {code:?}");
            let chunks: Vec<&str> = code.split('-').collect();
            assert_eq!(chunks.len(), 3, "code was {code:?}");
            assert!(chunks.iter().all(|c| c.len() == 4), "code was {code:?}");
        }
    }

    #[test]
    fn recovery_codes_avoid_the_characters_people_misread() {
        // The whole reason for the custom alphabet: these are read off a screen
        // and typed back under stress, usually from a printout, and 0/O and 1/I
        // are where that goes wrong. A code containing one is not a cosmetic
        // problem -- it is a member locked out of the building.
        const ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        for code in generate_recovery_codes(200) {
            for c in code.chars().filter(|c| *c != '-') {
                assert!(
                    ALPHABET.contains(c),
                    "recovery code {code:?} contains {c:?}, which is outside \
                     the unambiguous alphabet"
                );
            }
            for ambiguous in ['0', 'O', '1', 'I'] {
                assert!(
                    !code.contains(ambiguous),
                    "recovery code {code:?} contains {ambiguous:?}"
                );
            }
        }
    }

    #[test]
    fn recovery_codes_are_distinct_and_the_count_is_honored() {
        for n in [0u32, 1, 8, 10] {
            let codes = generate_recovery_codes(n);
            assert_eq!(codes.len(), n as usize);
            let unique: std::collections::HashSet<&String> = codes.iter().collect();
            assert_eq!(unique.len(), codes.len(), "a batch repeated a code");
        }
    }

    #[test]
    fn a_recovery_code_is_stored_as_a_salted_argon2_hash() {
        // A database read must not yield usable codes. Two hashes of the same
        // code differing is what says the salt is per-hash rather than fixed.
        let code = "ABCD-EFGH-JKLM";
        let a = hash_recovery_code(code).expect("hashes");
        let b = hash_recovery_code(code).expect("hashes");

        assert!(a.starts_with("$argon2"), "hash was {a}");
        assert!(!a.contains(code), "the hash contains the code itself");
        assert_ne!(a, b, "the same code hashed twice identically: no salt");
        assert!(verify_recovery_code(code, &a));
        assert!(verify_recovery_code(code, &b));
    }

    #[test]
    fn a_wrong_recovery_code_does_not_verify() {
        let hash = hash_recovery_code("ABCD-EFGH-JKLM").expect("hashes");
        for wrong in ["ABCD-EFGH-JKLN", "ABCDEFGHJKLM", "", "ABCD-EFGH", " "] {
            assert!(
                !verify_recovery_code(wrong, &hash),
                "{wrong:?} verified against another code's hash"
            );
        }
    }

    #[test]
    fn a_corrupt_hash_fails_closed_rather_than_panicking() {
        // `verify_recovery_code` swallows the parse error into `false`. That is
        // the right answer, and this is what says so -- a row mangled by a bad
        // migration must refuse the login, not take the request down.
        for hash in ["", "not-a-hash", "$argon2id$v=19$garbage", "$2y$10$abc"] {
            assert!(!verify_recovery_code("ABCD-EFGH-JKLM", hash));
        }
    }

    #[test]
    fn recovery_codes_are_case_sensitive() {
        // FINDING, pinned. The codes are shown in uppercase and hashed exactly
        // as generated, and neither `verify_recovery_code` nor its only caller
        // (`api::mfa::verify_recovery_path`, which trims and nothing else)
        // normalizes case. A member who types their printed code in lowercase
        // -- which is what a phone keyboard offers by default -- is refused,
        // and the refusal is indistinguishable from a wrong code.
        //
        // Worse, the attempt is not free: `/verify` consumes the login
        // challenge before it dispatches, so a lowercase code costs the user
        // their whole login and sends them back to the password prompt.
        //
        // Pinned rather than fixed because normalizing is a one-line change
        // with a real question behind it -- whether to uppercase in the API
        // layer or to widen `verify_recovery_code` -- and because the same
        // question applies to the dashes, which are also not stripped.
        let hash = hash_recovery_code("ABCD-EFGH-JKLM").expect("hashes");
        assert!(
            !verify_recovery_code("abcd-efgh-jklm", &hash),
            "recovery codes are now case-insensitive; if that was deliberate, \
             delete this pin"
        );
        assert!(
            !verify_recovery_code("ABCDEFGHJKLM", &hash),
            "recovery codes now ignore the dashes; if that was deliberate, \
             delete this pin"
        );
    }
}
