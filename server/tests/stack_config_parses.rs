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
];

fn substituted() -> String {
    let mut out = TEMPLATE.to_string();
    for (token, value) in SUBSTITUTIONS {
        out = out.replace(token, value);
    }
    out
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
         serialises the requests, which makes a race disappear and the tier \
         report a pass it did not earn",
        config.database.max_connections
    );
    assert!(
        config.toolguard.enabled,
        "the toolguard endpoints are the subject of the authentication fix the \
         contract stage asserts"
    );
}

#[test]
fn the_stack_config_never_clones_anybody_s_repositories() {
    let config: AppConfig = toml::from_str(&substituted()).expect("parses");
    // `PagesService::new` git-clones whatever is here into a hardcoded
    // /tmp/css-{wiki,site}-repo at boot. A stack that inherited the tracked
    // config would do two network clones on every bring-up and fail closed the
    // moment the network did -- and two test binaries doing it in parallel
    // would race over the same /tmp path.
    assert!(
        config.pages.wiki_repo.is_none(),
        "the stack config names a wiki repository: {:?}",
        config.pages.wiki_repo
    );
    assert!(
        config.pages.site_repo.is_none(),
        "the stack config names a site repository: {:?}",
        config.pages.site_repo
    );
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
