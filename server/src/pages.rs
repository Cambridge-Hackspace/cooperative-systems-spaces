use crate::config::{PagesConfig};

pub struct PagesService {
    config: PagesConfig,
}

impl PagesService {
    /// Create a new pages service
    pub fn new(config: PagesConfig) -> Self {
        Self {
            config,
        }
    }
}