//! The process-wide rustls crypto provider.
//!
//! rustls 0.23 resolves its crypto backend from a process-global provider, and
//! PANICS the first time a TLS config is built if it cannot determine one. The
//! edge met that panic as #207; this binary would meet it on its first HTTPS
//! request, which is every command it has. It reaches rustls through reqwest's
//! `rustls-tls-webpki-roots-no-provider`, chosen so that nothing but this crate
//! decides which provider is used.
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
//! `edge/src/tls.rs` is the same ten lines for the same reason;
//! `checks/tests/tls_backends_agree.rs`
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
