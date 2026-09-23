use crate::config::JevConfig;
use crate::types::{SystemOneRequest, SystemOneResponse};
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

    pub fn config(&self) -> &JevConfig {
        &self.config
    }

    pub async fn execute_system_one(
        &self,
        request: &SystemOneRequest,
    ) -> Result<SystemOneResponse, ProviderError> {
        let url = &self.config.endpoint;
        let mut attempts = 0;
        let mut backoff = self.config.initial_backoff;

        loop {
            attempts += 1;
            let res = self.http.post(url).json(request).send().await;

            match res {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json::<SystemOneResponse>().await {
                            Ok(parsed) => return Ok(parsed),
                            Err(_) => {
                                return Err(ProviderError::MalformedResponse(
                                    "Failed to deserialize TypeSafe SystemOne response".to_string(),
                                ));
                            }
                        }
                    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
                    {
                        return Err(ProviderError::AuthenticationFailed(format!(
                            "HTTP {status} from TypeSafe Jev API"
                        )));
                    } else if status == StatusCode::TOO_MANY_REQUESTS {
                        if attempts <= self.config.max_retries {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(2));
                            continue;
                        }
                        return Err(ProviderError::RateLimited(
                            "TypeSafe Jev API rate limit exceeded (429 Too Many Requests)"
                                .to_string(),
                        ));
                    } else if status.is_server_error() {
                        if attempts <= self.config.max_retries {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(2));
                            continue;
                        }
                        return Err(ProviderError::Unavailable(format!(
                            "TypeSafe Jev API server error {status}"
                        )));
                    } else {
                        return Err(ProviderError::InvalidRequest(format!(
                            "TypeSafe Jev API error {status}"
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
                        return Err(ProviderError::Timeout(
                            "Request to TypeSafe Jev API timed out".to_string(),
                        ));
                    }
                    if attempts <= self.config.max_retries {
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(2));
                        continue;
                    }
                    return Err(ProviderError::Network(
                        "Network error contacting TypeSafe Jev API".to_string(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SystemOneRequest;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[tokio::test]
    async fn api_errors_do_not_echo_response_body_or_endpoint_secrets() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response_body = "response-body-secret";
        let response_body_for_server = response_body.to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body_for_server.len(),
                response_body_for_server
            )
            .unwrap();
        });

        let api_key = "configured-api-key-secret";
        let endpoint_secret = "endpoint-query-secret";
        let config = JevConfig::new(api_key).with_endpoint(format!(
            "http://{address}/v1/systemone?token={endpoint_secret}"
        ));
        let request = SystemOneRequest {
            model: "jev-latest".to_string(),
            state: "synthetic test state".to_string(),
            questions: HashMap::new(),
        };

        let error = JevClient::new(config)
            .execute_system_one(&request)
            .await
            .unwrap_err()
            .to_string();
        server.join().unwrap();

        assert!(!error.contains(api_key));
        assert!(!error.contains(endpoint_secret));
        assert!(!error.contains(response_body));
    }
}
