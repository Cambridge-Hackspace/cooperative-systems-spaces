//! Cross-origin access for the API, driven by `[server]` config.
//!
//! `ServerConfig` has carried `cors_enabled` and `cors_origins` since the
//! config struct was written, and nothing read either one: there was no
//! `CorsLayer` anywhere and no `Access-Control-*` header was ever emitted. The
//! fields were not a disabled feature, they were a described one that did not
//! exist -- an operator who set `cors_origins` got no effect and no warning,
//! and `cors_enabled = true` by default announced a capability the server did
//! not have. This module is that capability; `main.rs` applies it to the `/api`
//! nest. Issue #21.
//!
//! Two rules decide the shape, and both are refusals rather than conveniences:
//!
//!   * `allow_credentials` is deliberately NOT set. Cross-origin API access is by
//!     bearer token in the `Authorization` header -- device and CLI clients, and
//!     the cmi5 content's fetch token (#134). The browser session lives in an
//!     httpOnly, `SameSite=Strict` cookie (#135), which is same-origin only and
//!     never rides a cross-origin request. Withholding `allow_credentials` is what
//!     lets a CORS-admitted origin -- notably a separate cmi5 content origin --
//!     call the LRS with its fetch token while remaining unable to use the session
//!     cookie. It also keeps a wildcard origin permissible in principle -- but see
//!     the second rule, which forbids one anyway.
//!
//!   * Enabled with no origin is refused, not widened to `*`. `*` on an API
//!     whose every authenticated route answers to a bearer token would let any
//!     website on the internet read those routes with a token it phished; the
//!     safe failure is to not start. The refusal lives in `allowed_origins` and
//!     is surfaced at boot and at reload through `validate` (see
//!     `config::validate_config`).

use anyhow::{anyhow, Result};
use axum::http::{header, HeaderValue, Method};
use tower_http::cors::CorsLayer;

use crate::config::ServerConfig;

/// The origins CORS will admit, parsed and validated, or an error describing
/// why the configuration cannot be served.
///
/// Blank entries are dropped first, so a list that is `[""]` or `["  "]` is
/// treated as empty rather than as one nameless origin -- the same "switched on
/// and cannot possibly work" case as an empty list, and refused identically.
fn allowed_origins(
    cfg: &ServerConfig,
    cmi5_content_origin: Option<&str>,
) -> Result<Vec<HeaderValue>> {
    let mut origins: Vec<&str> = cfg
        .cors_origins
        .iter()
        .map(|o| o.trim())
        .filter(|o| !o.is_empty())
        .collect();

    // #120 (#134): the cmi5 content origin, when configured, is admitted so the
    // package's LRS calls -- cross-origin, authorized by the fetch token, never
    // credentialed -- are allowed. CORS sets no `allow-credentials`, so admitting
    // it does not let that origin ride the session cookie. Deduped so listing it
    // in both `cors_origins` and `cmi5.content_origin` is not an error.
    if let Some(extra) = cmi5_content_origin.map(str::trim).filter(|e| !e.is_empty()) {
        if !origins.contains(&extra) {
            origins.push(extra);
        }
    }

    if origins.is_empty() {
        return Err(anyhow!(
            "server.cors_enabled is true but server.cors_origins lists no usable \
             origin. CORS with no allowed origin admits no browser at all, and \
             defaulting to '*' on an API whose credential is a bearer token would \
             expose every authenticated route to every website. List the exact \
             origins that may call the API (e.g. \"https://learn.example.org\"), \
             or set server.cors_enabled = false."
        ));
    }

    origins
        .iter()
        .map(|o| {
            o.parse::<HeaderValue>().map_err(|_| {
                anyhow!(
                    "server.cors_origins entry {:?} is not a valid origin. An \
                     origin is scheme, host and optional port with no path, e.g. \
                     \"https://learn.example.org\" or \"http://localhost:3000\".",
                    o
                )
            })
        })
        .collect()
}

/// Validate the CORS configuration without building the layer.
///
/// `config::validate_config` calls this at load and at reload, so a
/// misconfiguration is refused with a message rather than surfacing later as a
/// panic while the router is assembled or, worse, as a silently absent header.
pub fn validate(cfg: &ServerConfig, cmi5_content_origin: Option<&str>) -> Result<()> {
    if cfg.cors_enabled {
        allowed_origins(cfg, cmi5_content_origin)?;
    }
    Ok(())
}

/// The CORS layer for the API, or `None` when CORS is disabled.
///
/// `None` is the honest form of the old default: no layer, no headers, exactly
/// the behaviour before this existed. `Some` allows the configured origins the
/// methods and request headers the API actually uses -- `Authorization` for the
/// bearer token and `Content-Type` for JSON bodies. Preflight is handled by the
/// layer itself.
pub fn build_layer(
    cfg: &ServerConfig,
    cmi5_content_origin: Option<&str>,
) -> Result<Option<CorsLayer>> {
    if !cfg.cors_enabled {
        return Ok(None);
    }

    let origins = allowed_origins(cfg, cmi5_content_origin)?;
    let layer = CorsLayer::new()
        .allow_origin(origins)
        // #120 (low): PATCH was omitted, so preflight for the PATCH routes
        // (e.g. /api/admin/webhooks/auth-headers/{id}) failed from a browser.
        .allow_methods(vec![
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE]);

    Ok(Some(layer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, origins: &[&str]) -> ServerConfig {
        let mut c = ServerConfig::default();
        c.cors_enabled = enabled;
        c.cors_origins = origins.iter().map(|s| s.to_string()).collect();
        c
    }

    #[test]
    fn disabled_builds_no_layer() {
        // The whole point of the fix in one assertion: with CORS off there is
        // no layer, so no Access-Control-* header is ever emitted. If a future
        // change makes `build_layer` return Some here, the server would start
        // answering cross-origin requests nobody enabled.
        assert!(build_layer(&cfg(false, &[]), None).unwrap().is_none());
        // Still none even if origins happen to be listed: `cors_enabled` is the
        // switch, and a stray origin list must not turn CORS on by itself.
        assert!(build_layer(&cfg(false, &["https://example.org"]), None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn enabled_with_origins_builds_a_layer() {
        assert!(build_layer(&cfg(true, &["https://example.org"]), None)
            .unwrap()
            .is_some());
        assert!(validate(&cfg(true, &["https://example.org"]), None).is_ok());
    }

    #[test]
    fn enabled_with_no_origin_is_refused() {
        // The reported hazard: enabled but with nothing to allow must fail
        // loudly rather than defaulting to a wildcard. Asserted through both
        // entry points, because `validate` (boot/reload) and `build_layer`
        // (router assembly) are reached on different paths.
        assert!(build_layer(&cfg(true, &[]), None).is_err());
        assert!(validate(&cfg(true, &[]), None).is_err());
    }

    #[test]
    fn enabled_with_only_blank_origins_is_refused() {
        // `[""]` is not one nameless origin; it is no origin, and must be
        // refused exactly as the empty list is -- otherwise a blank entry left
        // in a config file would read as "allow an origin named empty string".
        assert!(build_layer(&cfg(true, &["", "   "]), None).is_err());
        assert!(validate(&cfg(true, &["  "]), None).is_err());
    }

    #[test]
    fn a_bad_origin_is_refused() {
        // A control character cannot sit in a header value; catching it here
        // turns a router-assembly panic into a boot-time message that names the
        // offending entry.
        assert!(build_layer(&cfg(true, &["http://ok.example", "bad\norigin"]), None).is_err());
    }

    #[test]
    fn the_cmi5_content_origin_is_admitted() {
        // #120 (#134): a configured cmi5 content origin is added to the allowed
        // set so the package's cross-origin LRS calls are permitted. Asserted at
        // the origin level so it fails if a future change drops the extra origin.
        let origins = allowed_origins(
            &cfg(true, &["https://app.example"]),
            Some("https://cmi5.example"),
        )
        .unwrap();
        let want: Vec<_> = ["https://app.example", "https://cmi5.example"]
            .iter()
            .map(|o| o.parse::<HeaderValue>().unwrap())
            .collect();
        assert_eq!(origins, want);
    }

    #[test]
    fn the_cmi5_content_origin_alone_is_enough() {
        // The SPA is same-origin, so an operator may run with no `cors_origins`
        // and only a cmi5 content origin. That is one usable origin, not none, so
        // the empty-list refusal must not fire.
        assert!(build_layer(&cfg(true, &[]), Some("https://cmi5.example"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn the_cmi5_content_origin_is_deduped() {
        // Listing it in both places is not two origins and not an error.
        let origins = allowed_origins(
            &cfg(true, &["https://cmi5.example"]),
            Some("https://cmi5.example"),
        )
        .unwrap();
        assert_eq!(origins.len(), 1);
    }
}
