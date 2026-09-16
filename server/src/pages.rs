use crate::config::PagesConfig;
use anyhow::{Context, Result};
use comrak::{markdown_to_html, Options};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::process::Command;
use tokio::time::{sleep, timeout, Duration};
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

/// Navigation item for building menus.
///
/// The tree itself is built by [`css_lib::nav`], which the server crate cannot
/// be compiled on every developer machine to test -- and which is why #81, a
/// navigation that silently discarded three quarters of the wiki, went
/// unnoticed until an administrator complained that the refresh button did
/// nothing. This is an alias rather than a parallel struct so the wire shape
/// and the builder cannot drift apart.
pub type NavItem = css_lib::nav::NavNode;

/// A rebuilt page set, produced with no lock held and swapped in afterwards.
///
/// The split exists so that the fetch and the render happen outside the store's
/// lock rather than inside it -- see [`PagesService::prepare`].
#[derive(Debug)]
pub struct PreparedPages {
    page_type: PageType,
    pages: HashMap<String, Page>,
    nav: Vec<NavItem>,
    /// Only ever `Some` for [`PageType::Site`].
    site_index: Option<Page>,
    default_branch: Option<String>,
}

impl PreparedPages {
    /// How many pages were built.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

pub struct PagesService {
    config: PagesConfig,
    store: Arc<RwLock<PageStore>>,
    wiki_repo_path: Option<PathBuf>,
    site_repo_path: Option<PathBuf>,
    wiki_default_branch: Option<String>,
    site_default_branch: Option<String>,
    /// Serialises refreshes of one checkout against each other. Not the store's
    /// lock: this one may be held across a network fetch precisely because no
    /// request handler ever waits on it.
    wiki_sync: Arc<tokio::sync::Mutex<()>>,
    site_sync: Arc<tokio::sync::Mutex<()>>,
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
            wiki_sync: Arc::new(tokio::sync::Mutex::new(())),
            site_sync: Arc::new(tokio::sync::Mutex::new(())),
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
            let gate = Arc::clone(&service.wiki_sync);

            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(period as u64)).await;
                    info!("Auto-updating wiki pages");
                    if let Err(e) =
                        Self::update_wiki_static(&config, &wiki_repo_path, &store, &gate).await
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
            let gate = Arc::clone(&service.site_sync);

            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(period as u64)).await;
                    info!("Auto-updating site pages");
                    if let Err(e) =
                        Self::update_site_static(&config, &site_repo_path, &store, &gate).await
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

    /// Fetch a repository and rebuild its pages, touching neither the store nor
    /// any lock that a request handler needs.
    ///
    /// This is the expensive half -- a network fetch, a directory walk, and a
    /// markdown render per file -- and none of it needs the store, so none of it
    /// is done while holding the store. `refresh_wiki_pages` used to hold the
    /// service's tokio write lock across all of it, which queued every reader of
    /// `/api/pages/*` behind an administrator's git pull (#94).
    ///
    /// `gate` serialises refreshes against each other and against the background
    /// updater, which the write lock used to do as a side effect. It is taken
    /// here rather than at the call sites so that preparing without it is not
    /// something a caller can forget: two `git pull`s in one working tree fight
    /// over `index.lock`, and the loser reports a refresh failure that has
    /// nothing to do with the repository.
    pub async fn prepare(
        config: &PagesConfig,
        repo_path: &Path,
        page_type: PageType,
        gate: &tokio::sync::Mutex<()>,
    ) -> Result<PreparedPages> {
        let _serialised = gate.lock().await;

        let (repo_url, include_readme) = match page_type {
            PageType::Wiki => (config.wiki_repo().clone(), config.wiki_readme()),
            PageType::Site => (config.site_repo().clone(), config.site_readme()),
        };
        let repo_url =
            repo_url.ok_or_else(|| anyhow::anyhow!("{page_type:?} repo not configured"))?;
        let deadline = Duration::from_secs(config.git_timeout_secs());

        Self::sync_repository_static(&repo_url, repo_path, deadline).await?;
        let default_branch = Self::get_default_branch_static(repo_path, deadline)
            .await
            .ok();

        // Walking the repository and rendering every file is disk and CPU work
        // with no await points in it. Left on the runtime it parks a worker for
        // the same reason the git calls did, just for less time.
        let owned_path = repo_path.to_path_buf();
        let pages = tokio::task::spawn_blocking(move || {
            Self::build_pages_static(&owned_path, page_type, include_readme)
        })
        .await
        .context("Page build task failed")??;

        let nav = Self::build_navigation_static(&pages);
        let site_index = match page_type {
            PageType::Site => pages
                .get(&Self::slug_from_filename_static(config.site_embed_index()))
                .cloned(),
            PageType::Wiki => None,
        };

        Ok(PreparedPages {
            page_type,
            pages,
            nav,
            site_index,
            default_branch,
        })
    }

    /// Swap a prepared set into the store.
    ///
    /// Synchronous and brief: the only part of a refresh that needs exclusive
    /// access to the service, and the only part worth making readers wait for.
    pub fn publish(&mut self, prepared: PreparedPages) -> usize {
        match prepared.page_type {
            PageType::Wiki => self.wiki_default_branch = prepared.default_branch.clone(),
            PageType::Site => self.site_default_branch = prepared.default_branch.clone(),
        }
        Self::publish_into(&self.store, prepared)
    }

    /// The store half of [`Self::publish`], shared with the background updaters
    /// so that the manual and automatic paths cannot drift apart.
    fn publish_into(store: &Arc<RwLock<PageStore>>, prepared: PreparedPages) -> usize {
        let mut guard = store.write().unwrap();
        let count = prepared.pages.len();
        match prepared.page_type {
            PageType::Wiki => {
                guard.wiki_pages = prepared.pages;
                guard.wiki_nav = prepared.nav;
            }
            PageType::Site => {
                guard.site_pages = prepared.pages;
                guard.site_nav = prepared.nav;
                guard.site_index = prepared.site_index;
            }
        }
        count
    }

    /// What a request handler needs to run a refresh after letting go of the
    /// service: the configuration, where the checkout lives, and the gate that
    /// keeps concurrent refreshes out of each other's working tree.
    ///
    /// Returns `None` when no repository of this kind is configured.
    pub fn refresh_inputs(
        &self,
        page_type: PageType,
    ) -> Option<(PagesConfig, PathBuf, Arc<tokio::sync::Mutex<()>>)> {
        let (path, gate) = match page_type {
            PageType::Wiki => (self.wiki_repo_path.clone()?, Arc::clone(&self.wiki_sync)),
            PageType::Site => (self.site_repo_path.clone()?, Arc::clone(&self.site_sync)),
        };
        Some((self.config.clone(), path, gate))
    }

    /// Trigger wiki pages update from repository (public for API use)
    pub async fn trigger_wiki_update(&mut self) -> Result<()> {
        let (config, path, gate) = self
            .refresh_inputs(PageType::Wiki)
            .ok_or_else(|| anyhow::anyhow!("Wiki repo path not set"))?;
        let prepared = Self::prepare(&config, &path, PageType::Wiki, &gate).await?;
        info!("Updated {} wiki pages", self.publish(prepared));
        Ok(())
    }

    /// Static version of update_wiki for use in background tasks
    async fn update_wiki_static(
        config: &PagesConfig,
        wiki_repo_path: &Option<PathBuf>,
        store: &Arc<RwLock<PageStore>>,
        gate: &tokio::sync::Mutex<()>,
    ) -> Result<()> {
        let repo_path = wiki_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wiki repo path not set"))?;
        let prepared = Self::prepare(config, repo_path, PageType::Wiki, gate).await?;
        info!("Updated {} wiki pages", Self::publish_into(store, prepared));
        Ok(())
    }

    /// Trigger site pages update from repository (public for API use)
    pub async fn trigger_site_update(&mut self) -> Result<()> {
        let (config, path, gate) = self
            .refresh_inputs(PageType::Site)
            .ok_or_else(|| anyhow::anyhow!("Site repo path not set"))?;
        let prepared = Self::prepare(&config, &path, PageType::Site, &gate).await?;
        info!("Updated {} site pages", self.publish(prepared));
        Ok(())
    }

    /// Static version of update_site for use in background tasks
    async fn update_site_static(
        config: &PagesConfig,
        site_repo_path: &Option<PathBuf>,
        store: &Arc<RwLock<PageStore>>,
        gate: &tokio::sync::Mutex<()>,
    ) -> Result<()> {
        let repo_path = site_repo_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Site repo path not set"))?;
        let prepared = Self::prepare(config, repo_path, PageType::Site, gate).await?;
        info!("Updated {} site pages", Self::publish_into(store, prepared));
        Ok(())
    }

    /// Run one git command with a deadline, on the runtime rather than on a
    /// worker thread.
    ///
    /// `std::process::Command::output()` blocks the calling OS thread until the
    /// child exits. Inside an `async fn` that parks a tokio worker: it cannot be
    /// preempted and its task cannot be stolen, so a `git pull` against an
    /// unreachable remote takes a worker out of the pool for as long as the
    /// transport allows. Tokio sizes that pool at one worker per core, which on
    /// a two-core host is half the runtime -- the same arithmetic that made the
    /// MQTT version of this bug a total outage rather than a stall
    /// (`checks/tests/mqtt_never_blocks_the_runtime.rs`).
    ///
    /// `kill_on_drop` is not optional here. Dropping the future a timeout
    /// abandons would otherwise leave `git` running, and an abandoned `git pull`
    /// keeps `index.lock` -- so the timeout meant to recover the service would
    /// be what stopped every later refresh from starting.
    async fn run_git(
        mut cmd: Command,
        what: &str,
        deadline: Duration,
    ) -> Result<std::process::Output> {
        cmd.kill_on_drop(true);
        match timeout(deadline, cmd.output()).await {
            Ok(result) => result.with_context(|| format!("Failed to execute git {what}")),
            Err(_) => Err(anyhow::anyhow!(
                "git {what} did not finish within {}s",
                deadline.as_secs()
            )),
        }
    }

    /// Fetch the repository into `repo_path`, cloning it if it is not there yet.
    ///
    /// Every call is bounded by `deadline`: an unreachable remote must fail the
    /// refresh, not hold the service open until the transport gives up.
    async fn sync_repository_static(
        repo_url: &str,
        repo_path: &Path,
        deadline: Duration,
    ) -> Result<()> {
        if repo_path.exists() {
            // Repository exists, pull latest changes
            info!("Pulling updates from repository: {}", repo_url);
            let mut pull = Command::new("git");
            pull.arg("-C").arg(repo_path).arg("pull");
            let output = Self::run_git(pull, "pull", deadline).await?;

            if !output.status.success() {
                warn!(
                    "Git pull failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                // Try to reset and pull again
                let mut reset = Command::new("git");
                reset
                    .arg("-C")
                    .arg(repo_path)
                    .arg("reset")
                    .arg("--hard")
                    .arg("HEAD");
                let reset_output = Self::run_git(reset, "reset", deadline).await?;

                if !reset_output.status.success() {
                    return Err(anyhow::anyhow!(
                        "Git reset failed: {}",
                        String::from_utf8_lossy(&reset_output.stderr)
                    ));
                }

                let mut retry = Command::new("git");
                retry.arg("-C").arg(repo_path).arg("pull");
                let retry_output = Self::run_git(retry, "pull after reset", deadline).await?;

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
            let mut clone = Command::new("git");
            clone.arg("clone").arg(repo_url).arg(repo_path);
            let output = Self::run_git(clone, "clone", deadline).await?;

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
    /// Build the navigation tree for a set of pages.
    ///
    /// The tree-shaping lives in [`css_lib::nav::build_navigation`]; this is the
    /// adapter that hands it the three fields it needs. See that module for what
    /// the previous implementation got wrong.
    fn build_navigation_static(pages: &HashMap<String, Page>) -> Vec<NavItem> {
        let sources: Vec<css_lib::nav::NavSource> = pages
            .values()
            .map(|page| css_lib::nav::NavSource {
                title: page.title.clone(),
                slug: page.slug.clone(),
                path: page.relative_path.clone(),
            })
            .collect();
        css_lib::nav::build_navigation(&sources)
    }

    /// Get the default branch of a git repository.
    ///
    /// Bounded like every other git call here: this runs immediately after a
    /// fetch, on the same runtime, and `symbolic-ref` on a repository whose
    /// filesystem has gone away can hang as readily as a network operation.
    async fn get_default_branch_static(repo_path: &Path, deadline: Duration) -> Result<String> {
        let mut symbolic = Command::new("git");
        symbolic
            .arg("-C")
            .arg(repo_path)
            .arg("symbolic-ref")
            .arg("refs/remotes/origin/HEAD");
        let output = Self::run_git(symbolic, "symbolic-ref", deadline).await?;

        if output.status.success() {
            let branch_ref = String::from_utf8_lossy(&output.stdout);
            // Output is like "refs/remotes/origin/main\n"
            // Extract just "main"
            if let Some(branch) = branch_ref.trim().strip_prefix("refs/remotes/origin/") {
                return Ok(branch.to_string());
            }
        }

        // Fallback: try to get the current branch
        let mut rev_parse = Command::new("git");
        rev_parse
            .arg("-C")
            .arg(repo_path)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD");
        let output = Self::run_git(rev_parse, "rev-parse", deadline).await?;

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

    /// How long any one git invocation may run before a refresh gives up.
    pub fn git_timeout_secs(&self) -> u64 {
        self.git_timeout_secs
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
