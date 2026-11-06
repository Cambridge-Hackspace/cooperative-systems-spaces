//! The process-wide rustls crypto provider.
//!
//! rustls 0.23 resolves its crypto backend from a process-global provider, and
//! PANICS the first time a TLS config is built if it cannot determine one. That
//! was #207: `tokio-tungstenite` pulls rustls with default features off, so no
//! provider was compiled in, and the WebSocket task died on every boot while
//! the rest of the edge carried on -- HTTP sync, local broker and scan answers
//! all working, the edge showing offline, nothing logged above INFO.
//!
//! Installed EXPLICITLY rather than left to rustls's own feature-based
//! auto-detection, even though exactly one provider is now compiled in and
//! auto-detection would work today. Auto-detection is a property of the whole
//! dependency graph: any future bump that enables a second provider anywhere
//! turns it back into a startup panic. An explicit install is a decision this
//! crate makes and keeps.
//!
//! Deliberately NOT in `css_lib`, which would be the obvious home for something
//! two binaries share: the server depends on css_lib and deliberately uses
//! native-tls (see server/Cargo.toml), so putting rustls there would drag a TLS
//! stack into the one crate that decided against it. `cli/src/tls.rs` is the
//! same ten lines for the same reason; `checks/tests/tls_backends_agree.rs`
//! asserts both exist.

use tracing::{debug, warn};

/// Install the process-wide provider. Idempotent and safe to call more than
/// once: a second call is reported and ignored, because losing the race means
/// a provider is installed, which is all this function is for.
pub fn install_crypto_provider() {
    match rustls::crypto::aws_lc_rs::default_provider().install_default() {
        Ok(()) => debug!("rustls crypto provider installed (aws-lc-rs, post-quantum preferred)"),
        Err(_) => warn!("rustls crypto provider was already installed; leaving it alone"),
    }
}

#[cfg(test)]
mod tests {
    /// The #207 oracle. Before the fix, nothing in this process installed a
    /// provider and `ClientConfig::builder()` -- which is what tungstenite calls
    /// on its way to a TLS handshake -- panicked instead of returning. Nothing
    /// in the edge's suites ever built one, which is why three tiers of testing
    /// and thirteen interop checks all passed over it.
    ///
    /// What this does NOT prove: that the install happens before the first TLS
    /// config in `main`. Ordering inside main is not observable from here;
    /// `checks/tests/tls_backends_agree.rs` asserts the call exists and this
    /// asserts it works.
    #[test]
    fn a_tls_client_config_can_be_built_after_installing_the_provider() {
        super::install_crypto_provider();

        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "no process-wide provider after install_crypto_provider()"
        );

        // The call that panicked. Roots are irrelevant to the provider lookup,
        // so an empty store is enough to exercise it.
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        assert!(
            !config.crypto_provider().cipher_suites.is_empty(),
            "the installed provider offers no cipher suites"
        );
    }

    #[test]
    fn installing_twice_is_not_fatal() {
        // Both binaries call this once, but tests in one process call it many
        // times, and a second install returning Err must not be an abort.
        super::install_crypto_provider();
        super::install_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
