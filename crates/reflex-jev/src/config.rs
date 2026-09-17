use std::time::Duration;

pub const OFFICIAL_JEV_SYSTEMONE_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const DEFAULT_JEV_MODEL: &str = "jev-latest";

#[derive(Debug, Clone)]
pub struct JevConfig {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub max_retries: u32,
    pub initial_backoff: Duration,
}

impl JevConfig {
    /// Constructs a JevConfig strictly from environment variables.
    /// Fails fast if JEV_API_KEY (or TYPESAFE_API_KEY) is missing or empty.
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("JEV_API_KEY")
            .or_else(|_| std::env::var("TYPESAFE_API_KEY"))
            .map_err(|_| {
                "JEV_API_KEY environment variable is strictly required when using --provider jev. Please run: export JEV_API_KEY=\"...\"".to_string()
            })?;

        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(
                "JEV_API_KEY is empty. A valid Jev/TypeSafe API key is strictly required."
                    .to_string(),
            );
        }

        let endpoint = std::env::var("JEV_API_BASE")
            .or_else(|_| std::env::var("TYPESAFE_API_BASE"))
            .unwrap_or_else(|_| OFFICIAL_JEV_SYSTEMONE_ENDPOINT.to_string());

        let model = std::env::var("JEV_MODEL").unwrap_or_else(|_| DEFAULT_JEV_MODEL.to_string());

        Ok(Self {
            endpoint,
            api_key,
            model,
            timeout: Duration::from_secs(15),
            max_retries: 3,
            initial_backoff: Duration::from_millis(150),
        })
    }

    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            endpoint: OFFICIAL_JEV_SYSTEMONE_ENDPOINT.to_string(),
            api_key: api_key.into(),
            model: DEFAULT_JEV_MODEL.to_string(),
            timeout: Duration::from_secs(15),
            max_retries: 3,
            initial_backoff: Duration::from_millis(150),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_jev_config_defaults() {
        let cfg = JevConfig::new("test_key");
        assert_eq!(cfg.endpoint, OFFICIAL_JEV_SYSTEMONE_ENDPOINT);
        assert_eq!(cfg.model, DEFAULT_JEV_MODEL);
        assert_eq!(cfg.api_key, "test_key");
    }

    #[test]
    fn test_jev_config_env_lifecycle() {
        let _guard = ENV_LOCK.lock().unwrap();

        // 1. Missing
        std::env::remove_var("JEV_API_KEY");
        std::env::remove_var("TYPESAFE_API_KEY");
        let res = JevConfig::from_env();
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("JEV_API_KEY environment variable is strictly required"));

        // 2. Empty
        std::env::set_var("JEV_API_KEY", "   ");
        let res = JevConfig::from_env();
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("JEV_API_KEY is empty"));

        // 3. Present
        std::env::set_var("JEV_API_KEY", "dummy_key_123");
        let cfg = JevConfig::from_env().unwrap();
        assert_eq!(cfg.api_key, "dummy_key_123");
        assert_eq!(cfg.endpoint, OFFICIAL_JEV_SYSTEMONE_ENDPOINT);
        assert_eq!(cfg.model, DEFAULT_JEV_MODEL);

        std::env::remove_var("JEV_API_KEY");
    }
}
