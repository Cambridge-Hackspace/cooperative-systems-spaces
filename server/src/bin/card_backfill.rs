//! Seal existing card codes in place (#108).
//!
//! The migration that added `code_encrypted` / `code_nonce` / `code_bidx` could
//! not fill them. Sealing needs the deployment's key, and a key reachable from
//! SQL is a key in the migration file, in the server log, and in
//! `pg_stat_statements` -- which is precisely the disclosure being prevented.
//! So the backfill is a separate, operator-run step that holds the key in
//! memory and nowhere else.
//!
//! Idempotent: it only touches rows whose `code_bidx` is null, so it can be
//! re-run after a partial failure, after a `toolpass_load`, or simply to check
//! that nothing is outstanding.
//!
//! `--verify` re-opens every sealed row and compares it against the plaintext
//! still beside it. That check exists because the plaintext is about to be
//! dropped in a later migration, and the moment to discover a key mismatch is
//! while the original is still there to re-derive from -- not afterwards.
//!
//! Takes keys on the command line or from the environment rather than reading
//! the app config, matching `toolpass_load`: a tool an operator runs by hand
//! should not quietly inherit a server's configuration.

use std::error::Error;

use diesel::pg::PgConnection;
use diesel::prelude::*;

use css_lib::card_crypto::CardCipher;
use css_server::schema::user_cards;

fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
        if let Some(rest) = a.strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn main() -> Result<(), Box<dyn Error>> {
    let database_url = arg("--database-url")
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or("--database-url or DATABASE_URL is required")?;
    let enc = arg("--encryption-key")
        .or_else(|| std::env::var("CSS_CARDS_ENCRYPTION_KEY").ok())
        .ok_or("--encryption-key or CSS_CARDS_ENCRYPTION_KEY is required")?;
    let idx = arg("--index-key")
        .or_else(|| std::env::var("CSS_CARDS_INDEX_KEY").ok())
        .ok_or("--index-key or CSS_CARDS_INDEX_KEY is required")?;
    let pepper = arg("--device-pepper")
        .or_else(|| std::env::var("CSS_CARDS_DEVICE_PEPPER").ok())
        .ok_or("--device-pepper or CSS_CARDS_DEVICE_PEPPER is required")?;

    let cipher = CardCipher::from_hex(&enc, &idx, &pepper)?;
    let mut conn = PgConnection::establish(&database_url)?;

    if flag("--verify") {
        return verify(&mut conn, &cipher);
    }

    let dry_run = flag("--dry-run");

    let pending: Vec<(uuid::Uuid, String)> = user_cards::table
        .filter(user_cards::code_bidx.is_null())
        .select((user_cards::id, user_cards::code))
        .load(&mut conn)?;

    let total: i64 = user_cards::table.count().get_result(&mut conn)?;
    println!("{} card(s) total, {} unsealed", total, pending.len());

    if pending.is_empty() {
        println!("nothing to do");
        return Ok(());
    }
    if dry_run {
        println!("--dry-run: stopping before writing");
        return Ok(());
    }

    let mut sealed = 0usize;
    for (id, code) in &pending {
        let s = cipher.seal(code)?;
        diesel::update(user_cards::table.find(id))
            .set((
                user_cards::code_encrypted.eq(Some(s.ciphertext)),
                user_cards::code_nonce.eq(Some(s.nonce)),
                user_cards::code_bidx.eq(Some(s.blind_index)),
                user_cards::code_wire_digest.eq(Some(cipher.wire_digest(code)?)),
            ))
            .execute(&mut conn)?;
        sealed += 1;
    }
    println!("sealed {sealed} card(s)");

    // Always verify what was just written. A backfill that reports success on
    // rows nobody can open is the failure this whole step exists to avoid, and
    // the plaintext to compare against is still right there.
    verify(&mut conn, &cipher)
}

/// Every sealed row must open to the plaintext beside it.
fn verify(conn: &mut PgConnection, cipher: &CardCipher) -> Result<(), Box<dyn Error>> {
    type Row = (
        uuid::Uuid,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    );
    let rows: Vec<Row> = user_cards::table
        .select((
            user_cards::id,
            user_cards::code,
            user_cards::code_encrypted,
            user_cards::code_nonce,
            user_cards::code_bidx,
            user_cards::code_wire_digest,
        ))
        .load(conn)?;

    let mut checked = 0usize;
    let mut unsealed = 0usize;
    let mut bad: Vec<uuid::Uuid> = Vec::new();

    for (id, code, ct, nonce, bidx, wire) in rows {
        match (ct, nonce, bidx, wire) {
            (Some(ct), Some(nonce), Some(bidx), Some(wire)) => {
                checked += 1;
                let opened = cipher.open(&ct, &nonce).ok();
                // Three halves, which fail for three different reasons:
                // unreadable (the sealed value does not round-trip), unfindable
                // (the server cannot resolve a swipe to this row), and
                // unusable-offline (a device holding the digest would not
                // recognise the card).
                let wire_ok = cipher
                    .wire_digest(&code)
                    .map(|d| d == wire)
                    .unwrap_or(false);
                if opened.as_deref() != Some(code.as_str())
                    || bidx != cipher.blind_index(&code)
                    || !wire_ok
                {
                    bad.push(id);
                }
            }
            _ => unsealed += 1,
        }
    }

    println!("verified {checked} sealed card(s); {unsealed} still unsealed");
    if bad.is_empty() {
        println!("all sealed cards open to their plaintext and index correctly");
        if unsealed > 0 {
            println!(
                "NOTE: {unsealed} row(s) are not sealed yet. Do not drop user_cards.code until \
                 this is zero."
            );
        }
        Ok(())
    } else {
        // Ids only. The whole point is not to print card codes.
        Err(format!(
            "{} card(s) do not match their plaintext -- wrong key, or written by a different \
             deployment. Ids: {:?}",
            bad.len(),
            bad
        )
        .into())
    }
}
