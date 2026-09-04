//! A generated device code must fit its column in bytes, on any cluster.
//!
//! `device_code` is `VARCHAR(n)`, and on a byte-counted cluster (SQL_ASCII) that
//! `n` is a BYTE limit, not a character one. `new_device_code` builds a code from
//! eight emoji drawn from an alphabet that mixes 3-, 4- and 6-7-byte characters,
//! so the worst-case code is `8 * (widest emoji)` bytes. When that exceeded
//! `VARCHAR(32)` the insert failed with SQLSTATE 22001 -- an intermittent 500 on
//! ~a third of invites, and the reason the concurrency and fuzz nightly went
//! non-deterministic before the column was widened.
//!
//! This is the fast oracle for that defect. It reads the alphabet and the code
//! length from the Rust source and the column width from the migrations, and
//! fails at `cargo test` speed if the worst-case code no longer fits -- whether
//! because someone narrowed the column or added a wider emoji. The battery would
//! catch it too, but only sometimes and only after a stack came up; this catches
//! it always and in milliseconds.

use std::fs;

use css_checks::{read, repo_root};

/// Strip `//` line comments so a quote inside a comment cannot be read as an
/// alphabet entry.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The emoji alphabet in `new_device_code`, as the byte length of each entry.
///
/// Each entry is a Rust string literal `"…"`; the source file is UTF-8, so the
/// bytes between the quotes are exactly the emoji's UTF-8 encoding and their
/// count is the byte length Postgres stores. The entries here contain no escapes,
/// so splitting on `"` yields the literal contents at the odd indices (0 is the
/// text before the first quote, 1 is the first literal, 2 the separator, …).
fn alphabet_byte_lengths(devices_rs: &str) -> Vec<usize> {
    let start = devices_rs
        .find("let emojis = [")
        .expect("could not find `let emojis = [` in models/devices.rs");
    let rest = &devices_rs[start..];
    let end = rest
        .find("];")
        .expect("could not find the end of the emojis array");
    let block = strip_line_comments(&rest[..end]);

    block
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.len())
        .collect()
}

/// How many emoji a code is: the `N` in `(0..N)` inside `new_device_code`.
fn code_length(devices_rs: &str) -> usize {
    let f = devices_rs
        .find("fn new_device_code")
        .expect("no new_device_code in models/devices.rs");
    let body = &devices_rs[f..];
    let at = body
        .find("(0..")
        .expect("no `(0..N)` count in new_device_code");
    let tail = &body[at + 4..];
    let n: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse().expect("could not parse the code length")
}

/// The effective `device_code` column width: the `VARCHAR(n)` from the latest
/// migration that sets the column's type. Later migrations win, so an `ALTER …
/// TYPE VARCHAR(64)` overrides the original `device_code VARCHAR(32)`.
fn effective_column_width() -> usize {
    let dir = repo_root().join("server/migrations");
    let mut hits: Vec<(String, usize)> = Vec::new();
    for entry in fs::read_dir(&dir)
        .expect("cannot read migrations dir")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        let up = entry.path().join("up.sql");
        let Ok(sql) = fs::read_to_string(&up) else {
            continue;
        };
        for line in sql.lines() {
            if !line.contains("device_code") || !line.contains("VARCHAR(") {
                continue;
            }
            let at = line.find("VARCHAR(").unwrap() + "VARCHAR(".len();
            let digits: String = line[at..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(w) = digits.parse::<usize>() {
                hits.push((name.clone(), w));
            }
        }
    }
    assert!(
        !hits.is_empty(),
        "no migration declares a VARCHAR width for device_code; this check cannot \
         verify the byte budget and must not silently pass"
    );
    hits.sort();
    hits.last().unwrap().1
}

#[test]
fn eight_emoji_fit_device_code_in_bytes() {
    let devices_rs = read("server/src/models/devices.rs");
    let lengths = alphabet_byte_lengths(&devices_rs);
    assert!(
        lengths.len() > 100,
        "read only {} alphabet entries; the parse is wrong and this check would \
         assert nothing",
        lengths.len()
    );

    let widest = *lengths.iter().max().unwrap();
    let count = code_length(&devices_rs);
    let worst_case = count * widest;
    let width = effective_column_width();

    assert!(
        worst_case <= width,
        "a device_code of {count} emoji at up to {widest} bytes each is {worst_case} \
         bytes worst-case, but device_code is VARCHAR({width}). On a byte-counted \
         cluster (SQL_ASCII) that overflows and the insert fails with SQLSTATE \
         22001 -- an intermittent 500. Widen the column or drop the wide \
         (variation-selector) emoji from the alphabet."
    );
}
