use console::style;
use tabled::{Table, Tabled};

use crate::client::{UserResponse, UserRole};

pub fn print_user_info(user: &UserResponse) {
    println!("{}", style("User Information").bold().underlined());
    println!("ID: {}", user.id);
    println!("Username: {}", user.username);
    println!("Email: {}", user.email);
    println!("Full Name: {}", user.full_name);
    println!("Role: {}", format_role(&user.role));
    println!("Active: {}", format_bool(user.is_active));
    println!("Created: {}", user.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("Updated: {}", user.updated_at.format("%Y-%m-%d %H:%M:%S UTC"));
}

pub fn print_users_table(users: &[UserResponse]) {
    if users.is_empty() {
        println!("No users found");
        return;
    }

    #[derive(Tabled)]
    struct UserRow {
        #[tabled(rename = "ID")]
        id: String,
        #[tabled(rename = "Username")]
        username: String,
        #[tabled(rename = "Email")]
        email: String,
        #[tabled(rename = "Full Name")]
        full_name: String,
        #[tabled(rename = "Role")]
        role: String,
        #[tabled(rename = "Active")]
        active: String,
        #[tabled(rename = "Created")]
        created: String,
    }

    let rows: Vec<UserRow> = users
        .iter()
        .map(|user| UserRow {
            id: format_uuid(&user.id),
            username: user.username.clone(),
            email: user.email.clone(),
            full_name: user.full_name.clone(),
            role: format_role(&user.role),
            active: format_bool(user.is_active),
            created: user.created_at.format("%Y-%m-%d").to_string(),
        })
        .collect();

    let table = Table::new(rows);
    println!("{}", table);
}

fn format_role(role: &UserRole) -> String {
    let colored = match role {
        UserRole::Unknown => style("Unknown").dim(),
        UserRole::Newbie => style("Newbie").yellow(),
        UserRole::Member => style("Member").green(),
        UserRole::Staff => style("Staff").blue(),
        UserRole::Admin => style("Admin").red().bold(),
    };
    colored.to_string()
}

fn format_bool(value: bool) -> String {
    if value {
        style("Yes").green().to_string()
    } else {
        style("No").red().to_string()
    }
}

/// Abbreviate a UUID for a table cell: `550e8400...0000`.
///
/// The slices are guarded rather than direct. A hyphenated UUID is always 36
/// characters so the original `&s[..8]` / `&s[len-4..]` could not panic in
/// practice, but they were unguarded indexing on a stringified value, and
/// `clippy::indexing_slicing` is denied across this workspace. Guarding also
/// makes the degenerate case well-defined instead of accidental: below 16
/// characters the abbreviation would be no shorter than the original and the
/// two slices would overlap, so the value is returned whole.
fn format_uuid(uuid: &uuid::Uuid) -> String {
    let uuid_str = uuid.to_string();
    const HEAD: usize = 8;
    const TAIL: usize = 4;

    // 8 + "..." + 4 = 15; at 15 or fewer characters abbreviating buys nothing.
    if uuid_str.len() <= HEAD + 3 + TAIL {
        return uuid_str;
    }

    match (uuid_str.get(..HEAD), uuid_str.get(uuid_str.len() - TAIL..)) {
        (Some(head), Some(tail)) => format!("{head}...{tail}"),
        // Unreachable for a UUID, whose rendering is ASCII; reached only if a
        // future rendering put a multi-byte boundary inside the cut.
        _ => uuid_str,
    }
}

pub fn print_success(message: &str) {
    println!("{}", style(message).green());
}

pub fn print_error(message: &str) {
    eprintln!("{}", style(message).red());
}

pub fn print_warning(message: &str) {
    println!("{}", style(message).yellow());
}

pub fn print_info(message: &str) {
    println!("{}", style(message).cyan());
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_uuid_abbreviates_head_and_tail() {
        let u = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(format_uuid(&u), "550e8400...0000");
    }

    /// The rendering is fixed-width, which is the point of using it in a table.
    #[test]
    fn format_uuid_is_always_fifteen_characters_for_a_real_uuid() {
        for _ in 0..64 {
            assert_eq!(format_uuid(&uuid::Uuid::new_v4()).len(), 15);
        }
    }

    #[test]
    fn format_uuid_keeps_the_nil_uuid_recognisable() {
        assert_eq!(format_uuid(&uuid::Uuid::nil()), "00000000...0000");
    }

    #[test]
    fn format_bool_is_yes_or_no() {
        // `console` strips styling when stdout is not a terminal, which is what
        // it is under a test harness -- so assert on the text, not the bytes.
        assert!(format_bool(true).contains("Yes"));
        assert!(format_bool(false).contains("No"));
    }
}
