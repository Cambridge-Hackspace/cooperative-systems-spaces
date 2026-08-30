use crate::config::PagesConfig;
use anyhow::{Context, Result};
use comrak::{markdown_to_html, Options};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

/// Represents a single markdown page in the wiki/site
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// The title extracted from the filename or frontmatter
    pub title: String,
    /// Rendered HTML content
    pub html_content: String,
    /// Raw markdown content
    pub raw_content: String,
    /// File path relative to the repo root
    pub relative_path: String,
    /// URL-friendly slug for the page
    pub slug: String,
    /// Last modified timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<std::time::SystemTime>,
}

/// Type of pages repository
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageType {
    Wiki,
    Site,
}

/// Storage for all built pages
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageStore {
    /// Wiki pages indexed by slug
    pub wiki_pages: HashMap<String, Page>,
    /// Site pages indexed by slug
    pub site_pages: HashMap<String, Page>,
    /// Navigation structure for wiki
    pub wiki_nav: Vec<NavItem>,
    /// Navigation structure for site
    pub site_nav: Vec<NavItem>,
    /// Site index page content (from site_embed_index config)
    pub site_index: Option<Page>,
}

/// Navigation item for building menus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    pub title: String,
    pub slug: String,
    pub path: String,
    pub children: Vec<NavItem>,
}

pub struct PagesService {
    config: PagesConfig,
    store: Arc<RwLock<PageStore>>,
    wiki_repo_path: Option<PathBuf>,
    site_repo_path: Option<PathBuf>,
    wiki_default_branch: Option<String>,
    site_default_branch: Option<String>,
}

impl PagesService {
    /// Create a new pages service and start it with initial build and auto-updating
    pub async fn new(config: PagesConfig) -> Result<Self> {
        let wiki_repo_path = config
            .wiki_repo()
            .as_ref()
            .map(|_| PathBuf::from("/tmp/css-wiki-repo"));
        let site_repo_path = config
            .site_repo()
            .as_ref()
            .map(|_| PathBuf::from("/tmp/css-site-repo"));

        let mut service = Self {
            config,
            store: Arc::new(RwLock::new(PageStore::default())),
            wiki_repo_path,
            site_repo_path,
            wiki_default_branch: None,
            site_default_branch: None,
        };

        // Perform initial build
        info!("Starting PagesService with initial build");
        if let Err(e) = service.build_all().await {
            error!("Failed initial build of pages: {}", e);
            // Don't fail on initial build - service can still work
        }

        // Spawn background tasks for auto-updating
        if service.config.wiki_auto_enabled() && service.config.wiki_repo().is_some() {
            let store = Arc::clone(&service.store);
            let config = service.config.clone();
            let wiki_repo_path = service.wiki_repo_path.clone();
            let period = service.config.wiki_period();

            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(period as u64)).await;
                    info!("Auto-updating wiki pages");
                    if let Err(e) = Self::update_wiki_static(&config, &wiki_repo_path, &store).await
                    {
                        error!("Failed to update wiki: {}", e);
                    }
                }
            });
        }

        if service.config.site_auto_enabled() && service.config.site_repo().is_some() {
            let store = Arc::clone(&service.store);
            let config = service.config.clone();
            let site_repo_path = service.site_repo_path.clone();
            let period = service.config.site_period();

            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(period as u64)).await;
                    info!("Auto-updating site pages");
                    if let Err(e) = Self::update_site_static(&config, &site_repo_path, &store).await
                    {
                        error!("Failed to update site: {}", e);
                    }
                }
            });
        }

        info!("PagesService started successfully");
        Ok(service)
    }

    /// Get a clone of the current page store
    pub fn get_store(&self) -> PageStore {
        self.store.read().unwrap().clone()
    }

    /// Get a specific wiki page by slug
    pub fn get_wiki_page(&self, slug: &str) -> Option<Page> {
        self.store.read().unwrap().wiki_pages.get(slug).cloned()
    }

    /// Get a specific site page by slug
    pub fn get_site_page(&self, slug: &str) -> Option<Page> {
        self.store.read().unwrap().site_pages.get(slug).cloned()
    }

    /// Get the site index page
    pub fn get_site_index(&self) -> Option<Page> {
        self.store.read().unwrap().site_index.clone()
    }

    /// Get the configured wiki repository URL (for edit links)
    pub fn get_wiki_repo_url(&self) -> Option<String> {
        self.config.wiki_repo.clone()
    }

    /// Get the configured site repository URL (for edit links)
    pub fn get_site_repo_url(&self) -> Option<String> {
        self.config.site_repo.clone()
    }

    /// Get the wiki repository default branch
    pub fn get_wiki_default_branch(&self) -> Option<String> {
        self.wiki_default_branch.clone()
    }

    /// Get the site repository default branch
    pub fn get_site_default_branch(&self) -> Option<String> {
        self.site_default_branch.clone()
    }

    /// Build all enabled page types
    async fn build_all(&mut self) -> Result<()> {
        if self.config.wiki_repo().is_some() {
            self.trigger_wiki_update().await?;
        }
        if self.config.site_repo().is_some() {
            self.trigger_site_update().await?;
        }
        Ok(())
    }

    /// Trigger wiki pages update from repository (public for API use)
    pub async fn trigger_wiki_update(&mut self) -> Result<()> {
        let repo_url = self
            .config
            .wiki_repo()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wiki repo not configured"))?;
        let repo_path = self
            .wiki_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wiki repo path not set"))?;

        Self::sync_repository_static(repo_url, repo_path).await?;

        // Get the default branch after sync
        self.wiki_default_branch = Self::get_default_branch_static(repo_path).ok();

        let pages = Self::build_pages_static(repo_path, PageType::Wiki, self.config.wiki_readme())?;
        let nav = Self::build_navigation_static(&pages);

        let mut store = self.store.write().unwrap();
        store.wiki_pages = pages;
        store.wiki_nav = nav;

        info!("Updated {} wiki pages", store.wiki_pages.len());
        Ok(())
    }

    /// Static version of update_wiki for use in background tasks
    async fn update_wiki_static(
        config: &PagesConfig,
        wiki_repo_path: &Option<PathBuf>,
        store: &Arc<RwLock<PageStore>>,
    ) -> Result<()> {
        let repo_url = config
            .wiki_repo()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wiki repo not configured"))?;
        let repo_path = wiki_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wiki repo path not set"))?;

        Self::sync_repository_static(repo_url, repo_path).await?;
        let pages = Self::build_pages_static(repo_path, PageType::Wiki, config.wiki_readme())?;
        let nav = Self::build_navigation_static(&pages);

        let mut store_guard = store.write().unwrap();
        store_guard.wiki_pages = pages;
        store_guard.wiki_nav = nav;

        info!("Updated {} wiki pages", store_guard.wiki_pages.len());
        Ok(())
    }

    /// Trigger site pages update from repository (public for API use)
    pub async fn trigger_site_update(&mut self) -> Result<()> {
        let repo_url = self
            .config
            .site_repo()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Site repo not configured"))?;
        let repo_path = self
            .site_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Site repo path not set"))?;

        Self::sync_repository_static(repo_url, repo_path).await?;

        // Get the default branch after sync
        self.site_default_branch = Self::get_default_branch_static(repo_path).ok();

        let pages = Self::build_pages_static(repo_path, PageType::Site, self.config.site_readme())?;
        let nav = Self::build_navigation_static(&pages);

        // Handle the site index page
        let site_index = pages
            .get(&Self::slug_from_filename_static(
                self.config.site_embed_index(),
            ))
            .cloned();

        let mut store = self.store.write().unwrap();
        store.site_pages = pages;
        store.site_nav = nav;
        store.site_index = site_index;

        info!("Updated {} site pages", store.site_pages.len());
        Ok(())
    }

    /// Static version of update_site for use in background tasks
    async fn update_site_static(
        config: &PagesConfig,
        site_repo_path: &Option<PathBuf>,
        store: &Arc<RwLock<PageStore>>,
    ) -> Result<()> {
        let repo_url = config
            .site_repo()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Site repo not configured"))?;
        let repo_path = site_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Site repo path not set"))?;

        Self::sync_repository_static(repo_url, repo_path).await?;
        let pages = Self::build_pages_static(repo_path, PageType::Site, config.site_readme())?;
        let nav = Self::build_navigation_static(&pages);

        let site_index = pages
            .get(&Self::slug_from_filename_static(config.site_embed_index()))
            .cloned();

        let mut store_guard = store.write().unwrap();
        store_guard.site_pages = pages;
        store_guard.site_nav = nav;
        store_guard.site_index = site_index;

        info!("Updated {} site pages", store_guard.site_pages.len());
        Ok(())
    }

    /// Static version of sync_repository
    async fn sync_repository_static(repo_url: &str, repo_path: &Path) -> Result<()> {
        if repo_path.exists() {
            // Repository exists, pull latest changes
            info!("Pulling updates from repository: {}", repo_url);
            let output = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .arg("pull")
                .output()
                .context("Failed to execute git pull")?;

            if !output.status.success() {
                warn!(
                    "Git pull failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                // Try to reset and pull again
                let reset_output = Command::new("git")
                    .arg("-C")
                    .arg(repo_path)
                    .arg("reset")
                    .arg("--hard")
                    .arg("HEAD")
                    .output()
                    .context("Failed to execute git reset")?;

                if !reset_output.status.success() {
                    return Err(anyhow::anyhow!(
                        "Git reset failed: {}",
                        String::from_utf8_lossy(&reset_output.stderr)
                    ));
                }

                let retry_output = Command::new("git")
                    .arg("-C")
                    .arg(repo_path)
                    .arg("pull")
                    .output()
                    .context("Failed to execute git pull after reset")?;

                if !retry_output.status.success() {
                    return Err(anyhow::anyhow!(
                        "Git pull failed even after reset: {}",
                        String::from_utf8_lossy(&retry_output.stderr)
                    ));
                }
            }
        } else {
            // Repository doesn't exist, clone it
            info!("Cloning repository: {} to {:?}", repo_url, repo_path);
            let output = Command::new("git")
                .arg("clone")
                .arg(repo_url)
                .arg(repo_path)
                .output()
                .context("Failed to execute git clone")?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Git clone failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        Ok(())
    }

    /// Static version of build_pages
    fn build_pages_static(
        repo_path: &Path,
        page_type: PageType,
        include_readme: bool,
    ) -> Result<HashMap<String, Page>> {
        let mut pages = HashMap::new();
        Self::scan_directory_static(repo_path, repo_path, &mut pages, page_type, include_readme)?;
        Ok(pages)
    }

    /// Static version of scan_directory
    fn scan_directory_static(
        base_path: &Path,
        current_path: &Path,
        pages: &mut HashMap<String, Page>,
        page_type: PageType,
        include_readme: bool,
    ) -> Result<()> {
        if !current_path.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(current_path)? {
            let entry = entry?;
            let path = entry.path();

            // Skip hidden files and directories
            if let Some(filename) = path.file_name() {
                let filename_str = filename.to_string_lossy();
                if filename_str.starts_with('.') {
                    continue;
                }

                // Skip README.md if not included
                if !include_readme && filename_str.eq_ignore_ascii_case("readme.md") {
                    continue;
                }
            }

            if path.is_dir() {
                Self::scan_directory_static(base_path, &path, pages, page_type, include_readme)?;
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "md" || ext == "markdown" {
                        if let Ok(page) = Self::build_page_static(&path, base_path, page_type) {
                            pages.insert(page.slug.clone(), page);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Static version of build_page
    fn build_page_static(file_path: &Path, base_path: &Path, page_type: PageType) -> Result<Page> {
        let raw_content = fs::read_to_string(file_path).context("Failed to read markdown file")?;

        // Convert markdown to HTML
        let mut html_content = markdown_to_html(&raw_content, &Options::default());

        // Post-process HTML to fix internal markdown links
        html_content = Self::fix_markdown_links(&html_content, page_type);

        // Extract title from the first H1 or use filename
        let title = Self::extract_title_static(&raw_content, file_path);

        // Create relative path and slug
        let relative_path = file_path
            .strip_prefix(base_path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let slug = Self::slug_from_path_static(&relative_path);

        let modified = fs::metadata(file_path).ok().and_then(|m| m.modified().ok());

        Ok(Page {
            title,
            html_content,
            raw_content,
            relative_path,
            slug,
            modified,
        })
    }

    /// Fix internal markdown links in HTML to point to proper routes
    /// Converts: <a href="STUFF.md">...</a> -> <a href="/wiki/stuff">...</a> or <a href="/site/stuff">...</a>
    /// Also handles: <a href="./path/to/FILE.md">...</a> keeping folder structure
    fn fix_markdown_links(html: &str, page_type: PageType) -> String {
        use regex::Regex;

        // Determine the route prefix based on page type
        let route_prefix = match page_type {
            PageType::Wiki => "/wiki",
            PageType::Site => "/page",
        };

        // Match markdown file links: href="something.md" or href="./path/file.md"
        let re = Regex::new(r#"href="([^"]*\.md(?:arkdown)?)""#).unwrap();

        re.replace_all(html, |caps: &regex::Captures| {
            let original_link = &caps[1];

            // Skip external links (http://, https://, etc.)
            if original_link.starts_with("http://")
                || original_link.starts_with("https://")
                || original_link.starts_with("//")
            {
                return caps[0].to_string();
            }

            // Convert the markdown filename to a slug, preserving folder structure
            let slug = original_link
                .trim_start_matches("./")
                .trim_start_matches("../")
                .trim_end_matches(".md")
                .trim_end_matches(".markdown")
                .replace('\\', "/") // Normalize Windows paths
                .to_lowercase();

            // Return the fixed link pointing to the appropriate route with folder structure
            format!(r#"href="{}/{}""#, route_prefix, slug)
        })
        .to_string()
    }

    /// Static version of extract_title
    fn extract_title_static(content: &str, file_path: &Path) -> String {
        // Try to find first H1 header
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") {
                return trimmed[2..].trim().to_string();
            }
        }

        // Fall back to filename without extension
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    }

    /// Static version of slug_from_path
    fn slug_from_path_static(path: &str) -> String {
        path.trim_start_matches('/')
            .trim_end_matches(".md")
            .trim_end_matches(".markdown")
            .replace('\\', "/") // Normalize Windows paths
            .to_lowercase()
        // Keep folder separators - don't replace / with -
    }

    /// Static version of slug_from_filename
    fn slug_from_filename_static(filename: &str) -> String {
        filename
            .trim_end_matches(".md")
            .trim_end_matches(".markdown")
            .to_lowercase()
    }

    /// Static version of build_navigation
    fn build_navigation_static(pages: &HashMap<String, Page>) -> Vec<NavItem> {
        // First, collect all pages and organize them by their path components
        let mut root_items: Vec<NavItem> = Vec::new();
        let mut folder_map: std::collections::HashMap<String, Vec<NavItem>> =
            std::collections::HashMap::new();

        for page in pages.values() {
            let slug_parts: Vec<&str> = page.slug.split('/').collect();

            if slug_parts.len() == 1 {
                // Root level page - add directly to root_items
                root_items.push(NavItem {
                    title: page.title.clone(),
                    slug: page.slug.clone(),
                    path: page.relative_path.clone(),
                    children: vec![],
                });
            } else {
                // Nested page - group by parent folder
                let parent_folder = slug_parts[0].to_string();
                let child_item = NavItem {
                    title: page.title.clone(),
                    slug: page.slug.clone(),
                    path: page.relative_path.clone(),
                    children: vec![],
                };

                folder_map
                    .entry(parent_folder.clone())
                    .or_insert_with(Vec::new)
                    .push(child_item);
            }
        }

        // Now attach children to their parent items
        for item in root_items.iter_mut() {
            if let Some(children) = folder_map.get(&item.slug) {
                item.children = children.clone();
                // Sort children by title
                item.children.sort_by(|a, b| a.title.cmp(&b.title));
            }
        }

        // Sort root items by title
        root_items.sort_by(|a, b| a.title.cmp(&b.title));

        root_items
    }

    /// Get the default branch of a git repository
    fn get_default_branch_static(repo_path: &Path) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("symbolic-ref")
            .arg("refs/remotes/origin/HEAD")
            .output()
            .context("Failed to execute git symbolic-ref")?;

        if output.status.success() {
            let branch_ref = String::from_utf8_lossy(&output.stdout);
            // Output is like "refs/remotes/origin/main\n"
            // Extract just "main"
            if let Some(branch) = branch_ref.trim().strip_prefix("refs/remotes/origin/") {
                return Ok(branch.to_string());
            }
        }

        // Fallback: try to get the current branch
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .output()
            .context("Failed to execute git rev-parse")?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                return Ok(branch);
            }
        }

        // Last resort fallback
        Ok("main".to_string())
    }
}

impl PagesConfig {
    /// Getters for the private fields
    pub fn wiki_repo(&self) -> &Option<String> {
        &self.wiki_repo
    }

    pub fn site_repo(&self) -> &Option<String> {
        &self.site_repo
    }

    pub fn site_embed_index(&self) -> &str {
        &self.site_embed_index
    }

    pub fn wiki_auto_enabled(&self) -> bool {
        self.wiki_auto_enabled
    }

    pub fn wiki_period(&self) -> usize {
        self.wiki_period
    }

    pub fn wiki_readme(&self) -> bool {
        self.wiki_readme
    }

    pub fn site_auto_enabled(&self) -> bool {
        self.site_auto_enabled
    }

    pub fn site_period(&self) -> usize {
        self.site_period
    }

    pub fn site_readme(&self) -> bool {
        self.site_readme
    }

    pub fn user_readme(&self) -> bool {
        self.user_readme
    }

    pub fn wiki_link(&self) -> &crate::config::LinkLocation {
        &self.wiki_link
    }

    pub fn site_link(&self) -> &crate::config::LinkLocation {
        &self.site_link
    }

    pub fn wiki_repo_exists(&self) -> bool {
        self.wiki_repo.is_some()
    }

    pub fn site_repo_exists(&self) -> bool {
        self.site_repo.is_some()
    }
}
