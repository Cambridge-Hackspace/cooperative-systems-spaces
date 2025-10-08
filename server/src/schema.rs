// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(
        diesel::deserialize::Queryable,
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::UserRole;

    users (id) {
        id -> Uuid,
        username -> Varchar,
        email -> Varchar,
        password_hash -> Varchar,
        full_name -> Varchar,
        is_active -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        role -> UserRole,
    }
}
