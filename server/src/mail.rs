//! Outbound email.
//!
//! The `[email]` config section has existed since the first release and was
//! read by nothing. Nine fields -- host, port, username, password, use_tls,
//! use_ssl, from_email, from_name, enabled -- parsed, validated, and then
//! discarded, while `config.sample.toml` described them as being for
//! "notifications, password resets, etc." A deployment could be configured
//! with working SMTP credentials, restart cleanly, and send nothing, with no
//! error anywhere to say so. This module is the consumer that section always
//! implied.
//!
//! Two design decisions are deliberate and both are about not repeating a
//! failure mode this codebase already has.
//!
//! **A send failure is returned, never discarded.** Every audit write in this
//! server is `let _ = state.db.create_audit_log(..)`, which is exactly why a
//! `AuditEventType` variant missing from the `audit_logs` CHECK constraint
//! drops rows in total silence -- the defect
//! `checks/tests/audit_event_types.rs` exists to catch. Mail is the one
//! subsystem where "it didn't happen and nobody was told" is indistinguishable
//! from a member never receiving a password reset, so [`MailService::send`]
//! returns its error and callers are expected to act on it.
//!
//! **`enabled = false` is [`MailError::Disabled`], not `Ok(())`.** A caller has
//! to be able to tell "this deployment does not do email" from "sent". Those
//! two answers lead to different handling -- the first is a configuration
//! statement, the second is a delivery claim -- and collapsing them into one
//! success is how a reset flow ends up silently doing nothing.
//!
//! Config is read live, per send, through [`ConfigManager::get_config`], the
//! same way `webhooks.rs:56` and `doors.rs:53` do it. That means
//! `POST /api/admin/reload-config` changes SMTP settings without a restart,
//! rather than the settings being snapshotted at boot.
//!
//! What this does not do: queue, retry, or rate-limit. A send is one attempt
//! against one server. Callers that need durability need something this module
//! is not.

use std::sync::Arc;

use lettre::message::{Mailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{ConfigManager, EmailConfig};

/// Why a send did not happen.
///
/// Every variant is a distinct operator action, which is why this is not one
/// stringly-typed error: `Disabled` means edit the config, `Config` means the
/// config is wrong, `Transport` means the SMTP server said no.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// `email.enabled` is false. Not a failure -- a statement that this
    /// deployment does not send mail -- but never reported as success.
    #[error("email is disabled in this deployment's configuration")]
    Disabled,

    /// The configuration cannot produce a valid message or transport: an
    /// unparseable `from_email`, an empty host, a recipient that is not an
    /// address.
    #[error("email configuration is unusable: {0}")]
    Config(String),

    /// The SMTP conversation failed -- connection refused, TLS rejected,
    /// authentication refused, recipient refused.
    #[error("SMTP delivery failed: {0}")]
    Transport(String),
}

/// Which of the three SMTP dialects a configuration selects.
///
/// Split out as a plain function over a plain enum so the choice is testable
/// without a network, a runtime, or a server. The alternative -- deciding
/// inline while building the transport -- would make the single most
/// misconfiguration-prone line in this module reachable only by opening a
/// socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// TLS from the first byte. Port 465.
    ImplicitTls,
    /// Plaintext connection upgraded by STARTTLS. Port 587.
    StartTls,
    /// No encryption at all. Port 25.
    Plaintext,
}

/// `use_ssl` wins over `use_tls`, because a configuration setting both is
/// asking for the stricter of the two rather than the weaker.
///
/// Refusing the pair outright was considered and rejected: `use_tls` defaults
/// to `true`, so an operator who sets `use_ssl = true` for port 465 and touches
/// nothing else is *left* holding both flags. Refusing would turn the obvious
/// way to configure SMTPS into a server that will not boot.
///
/// The STARTTLS branch maps to lettre's `starttls_relay`, which is
/// `Tls::Required` rather than opportunistic. That distinction matters: an
/// opportunistic transport silently sends AUTH credentials in the clear when a
/// relay fails to offer STARTTLS, which is precisely the case where something
/// is already wrong.
pub fn transport_kind(email: &EmailConfig) -> TransportKind {
    if email.use_ssl {
        TransportKind::ImplicitTls
    } else if email.use_tls {
        TransportKind::StartTls
    } else {
        TransportKind::Plaintext
    }
}

/// The `From:` mailbox for a configuration.
///
/// An empty `from_name` yields a bare address rather than `" " <addr>`, which
/// some receivers render as an empty display name and others reject outright.
fn from_mailbox(email: &EmailConfig) -> Result<Mailbox, MailError> {
    let raw = if email.from_name.trim().is_empty() {
        email.from_email.clone()
    } else {
        format!("{} <{}>", email.from_name, email.from_email)
    };
    raw.parse::<Mailbox>()
        .map_err(|e| MailError::Config(format!("from address {raw:?} is not a mailbox: {e}")))
}

/// Build a plain-text message, without sending it.
///
/// Separated from [`MailService::send`] so that header construction -- the part
/// that can be wrong in a way nobody notices until a receiver files the mail as
/// spam -- is assertable in a unit test.
pub fn build_message(
    email: &EmailConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<Message, MailError> {
    let to_mailbox = to
        .parse::<Mailbox>()
        .map_err(|e| MailError::Config(format!("recipient {to:?} is not a mailbox: {e}")))?;

    MessageBuilder::new()
        .from(from_mailbox(email)?)
        .to(to_mailbox)
        .subject(subject)
        .body(body.to_string())
        .map_err(|e| MailError::Config(format!("could not build the message: {e}")))
}

/// Sends mail using whatever `[email]` says at the moment of the call.
#[derive(Clone)]
pub struct MailService {
    /// Read live rather than snapshotted, so an operator who fixes a wrong SMTP
    /// password and reloads does not also have to restart the server.
    config: Arc<ConfigManager>,
}

impl MailService {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self { config }
    }

    /// Whether this deployment sends mail at all.
    ///
    /// For callers that need to decide *before* doing work -- offering a
    /// password-reset form, say -- rather than discovering it from a
    /// [`MailError::Disabled`] afterwards.
    pub fn is_enabled(&self) -> bool {
        self.config.get_config().email.enabled
    }

    /// Deliver one plain-text message.
    ///
    /// Returns [`MailError::Disabled`] when email is switched off, which is not
    /// the same as success and must not be treated as such.
    pub async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        let email = self.config.get_config().email;

        if !email.enabled {
            return Err(MailError::Disabled);
        }
        if email.host.trim().is_empty() {
            return Err(MailError::Config(
                "email.enabled is true but email.host is empty".to_string(),
            ));
        }

        let message = build_message(&email, to, subject, body)?;
        let transport = self.transport(&email)?;

        transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| MailError::Transport(e.to_string()))
    }

    /// Build the SMTP transport described by `email`.
    ///
    /// Credentials are attached only when a username is set: an empty username
    /// with a `Credentials` attached makes lettre offer AUTH with an empty
    /// user, which relays that permit unauthenticated submission answer with a
    /// 535 rather than by ignoring it.
    fn transport(
        &self,
        email: &EmailConfig,
    ) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
        let host = email.host.trim();

        let builder = match transport_kind(email) {
            TransportKind::ImplicitTls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|e| MailError::Config(format!("implicit TLS relay {host:?}: {e}")))?,
            TransportKind::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                    .map_err(|e| MailError::Config(format!("STARTTLS relay {host:?}: {e}")))?
            }
            TransportKind::Plaintext => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            }
        };

        // Without an explicit timeout, an SMTP port that accepts a connection
        // and then says nothing holds this task until the OS TCP timeout --
        // minutes. The password-reset request handler is unauthenticated, so
        // that is a request-exhaustion lever anyone on the internet can pull by
        // pointing a deployment at a black hole, or by a relay simply going
        // dark. Twenty seconds is far longer than any healthy submission
        // handshake and far shorter than the kernel's patience.
        let builder = builder
            .port(email.port)
            .timeout(Some(std::time::Duration::from_secs(20)));

        let builder = if email.username.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(
                email.username.clone(),
                email.password.clone(),
            ))
        };

        Ok(builder.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> EmailConfig {
        EmailConfig {
            host: "smtp.example.invalid".to_string(),
            port: 587,
            username: "postmaster@example.invalid".to_string(),
            password: "unused-in-these-tests".to_string(),
            use_tls: true,
            use_ssl: false,
            from_email: "noreply@example.invalid".to_string(),
            from_name: "CSS".to_string(),
            enabled: true,
        }
    }

    #[test]
    fn the_default_config_selects_starttls() {
        // The shipped default is `use_tls = true, use_ssl = false, port = 587`.
        // If that ever stops meaning STARTTLS, every deployment that never
        // touched these fields silently changes dialect.
        assert_eq!(
            transport_kind(&EmailConfig::default()),
            TransportKind::StartTls
        );
    }

    #[test]
    fn use_ssl_selects_implicit_tls_even_alongside_use_tls() {
        let mut c = cfg();
        c.use_ssl = true;
        // Both true: the stricter one wins. A configuration that names both is
        // asking for encryption, not for the weaker of the two.
        assert!(c.use_tls);
        assert_eq!(transport_kind(&c), TransportKind::ImplicitTls);
    }

    #[test]
    fn neither_flag_is_plaintext() {
        let mut c = cfg();
        c.use_tls = false;
        c.use_ssl = false;
        assert_eq!(transport_kind(&c), TransportKind::Plaintext);
    }

    #[test]
    fn the_from_header_carries_the_configured_display_name() {
        let c = cfg();
        let msg = build_message(&c, "member@example.invalid", "Subject", "Body")
            .expect("a valid configuration builds a message");
        let formatted = String::from_utf8(msg.formatted()).expect("headers are ASCII here");

        assert!(
            formatted.contains("From: \"CSS\" <noreply@example.invalid>")
                || formatted.contains("From: CSS <noreply@example.invalid>"),
            "the From header did not carry both from_name and from_email:\n{formatted}"
        );
        assert!(
            formatted.contains("To: member@example.invalid"),
            "the To header did not carry the recipient:\n{formatted}"
        );
    }

    #[test]
    fn an_empty_display_name_yields_a_bare_address() {
        // Not cosmetic: `" " <addr>` is rendered as an empty display name by
        // some clients and rejected outright by others.
        let mut c = cfg();
        c.from_name = String::new();
        let mbox = from_mailbox(&c).expect("a bare address is a valid mailbox");
        assert_eq!(mbox.to_string(), "noreply@example.invalid");
    }

    #[test]
    fn an_unparseable_from_address_is_a_config_error_not_a_panic() {
        let mut c = cfg();
        c.from_email = "not an address".to_string();
        match build_message(&c, "member@example.invalid", "S", "B") {
            Err(MailError::Config(_)) => {}
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_recipient_is_a_config_error() {
        match build_message(&cfg(), "who?", "S", "B") {
            Err(MailError::Config(_)) => {}
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_disabled_deployment_reports_disabled_rather_than_success() {
        // The whole point of the Disabled variant. If this ever returns Ok, a
        // password-reset handler would report "check your email" to a member
        // who will never receive one.
        let mut app = crate::config::AppConfig::default();
        app.email.enabled = false;
        let svc = MailService::new(Arc::new(ConfigManager::new(app, None)));

        assert!(!svc.is_enabled());
        match svc.send("member@example.invalid", "S", "B").await {
            Err(MailError::Disabled) => {}
            other => panic!("expected Disabled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_enabled_deployment_with_no_host_is_a_config_error() {
        // Guards the guard above: without this, `send` returning Disabled for
        // every input would satisfy the previous test while being entirely
        // broken.
        let mut app = crate::config::AppConfig::default();
        app.email.enabled = true;
        app.email.host = "   ".to_string();
        let svc = MailService::new(Arc::new(ConfigManager::new(app, None)));

        match svc.send("member@example.invalid", "S", "B").await {
            Err(MailError::Config(msg)) => assert!(
                msg.contains("host"),
                "the error should name the empty host, got: {msg}"
            ),
            other => panic!("expected a Config error, got {other:?}"),
        }
    }
}
