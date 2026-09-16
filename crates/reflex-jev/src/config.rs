use std::time::Duration;

#[derive(Debug, Clone)]
pub struct JevConfig {
    pub base_url: String,
    pub api_key: String,
    pub timeout: Duration,
    pub max_retries: u32,
    pub initial_backoff: Duration,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            base_url: std::env::var("JEV_API_BASE")
                .unwrap_or_else(|_| "https://api.jev.ai/v1".to_string()),
            api_key: std::env::var("JEV_API_KEY").unwrap_or_default(),
            timeout: Duration::from_millis(1500),
            max_retries: 3,
            initial_backoff: Duration::from_millis(50),
        }
    }
}

impl JevConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }
}
