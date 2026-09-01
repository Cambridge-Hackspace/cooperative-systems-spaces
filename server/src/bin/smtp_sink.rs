//! `css-smtp-sink` -- a tiny SMTP server that keeps what it is sent.
//!
//! The stack battery needs somewhere for the mailer to deliver, and everything
//! below the stack tier can only establish that a message was *constructed*.
//! Whether the server actually speaks SMTP -- EHLO, MAIL FROM, RCPT TO, DATA --
//! is a separate claim, and the evidence that it needed making is in the issue
//! this was written for: the deployed binary contained zero occurrences of
//! `EHLO`, `STARTTLS` or `smtp` while `[email]` sat fully configured.
//!
//! Modeled on `css-webhook-recvr`, the existing in-repo sink for the webhook
//! dispatcher, and for the same reason: a purpose-built binary built alongside
//! the server needs no container image, so it adds no digest to pin in both
//! `e2e/images.env` and `.reaper.toml`, and no image to pull in CI.
//!
//! Each accepted message is written to one file in `--maildir`, envelope first:
//!
//! ```text
//! X-Sink-Mail-From: <noreply@example.invalid>
//! X-Sink-Rcpt-To: <ada@example.invalid>
//! X-Sink-Received: 1
//!
//! From: "CSS E2E" <noreply@example.invalid>
//! To: ada@example.invalid
//! Subject: Reset your password
//! ...
//! ```
//!
//! The envelope is recorded separately from the headers on purpose. They are
//! different things, they can disagree, and a test that only ever reads `From:`
//! cannot tell whether `from_email` reached the envelope at all.
//!
//! Deliberately **not** implemented: STARTTLS, AUTH, pipelining, SIZE limits,
//! and any notion of rejecting a recipient. This exists to observe what the
//! server sends, not to be a mail server. `e2e/stack-config.toml` sets
//! `use_tls = false` and no username, which is what makes that sufficient.
//!
//! Blocking std sockets and one thread per connection, not tokio: connections
//! here are counted in single digits per run, and a blocking read loop is far
//! easier to be confident in than an async one when the thing it is testing is
//! somebody else's protocol implementation.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use clap::Parser;

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
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    fs::create_dir_all(&args.maildir)?;

    let listener = TcpListener::bind(&args.bind)?;
    println!(
        "css-smtp-sink listening on {}, writing to {}",
        args.bind,
        args.maildir.display()
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
        std::thread::spawn(move || {
            if let Err(e) = serve(stream, &maildir, &counter) {
                eprintln!("session ended: {e}");
            }
        });
    }
    Ok(())
}

/// One SMTP session, from greeting to QUIT.
fn serve(stream: TcpStream, maildir: &Path, counter: &AtomicU64) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let say = |w: &mut TcpStream, line: &str| -> std::io::Result<()> {
        w.write_all(line.as_bytes())?;
        w.write_all(b"\r\n")?;
        w.flush()
    };

    say(&mut writer, "220 css-smtp-sink ESMTP ready")?;

    let mut mail_from = String::new();
    let mut rcpt_to: Vec<String> = Vec::new();
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(()); // client hung up
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let verb = trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();

        match verb.as_str() {
            "EHLO" => {
                // No STARTTLS and no AUTH advertised. A client configured for
                // either fails loudly here rather than silently downgrading,
                // which is the outcome we want from a test double.
                say(&mut writer, "250-css-smtp-sink")?;
                say(&mut writer, "250 8BITMIME")?;
            }
            "HELO" => say(&mut writer, "250 css-smtp-sink")?,
            "MAIL" => {
                mail_from = angle_addr(trimmed);
                say(&mut writer, "250 2.1.0 Ok")?;
            }
            "RCPT" => {
                rcpt_to.push(angle_addr(trimmed));
                say(&mut writer, "250 2.1.5 Ok")?;
            }
            "DATA" => {
                say(&mut writer, "354 End data with <CR><LF>.<CR><LF>")?;
                let body = read_data(&mut reader)?;
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                write_message(maildir, n, &mail_from, &rcpt_to, &body)?;
                println!("stored message {n} for {rcpt_to:?}");
                std::io::stdout().flush()?;
                say(&mut writer, &format!("250 2.0.0 Ok: queued as {n}"))?;
                mail_from.clear();
                rcpt_to.clear();
            }
            "RSET" => {
                mail_from.clear();
                rcpt_to.clear();
                say(&mut writer, "250 2.0.0 Ok")?;
            }
            "NOOP" => say(&mut writer, "250 2.0.0 Ok")?,
            "QUIT" => {
                say(&mut writer, "221 2.0.0 Bye")?;
                return Ok(());
            }
            _ => say(&mut writer, "502 5.5.2 Command not implemented")?,
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
fn read_data(reader: &mut BufReader<TcpStream>) -> std::io::Result<String> {
    let mut body = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
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
    body: &str,
) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str(&format!("X-Sink-Mail-From: <{mail_from}>\n"));
    for rcpt in rcpt_to {
        out.push_str(&format!("X-Sink-Rcpt-To: <{rcpt}>\n"));
    }
    out.push_str(&format!("X-Sink-Received: {n}\n\n"));
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
    use super::angle_addr;

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
}
