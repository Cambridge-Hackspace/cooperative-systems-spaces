//! The database half of CS:S as an OpenID Connect provider (#225).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::database::{DatabaseError, DatabaseManager};
use crate::models::{
    NewOauthAuthorizationRequest, NewOauthClient, NewOauthRefreshToken, NewOauthSigningKey,
    OauthAuthorizationCode, OauthAuthorizationRequest, OauthClient, OauthConsent,
    OauthRefreshToken, OauthSigningKey,
};

/// What an administrator may change about an app. `None` leaves a field alone.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::oauth_clients)]
pub struct OauthClientChanges {
    pub name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
    pub post_logout_redirect_uris: Option<Vec<String>>,
    pub allowed_scopes: Option<Vec<String>>,
    pub skip_consent: Option<bool>,
    pub disabled: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// What redeeming an authorization code found.
pub enum Redeemed {
    /// First use: the code, now marked used.
    Fresh(OauthAuthorizationCode),
    /// The code was already used. Its tokens have been revoked.
    Replayed,
    /// No such code, or it belongs to another app.
    Unknown,
}

impl DatabaseManager {
    // ── apps ────────────────────────────────────────────────────────────────

    pub fn oauth_insert_client(&self, new: &NewOauthClient) -> Result<OauthClient, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        diesel::insert_into(oauth_clients::table)
            .values(new)
            .returning(OauthClient::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_list_clients(&self) -> Result<Vec<OauthClient>, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        oauth_clients::table
            .order(oauth_clients::created_at.asc())
            .select(OauthClient::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_get_client(&self, id: Uuid) -> Result<Option<OauthClient>, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        oauth_clients::table
            .find(id)
            .select(OauthClient::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_client_by_client_id(
        &self,
        client_id: &str,
    ) -> Result<Option<OauthClient>, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        oauth_clients::table
            .filter(oauth_clients::client_id.eq(client_id))
            .select(OauthClient::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_update_client(
        &self,
        id: Uuid,
        mut changes: OauthClientChanges,
    ) -> Result<Option<OauthClient>, DatabaseError> {
        use crate::schema::oauth_clients;
        changes.updated_at = Some(Utc::now());
        let mut conn = self.get_connection()?;
        diesel::update(oauth_clients::table.find(id))
            .set(&changes)
            .returning(OauthClient::as_returning())
            .get_result(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_set_client_secret(
        &self,
        id: Uuid,
        secret_hash: &str,
    ) -> Result<Option<OauthClient>, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        diesel::update(
            oauth_clients::table
                .find(id)
                .filter(oauth_clients::confidential.eq(true)),
        )
        .set((
            oauth_clients::secret_hash.eq(Some(secret_hash)),
            oauth_clients::updated_at.eq(Utc::now()),
        ))
        .returning(OauthClient::as_returning())
        .get_result(&mut conn)
        .optional()
        .map_err(DatabaseError::Diesel)
    }

    /// Remove an app. Its codes, tokens, consents and pending requests go with
    /// it (ON DELETE CASCADE), so nothing it was issued keeps working.
    pub fn oauth_delete_client(&self, id: Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::oauth_clients;
        let mut conn = self.get_connection()?;
        let n = diesel::delete(oauth_clients::table.find(id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(n > 0)
    }

    // ── signing keys ────────────────────────────────────────────────────────

    /// Every signing key, newest first.
    pub fn oauth_signing_keys(&self) -> Result<Vec<OauthSigningKey>, DatabaseError> {
        use crate::schema::oauth_signing_keys;
        let mut conn = self.get_connection()?;
        oauth_signing_keys::table
            .order(oauth_signing_keys::created_at.desc())
            .select(OauthSigningKey::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Add a new signing key and retire every other unretired one, in one
    /// transaction, so there is never a moment with two signing keys or none.
    pub fn oauth_rotate_signing_key(&self, new: &NewOauthSigningKey) -> Result<(), DatabaseError> {
        use crate::schema::oauth_signing_keys;
        let mut conn = self.get_connection()?;
        conn.transaction(|conn| {
            diesel::update(
                oauth_signing_keys::table.filter(oauth_signing_keys::retired_at.is_null()),
            )
            .set(oauth_signing_keys::retired_at.eq(Some(Utc::now())))
            .execute(conn)?;
            diesel::insert_into(oauth_signing_keys::table)
                .values(new)
                .execute(conn)?;
            Ok::<_, diesel::result::Error>(())
        })
        .map_err(DatabaseError::Diesel)
    }

    // ── requests waiting for the member ─────────────────────────────────────

    pub fn oauth_insert_request(
        &self,
        new: &NewOauthAuthorizationRequest,
    ) -> Result<OauthAuthorizationRequest, DatabaseError> {
        use crate::schema::oauth_authorization_requests;
        let mut conn = self.get_connection()?;
        diesel::insert_into(oauth_authorization_requests::table)
            .values(new)
            .returning(OauthAuthorizationRequest::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// A request that hasn't expired.
    pub fn oauth_live_request(
        &self,
        id: Uuid,
    ) -> Result<Option<OauthAuthorizationRequest>, DatabaseError> {
        use crate::schema::oauth_authorization_requests as r;
        let mut conn = self.get_connection()?;
        r::table
            .find(id)
            .filter(r::expires_at.gt(Utc::now()))
            .select(OauthAuthorizationRequest::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Finish with a request: it is answered once.
    pub fn oauth_delete_request(&self, id: Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::oauth_authorization_requests as r;
        let mut conn = self.get_connection()?;
        let n = diesel::delete(r::table.find(id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(n > 0)
    }

    /// Drop expired requests and codes. Called on a timer.
    pub fn oauth_sweep_expired(&self) -> Result<usize, DatabaseError> {
        use crate::schema::{oauth_authorization_codes as c, oauth_authorization_requests as r};
        let mut conn = self.get_connection()?;
        let now = Utc::now();
        let a = diesel::delete(r::table.filter(r::expires_at.le(now)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        // A used code is kept a day past its expiry so a late replay is still
        // recognised as a replay, and its tokens revoked.
        let b = diesel::delete(c::table.filter(c::expires_at.le(now - chrono::Duration::days(1))))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(a + b)
    }

    // ── authorization codes ─────────────────────────────────────────────────

    pub fn oauth_insert_code(&self, code: &OauthAuthorizationCode) -> Result<(), DatabaseError> {
        use crate::schema::oauth_authorization_codes;
        let mut conn = self.get_connection()?;
        diesel::insert_into(oauth_authorization_codes::table)
            .values(code)
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// Use a code once, for one app. The update is the lock: two redemptions
    /// racing see exactly one `Fresh`. A second use revokes every refresh token
    /// the first one led to (RFC 6749 section 4.1.2).
    pub fn oauth_redeem_code(
        &self,
        code_hash: &str,
        client: Uuid,
    ) -> Result<Redeemed, DatabaseError> {
        use crate::schema::{oauth_authorization_codes as c, oauth_refresh_tokens as t};
        let mut conn = self.get_connection()?;
        let now = Utc::now();
        let fresh = diesel::update(
            c::table
                .find(code_hash)
                .filter(c::client_id.eq(client))
                .filter(c::used_at.is_null()),
        )
        .set(c::used_at.eq(Some(now)))
        .returning(OauthAuthorizationCode::as_returning())
        .get_result(&mut conn)
        .optional()
        .map_err(DatabaseError::Diesel)?;
        if let Some(code) = fresh {
            return Ok(Redeemed::Fresh(code));
        }
        let replayed = c::table
            .find(code_hash)
            .filter(c::client_id.eq(client))
            .select(c::code_hash)
            .first::<String>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?
            .is_some();
        if replayed {
            diesel::update(
                t::table
                    .filter(t::code_hash.eq(code_hash))
                    .filter(t::revoked_at.is_null()),
            )
            .set(t::revoked_at.eq(Some(now)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
            return Ok(Redeemed::Replayed);
        }
        Ok(Redeemed::Unknown)
    }

    // ── refresh tokens ──────────────────────────────────────────────────────

    pub fn oauth_insert_refresh_token(
        &self,
        new: &NewOauthRefreshToken,
    ) -> Result<(), DatabaseError> {
        use crate::schema::oauth_refresh_tokens;
        let mut conn = self.get_connection()?;
        diesel::insert_into(oauth_refresh_tokens::table)
            .values(new)
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// Use a refresh token: it is revoked as it is used (rotation), and the
    /// caller issues the next one. A token that was already revoked means it
    /// was stolen or replayed, so every token for that member and app is
    /// revoked and `None` is returned.
    pub fn oauth_use_refresh_token(
        &self,
        token_hash: &str,
        client: Uuid,
    ) -> Result<Option<OauthRefreshToken>, DatabaseError> {
        use crate::schema::oauth_refresh_tokens as t;
        let mut conn = self.get_connection()?;
        let now = Utc::now();
        let used = diesel::update(
            t::table
                .filter(t::token_hash.eq(token_hash))
                .filter(t::client_id.eq(client))
                .filter(t::revoked_at.is_null())
                .filter(t::expires_at.gt(now)),
        )
        .set(t::revoked_at.eq(Some(now)))
        .returning(OauthRefreshToken::as_returning())
        .get_result(&mut conn)
        .optional()
        .map_err(DatabaseError::Diesel)?;
        if used.is_some() {
            return Ok(used);
        }
        let reused = t::table
            .filter(t::token_hash.eq(token_hash))
            .filter(t::client_id.eq(client))
            .filter(t::revoked_at.is_not_null())
            .select(OauthRefreshToken::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        if let Some(r) = reused {
            diesel::update(
                t::table
                    .filter(t::user_id.eq(r.user_id))
                    .filter(t::client_id.eq(r.client_id))
                    .filter(t::revoked_at.is_null()),
            )
            .set(t::revoked_at.eq(Some(now)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        }
        Ok(None)
    }

    /// Every tool, by name, for `GET /api/oauth/me/tools`.
    pub fn oauth_all_tools(&self) -> Result<Vec<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools;
        let mut conn = self.get_connection()?;
        tools::table
            .order(tools::name.asc())
            .select(crate::models::Tool::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ── consent ─────────────────────────────────────────────────────────────

    pub fn oauth_consent(
        &self,
        user: Uuid,
        client: Uuid,
    ) -> Result<Option<OauthConsent>, DatabaseError> {
        use crate::schema::oauth_consents as k;
        let mut conn = self.get_connection()?;
        k::table
            .find((user, client))
            .select(OauthConsent::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    pub fn oauth_grant_consent(
        &self,
        user: Uuid,
        client: Uuid,
        scopes: &[String],
    ) -> Result<(), DatabaseError> {
        use crate::schema::oauth_consents as k;
        let mut conn = self.get_connection()?;
        let row = OauthConsent {
            user_id: user,
            client_id: client,
            scopes: scopes.to_vec(),
            granted_at: Utc::now(),
        };
        diesel::insert_into(k::table)
            .values(&row)
            .on_conflict((k::user_id, k::client_id))
            .do_update()
            .set((k::scopes.eq(scopes.to_vec()), k::granted_at.eq(Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }
}
