//! The parts of the OpenID Connect flow that are pure rules (#225): PKCE,
//! redirect matching, scopes, opaque tokens and the redirect back to a client.
//! No database and nothing else from the crate, so these are tested on their
//! own.

use std::collections::BTreeSet;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine as _;

/// Every scope CS:S understands, with what a member is told it means. A
/// client may only be granted scopes from this list.
pub const SCOPES: &[(&str, &str)] = &[
    ("openid", "Confirm who you are"),
    ("profile", "Your name and username"),
    ("email", "Your email address"),
    ("roles", "Your roles in CS:S, such as member or admin"),
    ("training:read", "Which tools you are trained on"),
    // Not a member's to grant: an app holds it itself (client credentials),
    // to message members through their own notification settings (#226).
    ("notify:members", "Send members notifications through CS:S"),
    // The app asks which tools any member is trained on, at each use, rather
    // than keeping members' own tokens to ask with.
    ("training:members", "Which tools any member is trained on"),
];

/// Scopes that act for the app itself rather than for a member, so they are
/// only ever granted through the client-credentials grant.
pub const APP_SCOPES: &[&str] = &["notify:members", "training:members"];

/// PKCE (RFC 7636), S256 only: is `verifier` the one `challenge` was made from?
/// A verifier outside the RFC's alphabet or length is refused outright.
pub fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    let well_formed = (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b));
    if !well_formed {
        return false;
    }
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
    let computed = B64.encode(digest.as_ref());
    constant_time_eq(computed.as_bytes(), challenge.as_bytes())
}

/// A challenge is the base64url of a SHA-256: exactly 43 characters.
pub fn challenge_is_well_formed(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Redirect URIs match exactly, character for character, as OAuth 2.0
/// Security Best Current Practice requires. No prefix or wildcard matching.
pub fn redirect_allowed(registered: &[String], given: &str) -> bool {
    registered.iter().any(|r| r == given)
}

/// Is this something a client may register as a redirect URI? An absolute
/// https URL with no fragment; plain http only for loopback, for local tools.
pub fn redirect_is_registrable(uri: &str) -> bool {
    let Ok(u) = url::Url::parse(uri) else {
        return false;
    };
    if u.fragment().is_some() || u.host_str().is_none() {
        return false;
    }
    match u.scheme() {
        "https" => true,
        "http" => matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

/// A space-separated scope string as a set.
pub fn parse_scopes(s: &str) -> BTreeSet<String> {
    s.split_whitespace().map(str::to_string).collect()
}

pub fn join_scopes(s: &BTreeSet<String>) -> String {
    s.iter().cloned().collect::<Vec<_>>().join(" ")
}

pub fn scope_is_known(scope: &str) -> bool {
    SCOPES.iter().any(|(s, _)| *s == scope)
}

/// A new opaque secret: 32 random bytes, base64url. Used for authorization
/// codes, refresh tokens and client secrets.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    aws_lc_rs::rand::fill(&mut bytes).expect("system randomness");
    B64.encode(bytes)
}

/// How an opaque secret is stored: its SHA-256, hex. A code or refresh token
/// is 256 random bits, so a fast hash is enough; nobody can guess one to test
/// against it. (Client secrets are hashed with argon2 like passwords instead,
/// by the caller, because an admin might paste in a weak one some day.)
pub fn hash_secret(secret: &str) -> String {
    hex::encode(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, secret.as_bytes()).as_ref())
}

/// The client's redirect URI with `params` added to its query, keeping any
/// query it already had. The caller has already checked the URI is registered.
pub fn redirect_with(redirect_uri: &str, params: &[(&str, &str)]) -> Option<String> {
    let mut u = url::Url::parse(redirect_uri).ok()?;
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in params {
            q.append_pair(k, v);
        }
    }
    Some(u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 7636 appendix B's own example.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn the_rfc_example_verifier_matches_its_challenge() {
        assert!(pkce_matches(VERIFIER, CHALLENGE));
        assert!(challenge_is_well_formed(CHALLENGE));
    }

    #[test]
    fn another_verifier_does_not_match() {
        let other = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl";
        assert!(!pkce_matches(other, CHALLENGE));
    }

    #[test]
    fn a_verifier_outside_the_rfc_shape_is_refused() {
        assert!(!pkce_matches("short", CHALLENGE));
        assert!(!pkce_matches(&"a".repeat(129), CHALLENGE));
        assert!(!pkce_matches(&format!("{}+", &VERIFIER[..42]), CHALLENGE));
    }

    #[test]
    fn a_plain_challenge_is_not_well_formed() {
        // A client sending code_challenge_method=plain sends the verifier
        // itself, which is not 43 base64url characters of a hash.
        assert!(!challenge_is_well_formed("plain-challenge"));
    }

    #[test]
    fn redirects_match_exactly() {
        let reg = vec!["https://pop.example/cb".to_string()];
        assert!(redirect_allowed(&reg, "https://pop.example/cb"));
        assert!(!redirect_allowed(&reg, "https://pop.example/cb/"));
        assert!(!redirect_allowed(&reg, "https://pop.example/cb?x=1"));
        assert!(!redirect_allowed(&reg, "https://pop.example.evil/cb"));
        assert!(!redirect_allowed(&reg, "HTTPS://pop.example/cb"));
    }

    #[test]
    fn only_https_or_loopback_http_may_be_registered() {
        assert!(redirect_is_registrable("https://pop.example/cb"));
        assert!(redirect_is_registrable("http://localhost:8080/cb"));
        assert!(redirect_is_registrable("http://127.0.0.1/cb"));
        assert!(!redirect_is_registrable("http://pop.example/cb"));
        assert!(!redirect_is_registrable("https://pop.example/cb#frag"));
        assert!(!redirect_is_registrable("javascript:alert(1)"));
        assert!(!redirect_is_registrable("/relative"));
    }

    #[test]
    fn a_redirect_keeps_its_own_query_and_escapes_values() {
        let r = redirect_with(
            "https://pop.example/cb?tenant=a",
            &[("code", "x y"), ("state", "&s")],
        )
        .unwrap();
        assert_eq!(r, "https://pop.example/cb?tenant=a&code=x+y&state=%26s");
    }

    #[test]
    fn secrets_are_long_random_and_hash_stably() {
        let a = new_secret();
        assert_eq!(a.len(), 43);
        assert_ne!(a, new_secret());
        assert_eq!(hash_secret(&a), hash_secret(&a));
        assert_eq!(hash_secret(&a).len(), 64);
    }

    #[test]
    fn scopes_parse_as_a_set() {
        let s = parse_scopes("openid  profile openid");
        assert_eq!(join_scopes(&s), "openid profile");
        assert!(scope_is_known("training:read"));
        assert!(!scope_is_known("admin"));
    }
}
