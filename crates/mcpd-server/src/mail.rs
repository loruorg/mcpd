//! Outgoing mail: verification links, password resets, invitations.
//!
//! One generic SMTP client, configured by flags. Mail is sent from a spawned
//! task, never inside the request, for two reasons. A slow mail server must not
//! hold a sign-up open, and the time a request takes must not depend on whether
//! a mail went out: sign-up and password reset answer the same way whether or
//! not the address has an account, and a request that waited for SMTP only in
//! one of the two cases would say which.
//!
//! With no SMTP server there are two behaviours, and `main.rs` picks one. On a
//! daemon that lets strangers in (`--signup` on a public address) the features
//! that need mail refuse with a 503 and the daemon warns at startup, because a
//! link written to a log is a link the operator can read. Everywhere else the
//! link is written to the log at info level, which is what a laptop or a
//! self-hosted cluster without a mail relay needs to finish an invitation.

use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::sync::{Arc, Mutex};

/// How the connection to the SMTP server is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmtpTls {
    /// STARTTLS on the submission port, normally 587. The default.
    #[default]
    StartTls,
    /// TLS from the first byte, normally port 465.
    Tls,
    /// No TLS. Only for a relay on the same host or network, such as a local
    /// MTA or a test server.
    None,
}

impl SmtpTls {
    /// Parse the `--smtp-tls` value.
    ///
    /// # Errors
    /// A message naming the three accepted values.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "starttls" | "" => Ok(Self::StartTls),
            "tls" | "smtps" => Ok(Self::Tls),
            "none" | "off" => Ok(Self::None),
            other => Err(format!(
                "--smtp-tls must be starttls, tls or none, got {other:?}; starttls is right for \
                 port 587 and tls for port 465"
            )),
        }
    }

    /// The name `mcpd config` prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartTls => "starttls",
            Self::Tls => "tls",
            Self::None => "none",
        }
    }
}

/// An SMTP server to send through.
#[derive(Clone, Default)]
pub struct SmtpConfig {
    /// The server's host name.
    pub host: String,
    /// Its port. `None` takes the usual port for the TLS mode.
    pub port: Option<u16>,
    /// The user to authenticate as, when the server wants one.
    pub user: Option<String>,
    /// That user's password.
    pub password: Option<String>,
    /// The `From` address, such as `mcpd <no-reply@example.com>`.
    pub from: String,
    /// How the connection is secured.
    pub tls: SmtpTls,
}

impl std::fmt::Debug for SmtpConfig {
    // By hand, so the password can never reach a log line through `{:?}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<set>"))
            .field("from", &self.from)
            .field("tls", &self.tls)
            .finish()
    }
}

/// What to do with a mail.
#[derive(Clone, Debug, Default)]
pub enum MailConfig {
    /// Send it through this server.
    Smtp(SmtpConfig),
    /// Write the link to the log at info level. For a daemon on a laptop, or a
    /// self-hosted one whose operator reads its log.
    #[default]
    Log,
    /// Refuse every feature that needs mail. For `--signup` on a public address
    /// with no SMTP server, where a logged link is a link the operator can use.
    Refuse,
    /// Keep it in memory, for tests to read back.
    Memory(Outbox),
}

impl MailConfig {
    /// The line `mcpd config` prints.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Smtp(s) => format!(
                "smtp {}:{} ({}), from {}",
                s.host,
                s.port.unwrap_or_else(|| default_port(s.tls)),
                s.tls.as_str(),
                s.from
            ),
            Self::Log => "no smtp server; links are written to the log".to_owned(),
            Self::Refuse => {
                "no smtp server; sign-up, password reset and invitations are refused".to_owned()
            }
            Self::Memory(_) => "kept in memory".to_owned(),
        }
    }
}

const fn default_port(tls: SmtpTls) -> u16 {
    match tls {
        SmtpTls::StartTls => 587,
        SmtpTls::Tls => 465,
        SmtpTls::None => 25,
    }
}

/// One mail, as the tests read it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    /// The recipient.
    pub to: String,
    /// The subject line.
    pub subject: String,
    /// The plain-text body.
    pub body: String,
}

/// Mail kept in memory.
#[derive(Clone, Debug, Default)]
pub struct Outbox(Arc<Mutex<Vec<Mail>>>);

impl Outbox {
    /// Every mail sent so far, oldest first.
    #[must_use]
    pub fn all(&self) -> Vec<Mail> {
        self.0.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// The newest mail to `to`, when there is one.
    #[must_use]
    pub fn last_to(&self, to: &str) -> Option<Mail> {
        self.all().into_iter().rev().find(|m| m.to.eq_ignore_ascii_case(to))
    }

    fn push(&self, mail: Mail) {
        if let Ok(mut m) = self.0.lock() {
            m.push(mail);
        }
    }
}

/// The sender the daemon holds.
pub struct Mailer {
    config: MailConfig,
    transport: Option<AsyncSmtpTransport<Tokio1Executor>>,
    from: Option<Mailbox>,
}

/// A mail configuration that cannot be used, found at startup.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// The `From` address does not parse.
    #[error("--smtp-from got {0:?}, which is not an address; write it as no-reply@example.com or \"mcpd <no-reply@example.com>\"")]
    From(String),
    /// The transport could not be built.
    #[error("the SMTP server {host:?} cannot be used: {reason}")]
    Transport {
        /// The host.
        host: String,
        /// Why.
        reason: String,
    },
}

impl Mailer {
    /// Build the sender. Nothing is contacted here; a wrong host shows up as a
    /// logged failure on the first mail.
    ///
    /// # Errors
    /// A [`MailError`] when the `From` address or the TLS setup is unusable.
    pub fn new(config: MailConfig) -> Result<Self, MailError> {
        let (transport, from) = match &config {
            MailConfig::Smtp(s) => {
                let from: Mailbox = s.from.parse().map_err(|_| MailError::From(s.from.clone()))?;
                let bad = |e: lettre::transport::smtp::Error| MailError::Transport {
                    host: s.host.clone(),
                    reason: e.to_string(),
                };
                let mut b = match s.tls {
                    SmtpTls::StartTls => {
                        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&s.host)
                            .map_err(bad)?
                    }
                    SmtpTls::Tls => {
                        AsyncSmtpTransport::<Tokio1Executor>::relay(&s.host).map_err(bad)?
                    }
                    SmtpTls::None => {
                        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&s.host)
                    }
                };
                b = b.port(s.port.unwrap_or_else(|| default_port(s.tls)));
                if let (Some(user), Some(password)) = (&s.user, &s.password) {
                    b = b.credentials(Credentials::new(user.clone(), password.clone()));
                }
                (Some(b.timeout(Some(std::time::Duration::from_secs(20))).build()), Some(from))
            }
            _ => (None, None),
        };
        Ok(Self { config, transport, from })
    }

    /// A mailer that writes links to the log. For tests and embedders that set
    /// nothing.
    #[must_use]
    pub fn log() -> Self {
        Self { config: MailConfig::Log, transport: None, from: None }
    }

    /// Whether mail can go anywhere at all. False only for
    /// [`MailConfig::Refuse`], and every door that needs mail checks this
    /// before doing anything else, so it refuses the same way for every email.
    #[must_use]
    pub fn can_send(&self) -> bool {
        !matches!(self.config, MailConfig::Refuse)
    }

    /// The configuration, for `mcpd config`.
    #[must_use]
    pub fn config(&self) -> &MailConfig {
        &self.config
    }

    /// Send a mail from a spawned task, and return at once.
    ///
    /// `link` is what a reader needs from the mail; in log mode it is the only
    /// part written, so the log does not fill with prose.
    pub fn send(
        &self,
        tasks: &tokio_util::task::TaskTracker,
        to: &str,
        subject: &str,
        body: String,
        link: Option<&str>,
    ) {
        match &self.config {
            MailConfig::Refuse => {}
            MailConfig::Memory(outbox) => {
                outbox.push(Mail { to: to.to_owned(), subject: subject.to_owned(), body });
            }
            MailConfig::Log => {
                tracing::info!(
                    to,
                    subject,
                    link = link.unwrap_or(""),
                    "mail not sent: no smtp server"
                );
            }
            MailConfig::Smtp(_) => {
                let (Some(transport), Some(from)) = (self.transport.clone(), self.from.clone())
                else {
                    return;
                };
                let Ok(rcpt) = to.parse::<Mailbox>() else {
                    tracing::warn!("mail not sent: the recipient is not an address");
                    return;
                };
                let message = Message::builder()
                    .from(from)
                    .to(rcpt)
                    .subject(subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(body);
                let message = match message {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("mail not built: {e}");
                        return;
                    }
                };
                let subject = subject.to_owned();
                tasks.spawn(async move {
                    // The recipient is not logged: it is personal data, and the
                    // subject says which kind of mail failed.
                    if let Err(e) = transport.send(message).await {
                        tracing::error!(subject, "mail not sent: {e}");
                    }
                });
            }
        }
    }
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer").field("config", &self.config).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tls_mode_parses_and_names_the_choices_when_it_does_not() {
        assert_eq!(SmtpTls::parse("STARTTLS").unwrap(), SmtpTls::StartTls);
        assert_eq!(SmtpTls::parse("tls").unwrap(), SmtpTls::Tls);
        assert_eq!(SmtpTls::parse("none").unwrap(), SmtpTls::None);
        assert!(SmtpTls::parse("ssl3").unwrap_err().contains("starttls, tls or none"));
    }

    #[test]
    fn the_password_never_reaches_debug_output() {
        let c = SmtpConfig { password: Some("hunter2-secret".into()), ..SmtpConfig::default() };
        assert!(!format!("{c:?}").contains("hunter2-secret"));
    }

    #[test]
    fn a_bad_from_address_is_refused_at_startup() {
        let c = SmtpConfig {
            host: "localhost".into(),
            from: "not an address".into(),
            ..SmtpConfig::default()
        };
        assert!(matches!(Mailer::new(MailConfig::Smtp(c)), Err(MailError::From(_))));
    }

    #[tokio::test]
    async fn the_memory_outbox_keeps_what_was_sent() {
        let outbox = Outbox::default();
        let m = Mailer::new(MailConfig::Memory(outbox.clone())).unwrap();
        let tasks = tokio_util::task::TaskTracker::new();
        m.send(&tasks, "ana@example.com", "Hello", "body".into(), None);
        assert_eq!(outbox.last_to("ANA@example.com").unwrap().subject, "Hello");
        assert!(Mailer::new(MailConfig::Refuse).unwrap().can_send().eq(&false));
    }
}
