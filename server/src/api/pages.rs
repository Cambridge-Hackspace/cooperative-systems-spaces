use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::pages::{NavItem, Page};
use crate::AppState;

/// Serializable version of Page for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct PageResponse {
    pub title: String,
    pub html_content: String,
    pub slug: String,
    pub relative_path: String,
    pub repo_url: Option<String>,
    pub default_branch: Option<String>,
}

impl PageResponse {
    fn from_page(page: Page, repo_url: Option<String>, default_branch: Option<String>) -> Self {
        Self {
            title: page.title,
            html_content: page.html_content,
            slug: page.slug,
            relative_path: page.relative_path,
            repo_url,
            default_branch,
        }
    }
}

/// Navigation response
#[derive(Debug, Serialize, Deserialize)]
pub struct NavigationResponse {
    pub wiki_nav: Vec<NavItem>,
    pub site_nav: Vec<NavItem>,
}

/// Pages list response
#[derive(Debug, Serialize, Deserialize)]
pub struct PagesListResponse {
    pub pages: Vec<PageSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PageSummary {
    pub title: String,
    pub slug: String,
    pub relative_path: String,
}

/// Routes for pages functionality
pub fn pages_routes() -> Router<AppState> {
    Router::new()
        .route("/wiki", get(list_wiki_pages))
        .route("/wiki/*slug", get(get_wiki_page))
        .route("/site", get(list_site_pages))
        .route("/site/index", get(get_site_index))
        .route("/site/*slug", get(get_site_page))
        .route("/navigation", get(get_navigation))
}

/// List all wiki pages
async fn list_wiki_pages(
    State(state): State<AppState>,
) -> Result<Json<PagesListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;
    let store = pages_service.get_store();

    let pages: Vec<PageSummary> = store
        .wiki_pages
        .values()
        .map(|page| PageSummary {
            title: page.title.clone(),
            slug: page.slug.clone(),
            relative_path: page.relative_path.clone(),
        })
        .collect();

    Ok(Json(PagesListResponse { pages }))
}

/// Get a specific wiki page by slug
async fn get_wiki_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<PageResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;
    
    // Strip leading slash if present (from catch-all route)
    let slug = slug.trim_start_matches('/');

    match pages_service.get_wiki_page(slug) {
        Some(page) => {
            let repo_url = pages_service.get_wiki_repo_url();
            let default_branch = pages_service.get_wiki_default_branch();
            Ok(Json(PageResponse::from_page(page.clone(), repo_url, default_branch)))
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Wiki page not found",
                "slug": slug
            })),
        )),
    }
}

/// List all site pages
async fn list_site_pages(
    State(state): State<AppState>,
) -> Result<Json<PagesListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;
    let store = pages_service.get_store();

    let pages: Vec<PageSummary> = store
        .site_pages
        .values()
        .map(|page| PageSummary {
            title: page.title.clone(),
            slug: page.slug.clone(),
            relative_path: page.relative_path.clone(),
        })
        .collect();

    Ok(Json(PagesListResponse { pages }))
}

/// Get the site index page
async fn get_site_index(
    State(state): State<AppState>,
) -> Result<Json<PageResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;

    match pages_service.get_site_index() {
        Some(page) => {
            let repo_url = pages_service.get_site_repo_url();
            let default_branch = pages_service.get_site_default_branch();
            Ok(Json(PageResponse::from_page(page.clone(), repo_url, default_branch)))
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Site index page not found"
            })),
        )),
    }
}

/// Get a specific site page by slug
async fn get_site_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<PageResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;
    
    // Strip leading slash if present (from catch-all route)
    let slug = slug.trim_start_matches('/');

    match pages_service.get_site_page(slug) {
        Some(page) => {
            let repo_url = pages_service.get_site_repo_url();
            let default_branch = pages_service.get_site_default_branch();
            Ok(Json(PageResponse::from_page(page.clone(), repo_url, default_branch)))
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Site page not found",
                "slug": slug
            })),
        )),
    }
}

/// Get navigation structure for both wiki and site
async fn get_navigation(
    State(state): State<AppState>,
) -> Result<Json<NavigationResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pages_service = state.pages_service.read().await;
    let store = pages_service.get_store();

    Ok(Json(NavigationResponse {
        wiki_nav: store.wiki_nav,
        site_nav: store.site_nav,
    }))
}
