//! Migrations must apply to a database whatever its encoding is.
//!
//! Found by the stack battery, on a LATIN1 cluster, at migration 20 of 26:
//!
//! ```text
//! Failed to run 2026-05-29-100000-0000_add_places with: character with byte
//! sequence 0xe2 0x80 0xa6 in encoding "UTF8" has no equivalent in encoding "LATIN1"
//! ```
//!
//! 0xe2 0x80 0xa6 is U+2026, a typographic ellipsis, and it was in a `--` SQL
//! comment. Not in an identifier, not in a default, not in seed data — in prose
//! nothing reads. But diesel sends the file as one statement string, so the
//! encoding conversion applies to the whole thing, and one character somebody's
//! editor helpfully substituted for three dots made the schema impossible to
//! create on a non-UTF-8 cluster.
//!
//! What makes this worth a permanent check rather than a one-line fix is how
//! invisible it is. Every developer machine and every CI runner defaults to
//! UTF-8, so the migrations apply everywhere they are ever tried, and the
//! failure waits for the first deployment onto an older cluster — where it
//! presents as a server that will not start, with a message about byte
//! sequences.
//!
//! ASCII is the right rule rather than a workaround. A migration is DDL: its
//! identifiers are ASCII by convention and its comments are for developers.
//! Text that genuinely needs to be non-ASCII belongs in a row, inserted by the
//! application, over a connection whose encoding the application controls.

use css_checks::repo_root;

/// Every `.sql` file under `server/migrations`.
fn migration_files() -> Vec<std::path::PathBuf> {
    let root = repo_root().join("server/migrations");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "sql") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn relative(path: &std::path::Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

#[test]
fn there_are_migrations_to_check() {
    // Without this, a change to the directory layout would make every check
    // below iterate over nothing and pass.
    let files = migration_files();
    assert!(
        files.len() >= 40,
        "found only {} .sql files under server/migrations; the scan is broken",
        files.len()
    );
}

#[test]
fn every_migration_is_ascii() {
    let mut offenders = Vec::new();

    for path in migration_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (lineno, line) in text.lines().enumerate() {
            for ch in line.chars() {
                if !ch.is_ascii() {
                    offenders.push(format!(
                        "{}:{}: U+{:04X} {:?} in: {}",
                        relative(&path),
                        lineno + 1,
                        ch as u32,
                        ch,
                        line.trim()
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these migrations contain characters a non-UTF-8 database cannot store, so \
         the schema cannot be created there at all:\n{}\n\n\
         diesel sends each file as one statement string, so the encoding \
         conversion covers comments too — an em dash or an ellipsis in a `--` \
         line is enough. Write ASCII: `--` for an em dash, `...` for an ellipsis. \
         Text that genuinely needs to be non-ASCII belongs in a row inserted by \
         the application, not in DDL.",
        offenders.join("\n")
    );
}

#[test]
fn every_migration_directory_has_both_halves() {
    // A missing down.sql is not a portability problem, but it is discovered the
    // same way — at the worst possible moment — and this is where the
    // migrations are already being walked.
    let root = repo_root().join("server/migrations");
    let mut missing = Vec::new();

    for entry in std::fs::read_dir(&root).expect("server/migrations must exist") {
        let path = entry.expect("readable").path();
        if !path.is_dir() {
            continue;
        }
        for half in ["up.sql", "down.sql"] {
            if !path.join(half).exists() {
                missing.push(format!("{}/{half}", relative(&path)));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "these migration halves are missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn migration_names_are_unique_and_ordered() {
    // Two migrations sharing a version is a coin toss about which one runs.
    let root = repo_root().join("server/migrations");
    let mut versions: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&root).expect("server/migrations must exist") {
        let path = entry.expect("readable").path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .expect("a directory has a name")
            .to_string_lossy()
            .to_string();
        // diesel's version is everything before the first `_`.
        let version = name.split('_').next().unwrap_or(&name).to_string();
        versions.push(version);
    }

    let mut sorted = versions.clone();
    sorted.sort();
    let mut unique = sorted.clone();
    unique.dedup();

    assert_eq!(
        sorted.len(),
        unique.len(),
        "duplicate migration versions: {:?}",
        sorted
            .iter()
            .enumerate()
            .filter(|(i, v)| *i > 0 && sorted[i - 1] == **v)
            .map(|(_, v)| v)
            .collect::<Vec<_>>()
    );

    assert!(
        versions.len() >= 20,
        "only {} migration directories; the scan is broken",
        versions.len()
    );
}

#[test]
fn there_is_exactly_one_migrations_root() {
    // The repository once carried `migrations/` at the root as well as
    // `server/migrations/`, and the two copies had drifted -- different
    // `DROP CONSTRAINT` forms and a different index `WHERE` scope. Only one is
    // embedded by `diesel_migrations::embed_migrations!`, so the other was a
    // second, subtly different schema that nothing ever applied and everybody
    // could read.
    let root = repo_root();
    let stray = root.join("migrations");
    assert!(
        !stray.exists(),
        "a second migrations directory exists at {}. Only server/migrations is \
         embedded into the binary; a duplicate is a schema nothing applies and \
         everybody reads.",
        stray.display()
    );
    assert!(
        root.join("server/migrations").is_dir(),
        "server/migrations is missing, which is the one that is embedded"
    );
}
