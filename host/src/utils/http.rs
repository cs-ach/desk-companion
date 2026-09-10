use std::collections::HashMap;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> Self {
        let client = reqwest::Client::new();
        Self { client } 
    }

    pub async fn get(&self, url: &str, headers: HashMap<String, String>) -> Result<String, reqwest::Error> {
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(key.as_bytes()), HeaderValue::from_str(&value)) {
                header_map.insert(name, val);
            }
        }

        let response = self.client
            .get(url)
            .headers(header_map)
            .send()
            .await?;

        response.text().await
    }

    pub async fn post<B>(&self, url: &str, headers: HashMap<String, String>, body: B) -> Result<String, reqwest::Error> where B: Into<reqwest::Body>,
    {
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(key.as_bytes()), HeaderValue::from_str(&value)) {
                header_map.insert(name, val);
            }
        }

        let response = self.client
            .post(url)
            .headers(header_map)
            .body(body)
            .send()
            .await?;

        response.text().await
    }
}
