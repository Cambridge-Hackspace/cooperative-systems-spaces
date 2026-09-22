//! The stack battery's own configuration must load, and the build verb is
//! where that has to fail.
//!
//! Not an abstract concern. The first successful bring-up of the stack spent
//! ninety seconds starting Postgres and mosquitto, built a runtime image,
//! started css-server, waited out a 120-second readiness timeout, and then ran
//! five more stages that each reported a connection refused — because
//! `[calendar]` was missing its `calendars` key. The cause was one line in a
//! container log, four stages and several screens behind the first failure.
//!
//! This test parses the same template with `AppConfig`'s own deserializer. It
//! runs in the build verb, before a session spends a minute on a stack, and it
//! names the field.
//!
//! It also fixes something the container log could not have told anybody: it
//! runs on every `cargo test`, so somebody editing the suite's configuration on
//! a workstation finds out immediately rather than at the next session.

use css_server::config::AppConfig;

const TEMPLATE: &str = include_str!("../../e2e/stack-config.toml");

/// The substitutions `e2e/stack.sh` performs, with plausible values.
///
/// Kept in step with the `sed` invocation in `write_stack_config` by hand, and
/// `no_placeholder_survives_substitution` below is what makes that safe: a
/// token added to the template and not to this list fails here, and a token in
/// this list that the template no longer contains is caught by
/// `every_substitution_is_used`.
const SUBSTITUTIONS: &[(&str, &str)] = &[
    ("@SERVER_PORT@", "4399"),
    ("@STACK_TZ@", "America/Chicago"),
    ("@PG_USER@", "css_user"),
    ("@PG_PASS@", "css_pass"),
    ("@PG_PORT@", "5432"),
    ("@PG_DB@", "css"),
    ("@MQTT_PORT@", "1883"),
    ("@MQTT_NAMESPACE@", "css-e2e"),
    ("@SMTP_PORT@", "2525"),
    ("@GROUPSIO_PORT@", "4390"),
    ("@STRIPE_PORT@", "4391"),
    // Card encryption keys (#108). The real values live in stack.sh so one set
    // feeds both the server config and the lease driver; what matters here is
    // that they are 32 bytes of hex, because CardsConfig::cipher() refuses
    // anything else at startup and a stack that will not boot is a battery that
    // fails at bring-up rather than in this test.
    (
        "@CARDS_ENC_KEY@",
        "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
    ),
    (
        "@CARDS_IDX_KEY@",
        "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2",
    ),
    (
        "@CARDS_DEVICE_PEPPER@",
        "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3",
    ),
    // Representative of what `write_stack_config` actually substitutes: both
    // are derived from STACK_DIR, which is `${OUT}/stack` -- per-run, and the
    // whole point of `checkout_dir` being configurable at all.
    ("@WIKI_REPO@", "/var/tmp/css-e2e/out/stack/wiki-fixture"),
    ("@CHECKOUT_DIR@", "/var/tmp/css-e2e/out/stack"),
];

/// The value `substituted()` puts in for one placeholder, by name rather than
/// by position -- so reordering SUBSTITUTIONS cannot quietly change what an
/// assertion below is comparing against.
fn substitution_for(token: &str) -> &'static str {
    SUBSTITUTIONS
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("{token} is not in SUBSTITUTIONS"))
}

fn substituted() -> String {
    let mut out = TEMPLATE.to_string();
    for (token, value) in SUBSTITUTIONS {
        out = out.replace(token, value);
    }
    out
}

/// The stack's card keys must actually build a cipher.
///
/// `toml::from_str` is happy with any string, and `CardsConfig::cipher()` is
/// what the server calls at startup -- so without this the suite could ship a
/// config that parses here and refuses to boot there, which is a failure at
/// bring-up with no clue pointing back at this file.
#[test]
fn the_stack_card_keys_build_a_cipher() {
    let config: AppConfig = toml::from_str(&substituted()).expect("parses");
    let cipher = config
        .cards
        .cipher()
        .expect("stack card keys must be valid")
        .expect("the stack configures card keys, so this must not be None");
    let sealed = cipher.seal("STACK-CARD").expect("seals");
    assert_eq!(
        "STACK-CARD",
        cipher
            .open(&sealed.ciphertext, &sealed.nonce)
            .expect("opens"),
        "the configured keys must round-trip a card, or every swipe in the battery \
         is resolving against something nobody can read back"
    );
}

#[test]
fn the_stack_config_parses_as_an_app_config() {
    let text = substituted();
    let config: AppConfig = toml::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "e2e/stack-config.toml does not load as an AppConfig, so the stack \
             battery would fail at boot rather than here:\n\n{e}\n"
        )
    });

    // The values the stages depend on, asserted rather than assumed. Each of
    // these has a stage that reads it, and a silent change to any of them would
    // make that stage assert something other than what it says it does.
    assert_eq!(
        config.site.timezone, "America/Chicago",
        "the schema stage asserts the cluster is not on UTC; a UTC suite proves \
         nothing about an application that converts to a configured space \
         timezone on every schedule comparison"
    );
    assert!(
        config.auth.allow_registration,
        "every driver creates its accounts through /api/auth/register; with \
         registration closed the whole battery would test nothing but 403s"
    );
    assert!(
        !config.registration_challenge.throttle_enabled,
        "the fuzz and concurrency tiers register in bulk from one address; a \
         throttle would turn their findings into 429s that read like defects"
    );
    assert!(
        config.initial_setup.setup_enabled,
        "the contract stage needs an admin, and the only way to get one through \
         the shipping path is the initial-setup address"
    );
    assert_eq!(
        config.initial_setup.setup_admin_email, "admin@e2e.invalid",
        "the drivers hard-code this address to obtain an admin"
    );
    assert!(
        config.database.max_connections >= 32,
        "the concurrency tier fans out to {}; a pool smaller than the fan-out \
         serializes the requests, which makes a race disappear and the tier \
         report a pass it did not earn",
        config.database.max_connections
    );
    assert!(
        config.toolguard.enabled,
        "the toolguard endpoints are the subject of the authentication fix the \
         contract stage asserts"
    );
    assert!(
        config.auth.mfa.enabled,
        "the mfa stage enrolls a second factor and then checks that a password \
         alone stops issuing a token; with MFA switched off the whole \
         enrollment surface answers 403 before doing anything, and the stage \
         would report a row of refusals as though it had proved something"
    );
    assert!(
        config.auth.mfa.allow_totp,
        "the mfa stage's only enrollable factor is TOTP -- no stage can drive a \
         real authenticator -- so with TOTP disallowed there is nothing for it \
         to enroll"
    );
    assert_eq!(
        config.auth.mfa.enforcement,
        css_server::config::MfaEnforcement::OptIn,
        "anything stricter puts must_enroll_mfa on every account the other \
         stages create, which no assertion of theirs wants and which would make \
         a login response shape depend on this file"
    );
    assert_eq!(
        config.auth.mfa.relying_party_id, "localhost",
        "the rp_id must be the effective domain of the origin above, or the \
         same silent 403 applies"
    );
    assert_eq!(
        config.auth.mfa.recovery_code_count, 10,
        "the mfa stage asserts the exact count issued at enrollment and the \
         exact remainder after spending one; a different count here makes those \
         two assertions disagree with the driver rather than with the server"
    );
    assert_eq!(
        config.auth.mfa.relying_party_origin,
        format!("http://localhost:{}", substitution_for("@SERVER_PORT@")),
        "the WebAuthn relying party origin must be a *domain*, not the \
         127.0.0.1 the drivers connect to: `WebauthnBuilder::new` validates the \
         rp_id through `Url::domain()`, which is None for an IP literal, so an \
         IP origin makes the instance fail to build and every passkey endpoint \
         answer 403 while looking configured"
    );
}

/// The stack must not reach the network to build its pages, and must not share a
/// working tree with anything else on the host.
///
/// This used to assert that the config named no repository at all, which was a
/// bigger hammer than the problem: the hazards are the *network* and the
/// *shared path*, not the existence of a repository. Saying it that way cost
/// the pages pipeline all of its end-to-end coverage -- #81 shipped a
/// navigation that silently discarded sixty of sixty-seven pages, and no tier
/// could have noticed, because no tier had a wiki.
///
/// So the claim is narrowed to what it was always about, and the stack now
/// builds a local fixture repository (`make_wiki_fixture` in e2e/stack.sh) and
/// checks it out somewhere stack-local.
#[test]
fn the_stack_config_never_clones_over_the_network() {
    let config: AppConfig = toml::from_str(&substituted()).expect("parses");

    for (which, repo) in [
        ("wiki", &config.pages.wiki_repo),
        ("site", &config.pages.site_repo),
    ] {
        let Some(repo) = repo else { continue };
        assert!(
            !repo.contains("://") && !repo.contains('@'),
            "the stack config names a remote {which} repository ({repo:?}). \
             Bring-up would clone it on every run and fail closed the moment \
             the network did; the fixture is built locally for that reason."
        );
        assert!(
            repo.starts_with('/'),
            "the {which} repository path {repo:?} is not absolute, so what it \
             resolves to depends on the server's working directory"
        );
    }

    // The clone target used to be a hardcoded /tmp path, which is why two test
    // binaries could race over one working tree. Stack-local is the fix; /tmp
    // would reintroduce exactly the collision.
    assert_ne!(
        "/tmp", config.pages.checkout_dir,
        "the stack checks repositories out into the shared /tmp, so two stacks \
         on one host would fight over the same working tree"
    );

    // Auto-update stays off: the fixture never changes after bring-up, so a
    // poller would only add a timer that can fire mid-assertion.
    assert!(!config.pages.wiki_auto_enabled && !config.pages.site_auto_enabled);
}

#[test]
fn no_placeholder_survives_substitution() {
    // A token added to the template but not to SUBSTITUTIONS reaches the
    // running server as literal text -- `port = @PG_PORT@` is not even valid
    // TOML, and `site_url = "http://127.0.0.1:@SERVER_PORT@"` is, which is
    // worse: the server starts and every URL it generates is wrong.
    let text = substituted();
    let leftovers: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter(|l| l.contains('@') && l.matches('@').count() >= 2)
        .collect();

    // Email addresses contain one '@'; a placeholder contains two. Anything
    // matching the placeholder shape and not in SUBSTITUTIONS is the failure.
    let unresolved: Vec<&&str> = leftovers
        .iter()
        .filter(|l| {
            l.split('@').skip(1).step_by(2).any(|inner| {
                !inner.is_empty() && inner.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            })
        })
        .collect();

    assert!(
        unresolved.is_empty(),
        "placeholders survived substitution; add them to SUBSTITUTIONS here and \
         to the sed invocation in e2e/stack.sh:\n{unresolved:#?}"
    );
}

#[test]
fn every_substitution_is_used() {
    // The other direction. An entry here for a token the template no longer
    // contains is a substitution nobody performs, and it makes the list above
    // stop being a description of what stack.sh does.
    for (token, _) in SUBSTITUTIONS {
        assert!(
            TEMPLATE.contains(token),
            "{token} is substituted here but no longer appears in \
             e2e/stack-config.toml"
        );
    }
}
