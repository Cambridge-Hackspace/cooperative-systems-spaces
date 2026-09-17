//! The `devseed` stage and the JS driver fixtures must agree about the admin.
//!
//! `[initial_setup]` grants admin by matching one configured address, so both
//! fixtures have to register that same address and only the first one to run
//! gets to choose the password. They disagreed for a long time -- devseed used
//! `password123!`, `drivers/lib.mjs` uses `e2e-password-1234` -- and the
//! consequence was bad enough that devseed was kept out of the battery
//! entirely: every driver calling `adminAccount()` afterwards received an
//! address it could neither claim nor sign into, and failed with a 401 three
//! stages downstream that mentioned neither devseed nor passwords.
//!
//! They agree now, which is what lets devseed run in the battery and therefore
//! stop rotting. This check is what keeps them agreeing.
//!
//! The literal is duplicated on purpose rather than plumbed through an
//! environment variable. Shell and node share no constant, and a variable one
//! side forgot to export would fail exactly the way the original bug did --
//! late, and somewhere else. Duplication plus an assertion fails early and in
//! the right place, which is the trade this file exists to make.
//!
//! What this does NOT prove: that either fixture works. It proves they cannot
//! silently diverge, which is the specific failure that cost a stage its place
//! in the battery.

use css_checks::read;

/// The password `devseed` falls back to, read out of the shell default.
fn devseed_password() -> String {
    let run_sh = read("e2e/run.sh");
    let marker = "local pass=\"${CSS_DEV_ADMIN_PASS:-";
    let at = run_sh.find(marker).unwrap_or_else(|| {
        panic!(
            "could not find devseed's password default in e2e/run.sh. If the \
             stage was rewritten, this check has to be rewritten with it rather \
             than left passing on a string that is no longer there."
        )
    });
    let rest = &run_sh[at + marker.len()..];
    rest[..rest.find("}\"").expect("unterminated default")].to_string()
}

/// The password the JS drivers register and sign in with.
fn driver_password() -> String {
    let lib = read("e2e/drivers/lib.mjs");
    let marker = "export const PASSWORD = '";
    let at = lib
        .find(marker)
        .unwrap_or_else(|| panic!("could not find `export const PASSWORD` in e2e/drivers/lib.mjs"));
    let rest = &lib[at + marker.len()..];
    rest[..rest.find('\'').expect("unterminated password literal")].to_string()
}

#[test]
fn devseed_and_the_drivers_register_the_admin_with_the_same_password() {
    let seed = devseed_password();
    let drivers = driver_password();

    assert!(
        !seed.is_empty() && !drivers.is_empty(),
        "one of the passwords came back empty ({seed:?} / {drivers:?}), so this \
         check would pass on any pair of blanks"
    );

    assert_eq!(
        seed, drivers,
        "e2e/run.sh's devseed stage registers the admin with {seed:?} and \
         e2e/drivers/lib.mjs signs in with {drivers:?}.\n\n\
         Both claim the one address [initial_setup] grants admin to, and \
         devseed runs first, so every driver that calls adminAccount() will get \
         a 401 several stages later with nothing pointing back at this. Make \
         them the same string."
    );
}

#[cfg(test)]
mod the_check_rejects_the_state_that_shipped_the_bug {
    use super::*;

    /// The two literals as they actually stood before devseed joined the
    /// battery. The check has to reject this pair, or it would have permitted
    /// precisely the drift it exists to prevent.
    #[test]
    fn the_historical_pair_does_not_match() {
        assert_ne!("password123!", "e2e-password-1234");
    }

    /// And the extractors have to find something. Both return a `String`, and a
    /// silent empty one on each side would compare equal and pass.
    #[test]
    fn both_extractors_find_a_real_literal() {
        assert!(!devseed_password().is_empty());
        assert!(!driver_password().is_empty());
        assert!(devseed_password().len() > 4);
    }
}
