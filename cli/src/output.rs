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

fn format_uuid(uuid: &uuid::Uuid) -> String {
    let uuid_str = uuid.to_string();
    format!("{}...{}", &uuid_str[..8], &uuid_str[uuid_str.len()-4..])
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