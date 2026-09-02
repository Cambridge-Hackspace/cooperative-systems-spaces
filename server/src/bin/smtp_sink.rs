//! `css-smtp-sink` -- a tiny SMTP server that keeps what it is sent.
//!
//! The stack battery needs somewhere for the mailer to deliver, and everything
//! below the stack tier can only establish that a message was *constructed*.
//! Whether the server actually speaks SMTP -- EHLO, STARTTLS, MAIL FROM, RCPT
//! TO, DATA -- is a separate claim, and the evidence that it needed making is
//! in the issue this was written for: the deployed binary contained zero
//! occurrences of `EHLO`, `STARTTLS` or `smtp` while `[email]` sat fully
//! configured.
//!
//! Modeled on `css-webhook-recvr`, the existing in-repo sink for the webhook
//! dispatcher, and for the same reason: a purpose-built binary built alongside
//! the server needs no container image, so it adds no digest to pin in both
//! `e2e/images.env` and `.reaper.toml`, and no image to pull in CI.
//!
//! ## STARTTLS
//!
//! Supported, and that is the point of it. `use_tls = true` on port 587 is what
//! a real deployment uses, and it is the branch of `MailService::transport`
//! most likely to break against a real relay: certificate verification, the
//! upgrade handshake, and lettre's `Tls::Required` refusing to fall back. A
//! sink that only spoke cleartext would leave that path proven by a unit test
//! asserting which *enum variant* gets selected, which is not the same claim.
//!
//! It is offered, never required, so one sink serves both `use_tls = true` and
//! `use_tls = false` and the driver can exercise each. The certificate is
//! generated per run by `e2e/stack.sh` and trusted by css-server through
//! `SSL_CERT_FILE`; it is a throwaway for a loopback listener and never leaves
//! the machine.
//!
//! Each accepted message is written to one file in `--maildir`, envelope first:
//!
//! ```text
//! X-Sink-Mail-From: <noreply@example.invalid>
//! X-Sink-Rcpt-To: <ada@example.invalid>
//! X-Sink-Received: 1
//! X-Sink-Transport: starttls
//!
//! From: "CSS E2E" <noreply@example.invalid>
//! ...
//! ```
//!
//! The envelope is recorded separately from the headers on purpose. They are
//! different things, they can disagree, and a test that only reads `From:`
//! cannot tell whether `from_email` reached the envelope at all.
//! `X-Sink-Transport` is there so a driver can assert a message arrived *over
//! TLS* rather than merely arriving.
//!
//! Deliberately **not** implemented: AUTH, pipelining, SIZE limits, and any
//! notion of rejecting a recipient. This exists to observe what the server
//! sends, not to be a mail server.
//!
//! Blocking std sockets and one thread per connection, not tokio: connections
//! here are counted in single digits per run, and a blocking read loop is far
//! easier to be confident in than an async one when the thing it is testing is
//! somebody else's protocol implementation.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use clap::Parser;
use native_tls::{Identity, TlsAcceptor};

#[derive(Parser, Debug)]
#[command(
    name = "css-smtp-sink",
    about = "Test SMTP sink that stores what it is sent"
)]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "SMTP_SINK_BIND", default_value = "127.0.0.1:2525")]
    bind: String,

    /// Directory to write accepted messages into. Created if absent.
    #[arg(long, env = "SMTP_SINK_MAILDIR", default_value = "./mail")]
    maildir: PathBuf,

    /// PEM certificate to serve for STARTTLS. Both this and --tls-key must be
    /// given, or STARTTLS is not advertised at all.
    #[arg(long, env = "SMTP_SINK_TLS_CERT")]
    tls_cert: Option<PathBuf>,

    /// PEM private key matching --tls-cert.
    #[arg(long, env = "SMTP_SINK_TLS_KEY")]
    tls_key: Option<PathBuf>,
}

/// How a message reached us. Recorded on the message so a driver can assert
/// that a send configured for TLS actually used it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Plain,
    StartTls,
}

impl Transport {
    fn as_str(self) -> &'static str {
        match self {
            Transport::Plain => "plain",
            Transport::StartTls => "starttls",
        }
    }
}

/// What a command loop stopped for.
enum Outcome {
    /// The session ended: QUIT, or the client hung up.
    Done,
    /// The client asked to upgrade, and `220` has already been sent.
    StartTls,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    fs::create_dir_all(&args.maildir)?;

    let acceptor = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => {
            let identity = Identity::from_pkcs8(&fs::read(cert)?, &fs::read(key)?)
                .map_err(|e| std::io::Error::other(format!("bad TLS identity: {e}")))?;
            let acceptor = TlsAcceptor::new(identity)
                .map_err(|e| std::io::Error::other(format!("TLS acceptor: {e}")))?;
            Some(Arc::new(acceptor))
        }
        // Both or neither. One without the other is a misconfiguration that
        // would otherwise surface as "STARTTLS was never offered", three layers
        // away from its cause.
        (None, None) => None,
        _ => {
            return Err(std::io::Error::other(
                "--tls-cert and --tls-key must be given together",
            ))
        }
    };

    let listener = TcpListener::bind(&args.bind)?;
    println!(
        "css-smtp-sink listening on {}, writing to {}, starttls {}",
        args.bind,
        args.maildir.display(),
        if acceptor.is_some() {
            "offered"
        } else {
            "not offered"
        }
    );
    // Flushed so a driver waiting on this line is not blocked by block
    // buffering when stdout is a pipe, which it is under every stage runner.
    std::io::stdout().flush()?;

    let counter = Arc::new(AtomicU64::new(0));
    let maildir = Arc::new(args.maildir);

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept failed: {e}");
                continue;
            }
        };
        let counter = counter.clone();
        let maildir = maildir.clone();
        let acceptor = acceptor.clone();
        std::thread::spawn(move || {
            if let Err(e) = serve(stream, &maildir, &counter, acceptor.as_deref()) {
                eprintln!("session ended: {e}");
            }
        });
    }
    Ok(())
}

/// One SMTP session, from greeting to QUIT, upgrading once if asked.
fn serve(
    stream: TcpStream,
    maildir: &Path,
    counter: &AtomicU64,
    acceptor: Option<&TlsAcceptor>,
) -> std::io::Result<()> {
    let mut io = BufReader::new(stream);
    say(io.get_mut(), "220 css-smtp-sink ESMTP ready")?;

    match run_commands(
        &mut io,
        maildir,
        counter,
        acceptor.is_some(),
        Transport::Plain,
    )? {
        Outcome::Done => Ok(()),
        Outcome::StartTls => {
            let acceptor = acceptor.expect("StartTls is only returned when an acceptor exists");
            let upgraded = acceptor
                .accept(io.into_inner())
                .map_err(|e| std::io::Error::other(format!("TLS handshake: {e}")))?;

            // `false` here is not a policy choice, it is what stops this
            // function instantiating itself forever: a second upgrade would
            // need `run_commands` over `TlsStream<TlsStream<..>>` and so on
            // without end. A client sending STARTTLS twice gets 503, which is
            // what a real server does anyway.
            let mut tls_io = BufReader::new(upgraded);
            run_commands(&mut tls_io, maildir, counter, false, Transport::StartTls)?;
            Ok(())
        }
    }
}

fn say<W: Write>(w: &mut W, line: &str) -> std::io::Result<()> {
    w.write_all(line.as_bytes())?;
    w.write_all(b"\r\n")?;
    w.flush()
}

/// The command loop, over whichever stream we are currently speaking.
fn run_commands<S: Read + Write>(
    io: &mut BufReader<S>,
    maildir: &Path,
    counter: &AtomicU64,
    offer_starttls: bool,
    transport: Transport,
) -> std::io::Result<Outcome> {
    let mut mail_from = String::new();
    let mut rcpt_to: Vec<String> = Vec::new();
    let mut line = String::new();

    loop {
        line.clear();
        if io.read_line(&mut line)? == 0 {
            return Ok(Outcome::Done); // client hung up
        }
        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
        let verb = trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();

        match verb.as_str() {
            "EHLO" => {
                say(io.get_mut(), "250-css-smtp-sink")?;
                if offer_starttls {
                    say(io.get_mut(), "250-STARTTLS")?;
                }
                // No AUTH, ever. A server that started requiring it fails
                // loudly here rather than quietly sending credentials nowhere.
                say(io.get_mut(), "250 8BITMIME")?;
            }
            "HELO" => say(io.get_mut(), "250 css-smtp-sink")?,
            "STARTTLS" => {
                if offer_starttls {
                    say(io.get_mut(), "220 2.0.0 Ready to start TLS")?;
                    return Ok(Outcome::StartTls);
                }
                say(io.get_mut(), "503 5.5.1 STARTTLS not available")?;
            }
            "MAIL" => {
                mail_from = angle_addr(&trimmed);
                say(io.get_mut(), "250 2.1.0 Ok")?;
            }
            "RCPT" => {
                rcpt_to.push(angle_addr(&trimmed));
                say(io.get_mut(), "250 2.1.5 Ok")?;
            }
            "DATA" => {
                say(io.get_mut(), "354 End data with <CR><LF>.<CR><LF>")?;
                let body = read_data(io)?;
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                write_message(maildir, n, &mail_from, &rcpt_to, transport, &body)?;
                println!(
                    "stored message {n} for {rcpt_to:?} over {}",
                    transport.as_str()
                );
                std::io::stdout().flush()?;
                say(io.get_mut(), &format!("250 2.0.0 Ok: queued as {n}"))?;
                mail_from.clear();
                rcpt_to.clear();
            }
            "RSET" => {
                mail_from.clear();
                rcpt_to.clear();
                say(io.get_mut(), "250 2.0.0 Ok")?;
            }
            "NOOP" => say(io.get_mut(), "250 2.0.0 Ok")?,
            "QUIT" => {
                say(io.get_mut(), "221 2.0.0 Bye")?;
                return Ok(Outcome::Done);
            }
            _ => say(io.get_mut(), "502 5.5.2 Command not implemented")?,
        }
    }
}

/// The address inside angle brackets, or the whole argument if there are none.
///
/// `MAIL FROM:<a@b>` and `RCPT TO:<a@b>` both arrive with the colon attached to
/// the verb's second word, and lettre sends parameters after them, so this
/// takes the bracketed span rather than splitting on whitespace.
fn angle_addr(line: &str) -> String {
    match (line.find('<'), line.rfind('>')) {
        (Some(a), Some(b)) if b > a => line[a + 1..b].to_string(),
        _ => line
            .split_once(':')
            .map(|(_, rest)| rest.trim().to_string())
            .unwrap_or_default(),
    }
}

/// Read until the lone `.` terminator, undoing dot-stuffing.
fn read_data<S: Read + Write>(io: &mut BufReader<S>) -> std::io::Result<String> {
    let mut body = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if io.read_line(&mut line)? == 0 {
            break; // hung up mid-message; keep what arrived
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "." {
            break;
        }
        // RFC 5321 4.5.2: a leading '.' in the payload is doubled on the wire.
        // Leaving it in would corrupt any line of a message body that happens
        // to start with a period.
        body.push_str(trimmed.strip_prefix("..").unwrap_or(trimmed));
        body.push('\n');
    }
    Ok(body)
}

fn write_message(
    maildir: &Path,
    n: u64,
    mail_from: &str,
    rcpt_to: &[String],
    transport: Transport,
    body: &str,
) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str(&format!("X-Sink-Mail-From: <{mail_from}>\n"));
    for rcpt in rcpt_to {
        out.push_str(&format!("X-Sink-Rcpt-To: <{rcpt}>\n"));
    }
    out.push_str(&format!("X-Sink-Received: {n}\n"));
    out.push_str(&format!("X-Sink-Transport: {}\n\n", transport.as_str()));
    out.push_str(body);

    // Written to a temporary name and renamed, so a driver polling the
    // directory cannot read a half-written file and conclude the message is
    // malformed. Rename within one directory is atomic on every filesystem this
    // runs on.
    let final_path = maildir.join(format!("{n:04}.eml"));
    let temp_path = maildir.join(format!(".{n:04}.eml.partial"));
    fs::write(&temp_path, out)?;
    fs::rename(&temp_path, &final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_bracketed_address() {
        assert_eq!(
            angle_addr("MAIL FROM:<noreply@example.invalid>"),
            "noreply@example.invalid"
        );
        assert_eq!(
            angle_addr("RCPT TO:<ada@example.invalid>"),
            "ada@example.invalid"
        );
    }

    #[test]
    fn ignores_parameters_after_the_address() {
        // lettre sends BODY=8BITMIME on MAIL FROM. Splitting on whitespace
        // would capture it as part of the address.
        assert_eq!(
            angle_addr("MAIL FROM:<a@b.invalid> BODY=8BITMIME SIZE=1234"),
            "a@b.invalid"
        );
    }

    #[test]
    fn falls_back_when_there_are_no_brackets() {
        // Not required by the RFC for these verbs, but a sink that returned an
        // empty string here would make a driver's envelope assertion fail in a
        // way that pointed at the server rather than at this file.
        assert_eq!(angle_addr("MAIL FROM: a@b.invalid"), "a@b.invalid");
    }

    #[test]
    fn the_two_transports_are_named_distinctly() {
        // The whole value of X-Sink-Transport is that a driver can tell them
        // apart. If these ever collided, an assertion that a message arrived
        // over TLS would pass for one that did not.
        assert_ne!(Transport::Plain.as_str(), Transport::StartTls.as_str());
    }
}
