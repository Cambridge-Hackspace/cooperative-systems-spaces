// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "assessment_type"))]
    pub struct AssessmentType;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "space_device_kind"))]
    pub struct SpaceDeviceKind;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "space_device_platform"))]
    pub struct SpaceDevicePlatform;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "tool_category"))]
    pub struct ToolCategory;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "tool_status"))]
    pub struct ToolStatus;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "training_status"))]
    pub struct TrainingStatus;

    #[derive(serde::Serialize, serde::Deserialize, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;
}

diesel::table! {
    audit_event_types (name) {
        name -> Text,
    }
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
    space_device_auth (id) {
        id -> Uuid,
        device_id -> Uuid,
        #[max_length = 255]
        auth_token -> Varchar,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    space_device_auth_requests (id) {
        id -> Uuid,
        #[max_length = 32]
        device_code -> Varchar,
        expires_at -> Timestamptz,
        used_at -> Nullable<Timestamptz>,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::SpaceDeviceKind;
    use super::sql_types::SpaceDevicePlatform;

    space_devices (id) {
        id -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        deleted_at -> Nullable<Timestamptz>,
        last_seen_at -> Nullable<Timestamptz>,
        kind -> SpaceDeviceKind,
        #[max_length = 17]
        mac_address -> Varchar,
        #[max_length = 50]
        software_version -> Varchar,
        #[max_length = 15]
        ipv4_address -> Nullable<Varchar>,
        #[max_length = 45]
        ipv6_address -> Nullable<Varchar>,
        uptime -> Int8,
        platform -> SpaceDevicePlatform,
        place_id -> Nullable<Uuid>,
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
        external_api_key -> Nullable<Text>,
        place_id -> Nullable<Uuid>,
        schedule_id -> Nullable<Uuid>,
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
        self_attestable -> Bool,
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
        acknowledged_materials_url -> Nullable<Text>,
    }
}

diesel::table! {
    webhook_auth_headers (id) {
        id -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        #[max_length = 255]
        header_name -> Varchar,
        header_value -> Text,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    webhooks (id) {
        id -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        url -> Text,
        enabled -> Bool,
        signing_secret -> Text,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    webhook_event_subscriptions (id) {
        id -> Uuid,
        webhook_id -> Uuid,
        event_type -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    webhook_auth_links (id) {
        id -> Uuid,
        webhook_id -> Uuid,
        auth_header_id -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    webhook_deliveries (id) {
        id -> Uuid,
        webhook_id -> Uuid,
        audit_log_id -> Nullable<Uuid>,
        event_type -> Text,
        attempt -> Int4,
        success -> Bool,
        status_code -> Nullable<Int4>,
        response_body -> Nullable<Text>,
        error -> Nullable<Text>,
        request_payload -> Nullable<Jsonb>,
        created_at -> Timestamptz,
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
        meta -> Jsonb,
        mfa_enrolled_at -> Nullable<Timestamptz>,
        email_verified_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    places (id) {
        id -> Uuid,
        parent_id -> Nullable<Uuid>,
        place_type -> Text,
        #[max_length = 120]
        name -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 255]
        external_id -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        is_special -> Bool,
    }
}

diesel::table! {
    doors (id) {
        id -> Uuid,
        #[max_length = 120]
        name -> Varchar,
        location -> Nullable<Text>,
        description -> Nullable<Text>,
        edge_device_id -> Nullable<Uuid>,
        unlock_duration_ms -> Int4,
        enabled -> Bool,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        place_id_from -> Nullable<Uuid>,
        place_id_to -> Nullable<Uuid>,
    }
}

diesel::table! {
    door_access_rules (id) {
        id -> Uuid,
        door_id -> Uuid,
        kind -> Text,
        value -> Text,
        effect -> Text,
        created_at -> Timestamptz,
        schedule_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    schedules (id) {
        id -> Uuid,
        #[max_length = 120]
        name -> Varchar,
        description -> Nullable<Text>,
        intervals -> Jsonb,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        is_public -> Bool,
    }
}

diesel::table! {
    home_links (id) {
        id -> Uuid,
        #[max_length = 120]
        label -> Varchar,
        url -> Text,
        description -> Nullable<Text>,
        #[max_length = 120]
        icon -> Nullable<Varchar>,
        audience -> Text,
        sort_order -> Int4,
        enabled -> Bool,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        expires_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    door_access_events (id) {
        id -> Uuid,
        door_id -> Uuid,
        user_id -> Nullable<Uuid>,
        method -> Text,
        card_id_attempted -> Nullable<Text>,
        granted -> Bool,
        reason -> Nullable<Text>,
        ip_address -> Nullable<Text>,
        occurred_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    door_checkins (id) {
        id -> Uuid,
        door_id -> Uuid,
        user_id -> Uuid,
        door_access_event_id -> Nullable<Uuid>,
        ip_address -> Nullable<Text>,
        user_agent -> Nullable<Text>,
        occurred_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    user_mfa_totp (id) {
        id -> Uuid,
        user_id -> Uuid,
        secret_base32 -> Text,
        confirmed_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    user_mfa_webauthn (id) {
        id -> Uuid,
        user_id -> Uuid,
        credential_id -> Bytea,
        passkey -> Jsonb,
        #[max_length = 120]
        label -> Varchar,
        created_at -> Timestamptz,
        last_used_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    user_mfa_recovery_codes (id) {
        id -> Uuid,
        user_id -> Uuid,
        code_hash -> Text,
        used_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    profile_config_versions (id) {
        id -> Uuid,
        version -> BigInt,
        profile_fields -> Jsonb,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        profiles_enabled -> Nullable<Bool>,
    }
}

diesel::table! {
    password_reset_tokens (id) {
        id -> Uuid,
        user_id -> Uuid,
        token_hash -> Text,
        expires_at -> Timestamptz,
        used_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    email_verification_tokens (id) {
        id -> Uuid,
        user_id -> Uuid,
        token_hash -> Text,
        expires_at -> Timestamptz,
        used_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    cmi5_courses (id) {
        id -> Uuid,
        course_iri -> Text,
        title -> Nullable<Text>,
        description -> Nullable<Text>,
        content_path -> Text,
        manifest_xml -> Text,
        imported_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        deleted_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    cmi5_blocks (id) {
        id -> Uuid,
        course_id -> Uuid,
        parent_block_id -> Nullable<Uuid>,
        block_iri -> Nullable<Text>,
        title -> Nullable<Text>,
        position -> Int4,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    cmi5_assignable_units (id) {
        id -> Uuid,
        course_id -> Uuid,
        block_id -> Nullable<Uuid>,
        au_iri -> Text,
        title -> Nullable<Text>,
        launch_url -> Text,
        launch_parameters -> Nullable<Text>,
        launch_method -> Nullable<Text>,
        move_on -> Text,
        mastery_score -> Nullable<Float8>,
        position -> Int4,
        training_step_id -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    cmi5_registrations (id) {
        id -> Uuid,
        user_id -> Uuid,
        au_id -> Uuid,
        actor_account_name -> Text,
        launch_mode -> Text,
        satisfied_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        passed_at -> Nullable<Timestamptz>,
        failed_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    cmi5_launch_tokens (id) {
        id -> Uuid,
        registration_id -> Uuid,
        fetch_token_hash -> Text,
        session_token_hash -> Nullable<Text>,
        fetch_consumed_at -> Nullable<Timestamptz>,
        expires_at -> Timestamptz,
        session_expires_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    cmi5_statements (id) {
        id -> Uuid,
        registration_id -> Uuid,
        statement_id -> Uuid,
        verb_iri -> Text,
        stored -> Timestamptz,
        statement -> Jsonb,
        voided -> Bool,
    }
}

diesel::table! {
    cmi5_state_documents (id) {
        id -> Uuid,
        registration_id -> Uuid,
        activity_iri -> Text,
        agent_account_name -> Text,
        state_id -> Text,
        document -> Jsonb,
        etag -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::joinable!(space_device_auth -> space_devices (device_id));
diesel::joinable!(space_device_auth_requests -> users (created_by));
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
diesel::joinable!(webhook_auth_headers -> users (created_by));
diesel::joinable!(webhooks -> users (created_by));
diesel::joinable!(webhook_event_subscriptions -> webhooks (webhook_id));
diesel::joinable!(webhook_auth_links -> webhooks (webhook_id));
diesel::joinable!(webhook_auth_links -> webhook_auth_headers (auth_header_id));
diesel::joinable!(webhook_deliveries -> webhooks (webhook_id));
diesel::joinable!(webhook_deliveries -> audit_logs (audit_log_id));
diesel::joinable!(user_mfa_totp -> users (user_id));
diesel::joinable!(user_mfa_webauthn -> users (user_id));
diesel::joinable!(user_mfa_recovery_codes -> users (user_id));
diesel::joinable!(doors -> space_devices (edge_device_id));
diesel::joinable!(doors -> users (created_by));
// (places → places self-join not registered via `joinable!`; walked manually
// in the ancestor query helper.)
diesel::joinable!(tools -> places (place_id));
diesel::joinable!(space_devices -> places (place_id));
diesel::joinable!(door_access_rules -> schedules (schedule_id));
diesel::joinable!(schedules -> users (created_by));
diesel::joinable!(tools -> schedules (schedule_id));
diesel::joinable!(home_links -> users (created_by));
diesel::joinable!(door_access_rules -> doors (door_id));
diesel::joinable!(door_access_events -> doors (door_id));
diesel::joinable!(door_access_events -> users (user_id));
diesel::joinable!(door_checkins -> doors (door_id));
diesel::joinable!(door_checkins -> users (user_id));
diesel::joinable!(door_checkins -> door_access_events (door_access_event_id));
diesel::joinable!(profile_config_versions -> users (created_by));
diesel::joinable!(password_reset_tokens -> users (user_id));
diesel::joinable!(email_verification_tokens -> users (user_id));

diesel::joinable!(cmi5_courses -> users (imported_by));
diesel::joinable!(cmi5_blocks -> cmi5_courses (course_id));
diesel::joinable!(cmi5_assignable_units -> cmi5_courses (course_id));
diesel::joinable!(cmi5_assignable_units -> cmi5_blocks (block_id));
diesel::joinable!(cmi5_assignable_units -> training_steps (training_step_id));
diesel::joinable!(cmi5_registrations -> users (user_id));
diesel::joinable!(cmi5_registrations -> cmi5_assignable_units (au_id));
diesel::joinable!(cmi5_launch_tokens -> cmi5_registrations (registration_id));
diesel::joinable!(cmi5_statements -> cmi5_registrations (registration_id));
diesel::joinable!(cmi5_state_documents -> cmi5_registrations (registration_id));
// cmi5_blocks -> cmi5_blocks (parent_block_id) is a self-join; like the places
// self-reference, it is walked manually rather than registered via joinable!.

diesel::allow_tables_to_appear_in_same_query!(
    audit_event_types,
    audit_logs,
    space_device_auth,
    space_device_auth_requests,
    space_devices,
    tool_events,
    tool_trainers,
    tool_training_types,
    tools,
    training_instructors,
    training_prerequisites,
    training_records,
    training_steps,
    doors,
    door_access_events,
    door_access_rules,
    door_checkins,
    places,
    schedules,
    home_links,
    user_mfa_recovery_codes,
    user_mfa_totp,
    user_mfa_webauthn,
    user_tool_training,
    user_training_progress,
    users,
    webhook_auth_headers,
    webhooks,
    webhook_event_subscriptions,
    webhook_auth_links,
    webhook_deliveries,
    profile_config_versions,
    password_reset_tokens,
    email_verification_tokens,
    cmi5_courses,
    cmi5_blocks,
    cmi5_assignable_units,
    cmi5_registrations,
    cmi5_launch_tokens,
    cmi5_statements,
    cmi5_state_documents,
);
