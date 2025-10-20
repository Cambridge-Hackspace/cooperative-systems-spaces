// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "assessment_type"))]
    pub struct AssessmentType;

    #[derive(
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "tool_category"))]
    pub struct ToolCategory;

    #[derive(
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "tool_status"))]
    pub struct ToolStatus;

    #[derive(
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "training_status"))]
    pub struct TrainingStatus;

    #[derive(
        serde::Serialize,
        serde::Deserialize,
        diesel::sql_types::SqlType,
    )]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;
}

diesel::table! {
    audit_logs (id) {
        id -> Uuid,
        event_type -> Text,
        user_id -> Nullable<Uuid>,
        actor_id -> Nullable<Uuid>,
        event_data -> Jsonb,
        ip_address -> Nullable<Text>,
        user_agent -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ToolStatus;

    tool_events (id) {
        id -> Uuid,
        tool_id -> Uuid,
        event_type -> Varchar,
        old_status -> Nullable<ToolStatus>,
        new_status -> Nullable<ToolStatus>,
        user_id -> Nullable<Uuid>,
        actor_id -> Nullable<Uuid>,
        notes -> Nullable<Text>,
        scan_data -> Nullable<Jsonb>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    tool_trainers (id) {
        id -> Uuid,
        user_id -> Uuid,
        tool_id -> Uuid,
        authorized_by -> Uuid,
        authorized_at -> Timestamptz,
        notes -> Nullable<Text>,
        expires_at -> Nullable<Timestamptz>,
        is_active -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    tool_training_types (id) {
        id -> Uuid,
        tool_id -> Uuid,
        training_name -> Varchar,
        description -> Nullable<Text>,
        expires_after_days -> Nullable<Int4>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ToolCategory;
    use super::sql_types::ToolStatus;

    tools (id) {
        id -> Uuid,
        name -> Varchar,
        description -> Nullable<Text>,
        category -> ToolCategory,
        status -> ToolStatus,
        barcode -> Nullable<Varchar>,
        serial_number -> Nullable<Varchar>,
        location -> Nullable<Varchar>,
        purchase_date -> Nullable<Date>,
        purchase_price -> Nullable<Numeric>,
        maintenance_notes -> Nullable<Text>,
        requires_training -> Bool,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        external_id -> Nullable<Varchar>,
    }
}

diesel::table! {
    training_instructors (id) {
        id -> Uuid,
        user_id -> Uuid,
        training_step_id -> Uuid,
        certified_by -> Uuid,
        certified_at -> Timestamptz,
        expires_at -> Nullable<Timestamptz>,
        notes -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    training_prerequisites (id) {
        id -> Uuid,
        training_step_id -> Uuid,
        prerequisite_step_id -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    training_records (id) {
        id -> Uuid,
        tool_id -> Uuid,
        trainee_user_id -> Uuid,
        trainer_user_id -> Uuid,
        training_date -> Date,
        completion_status -> Varchar,
        minutes_trained -> Nullable<Int4>,
        skills_covered -> Nullable<Array<Nullable<Text>>>,
        notes -> Nullable<Text>,
        next_steps -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        training_step_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::AssessmentType;

    training_steps (id) {
        id -> Uuid,
        tool_id -> Uuid,
        step_number -> Int4,
        step_name -> Varchar,
        description -> Nullable<Text>,
        training_materials_url -> Nullable<Text>,
        requires_assessment -> Bool,
        assessment_type -> Nullable<AssessmentType>,
        duration_minutes -> Nullable<Int4>,
        expires_after_days -> Nullable<Int4>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    user_tool_training (id) {
        id -> Uuid,
        user_id -> Uuid,
        training_type_id -> Uuid,
        trainer_id -> Uuid,
        trained_at -> Timestamptz,
        expires_at -> Nullable<Timestamptz>,
        notes -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::TrainingStatus;

    user_training_progress (id) {
        id -> Uuid,
        user_id -> Uuid,
        training_step_id -> Uuid,
        status -> TrainingStatus,
        instructor_id -> Nullable<Uuid>,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        expires_at -> Nullable<Timestamptz>,
        assessment_score -> Nullable<Int4>,
        notes -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
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
        profile -> Jsonb,
    }
}

diesel::joinable!(tool_events -> tools (tool_id));
diesel::joinable!(tool_trainers -> tools (tool_id));
diesel::joinable!(tool_training_types -> tools (tool_id));
diesel::joinable!(tool_training_types -> users (created_by));
diesel::joinable!(tools -> users (created_by));
diesel::joinable!(training_instructors -> training_steps (training_step_id));
diesel::joinable!(training_records -> tools (tool_id));
diesel::joinable!(training_records -> training_steps (training_step_id));
diesel::joinable!(training_steps -> tools (tool_id));
diesel::joinable!(training_steps -> users (created_by));
diesel::joinable!(user_tool_training -> tool_training_types (training_type_id));
diesel::joinable!(user_training_progress -> training_steps (training_step_id));

diesel::allow_tables_to_appear_in_same_query!(
    audit_logs,
    tool_events,
    tool_trainers,
    tool_training_types,
    tools,
    training_instructors,
    training_prerequisites,
    training_records,
    training_steps,
    user_tool_training,
    user_training_progress,
    users,
);
