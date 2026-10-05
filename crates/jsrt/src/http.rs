use std::{collections::HashMap, time::Duration};
use base64::Engine;
use reqwest::{Method, header::{HeaderMap, HeaderName, HeaderValue}};

pub struct HttpResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub binary: bool,
}

pub struct FetchOptions {
    pub method: Method,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub binary: bool,
}

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> Self {
        let client = reqwest::Client::new();
        Self { client }
    }

    pub async fn fetch(&self, url: String, options: FetchOptions) -> Result<HttpResponse, reqwest::Error> {

        let binary = options.binary;
        let header_map = self.header_to_map(options.headers.unwrap_or_default());

        let mut req = self.client.request(options.method, url)
            .timeout(Duration::from_secs(10))
            .headers(header_map);

        if let Some(b) = options.body { req = req.body(b) }

        let resp = req.send().await?;

        let status_code = resp.status().as_u16();
        let headers = self.map_to_header(resp.headers());

        let body = if binary {
            base64::engine::general_purpose::STANDARD.encode(resp.bytes().await?)
        } else {
            resp.text().await?
        };

        Ok(HttpResponse { status_code, headers, body, binary })
    }

    fn header_to_map(&self, headers: HashMap<String, String>) -> HeaderMap{
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(key.as_bytes()), HeaderValue::from_str(&value)) {
                header_map.insert(name, val);
            }
        }
        header_map
    }

    fn map_to_header(&self, headers:&HeaderMap) -> HashMap<String, String>{
        let mut header_hashmap = HashMap::new();
        for (key, value) in headers.iter(){
            if let Ok(v) = value.to_str(){
                header_hashmap.insert(key.to_string(), v.to_owned());
            }
        }
        header_hashmap
    }

}
