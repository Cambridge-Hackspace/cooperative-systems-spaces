//! A user's Stripe customer ids (#118).
//!
//! One row per customer id Stripe knows the user by. Inbound webhooks map ANY
//! of them to the account; checkout and the Billing Portal use the one with a
//! live subscription (else the most recently updated); "has a subscription" is
//! "any row has one". Never card data: ids and a status string only.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::user_stripe_customers;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = user_stripe_customers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserStripeCustomer {
    pub id: Uuid,
    pub user_id: Uuid,
    pub customer_id: String,
    /// The subscription currently attached to this customer, if any.
    pub subscription_id: Option<String>,
    /// Last-seen Stripe status, display only. The ledger is the gate.
    pub subscription_status: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_stripe_customers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserStripeCustomer {
    pub user_id: Uuid,
    pub customer_id: String,
    pub subscription_id: Option<String>,
    pub subscription_status: Option<String>,
}
