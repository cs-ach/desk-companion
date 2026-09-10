use std::collections::HashMap;

use jsrt::rquickjs::{self, Ctx, Function, Value};
use plugin::Plugin;
use reqwest::Body;

use crate::http;

pub fn log<'js>(ctx: &Ctx<'js>, plugin: &Plugin) -> rquickjs::Result<Value<'js>> {
    let id = plugin.manifest.id.clone();
    let function = Function::new(ctx.clone(), move |level: String, message: String| {
        println!("[{id}] {level}: {message}");
    })?;
    Ok(function.into_value())
}

pub async fn http_get(url: &str, headers: HashMap<String, String>, http_client: http::HttpClient) -> String {
    let resp = http_client.get(url, headers).await;
    return resp.unwrap();
}

pub async fn http_post(url: &str, headers: HashMap<String, String>, body: Body, http_client: http::HttpClient) -> String {
    let resp = http_client.post(url, headers, body).await;
    return resp.unwrap();
}
