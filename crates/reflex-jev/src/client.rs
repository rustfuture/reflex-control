use crate::config::JevConfig;
use crate::types::{JevDecisionRequest, JevDecisionResponse};
use reflex_provider::ProviderError;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use std::time::Duration;

#[derive(Clone)]
pub struct JevClient {
    config: JevConfig,
    http: reqwest::Client,
}

impl JevClient {
    pub fn new(config: JevConfig) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if !config.api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", config.api_key)) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .default_headers(headers)
            .build()
            .unwrap_or_default();

        Self { config, http }
    }

    pub async fn post_decision(
        &self,
        endpoint: &str,
        request: &JevDecisionRequest,
    ) -> Result<JevDecisionResponse, ProviderError> {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        let mut attempts = 0;
        let mut backoff = self.config.initial_backoff;

        loop {
            attempts += 1;
            let res = self.http.post(&url).json(request).send().await;

            match res {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json::<JevDecisionResponse>().await {
                            Ok(parsed) => return Ok(parsed),
                            Err(e) => {
                                return Err(ProviderError::MalformedResponse(format!(
                                    "Failed to parse Jev response: {e}"
                                )))
                            }
                        }
                    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
                    {
                        let msg = response.text().await.unwrap_or_default();
                        return Err(ProviderError::AuthenticationFailed(format!(
                            "HTTP {status}: {msg}"
                        )));
                    } else if status == StatusCode::TOO_MANY_REQUESTS {
                        if attempts <= self.config.max_retries {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(2));
                            continue;
                        }
                        return Err(ProviderError::RateLimited(
                            "Exceeded maximum retries on 429 Too Many Requests".to_string(),
                        ));
                    } else if status.is_server_error() {
                        if attempts <= self.config.max_retries {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(2));
                            continue;
                        }
                        let msg = response.text().await.unwrap_or_default();
                        return Err(ProviderError::Unavailable(format!(
                            "Server error {status}: {msg}"
                        )));
                    } else {
                        let msg = response.text().await.unwrap_or_default();
                        return Err(ProviderError::InvalidRequest(format!(
                            "HTTP {status}: {msg}"
                        )));
                    }
                }
                Err(err) => {
                    if err.is_timeout() {
                        if attempts <= self.config.max_retries {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(2));
                            continue;
                        }
                        return Err(ProviderError::Timeout(format!(
                            "Request to {url} timed out"
                        )));
                    }
                    if attempts <= self.config.max_retries {
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(2));
                        continue;
                    }
                    return Err(ProviderError::Network(format!(
                        "Network error contacting Jev: {err}"
                    )));
                }
            }
        }
    }
}
