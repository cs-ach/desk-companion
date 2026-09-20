use std::collections::HashMap;
use rquickjs::{Ctx, Exception, Function, IntoJs, Object, Value, function::{Async, Opt}};
use plugin::Plugin;
use reqwest::Method;

use crate::describe;
use crate::http::{FetchOptions, HttpClient, HttpResponse};

pub fn fetch<'js>(ctx: &Ctx<'js>, _plugin: &Plugin, http_client: HttpClient) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(ctx.clone(), Async(move |ctx: Ctx<'js>, url: String, options: Opt<Object<'js>>| {
        let parsed = parse_options(&ctx, options);
        let client = http_client.clone();

        async move {
            let options = parsed?;
            let response = client
                .fetch(url, options)
                .await
                .map_err(|e| Exception::throw_message(&ctx, &describe(&e)))?;

            Ok::<_, rquickjs::Error>(response)
        }
    }))?;
    Ok(function.into_value())
}

fn parse_options<'js>(ctx: &Ctx<'js>, options: Opt<Object<'js>>) -> rquickjs::Result<FetchOptions> {
    let Some(obj) = options.0 else {
        return Ok(FetchOptions { method: Method::GET, headers: None, body: None });
    };

    let method_name = obj
        .get::<_, Option<String>>("method")?
        .unwrap_or_else(|| "GET".to_string())
        .to_ascii_uppercase();

    let headers = obj.get::<_, Option<HashMap<String, String>>>("headers")?;
    let body = obj.get::<_, Option<String>>("body")?;

    let method = Method::from_bytes(method_name.as_bytes())
        .ok()
        .filter(|m| matches!(*m, Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE))
        .ok_or_else(|| Exception::throw_message(ctx, &format!("unsupported fetch method: {method_name}")))?;

    Ok(FetchOptions { method, headers, body })
}

impl<'js> IntoJs<'js> for HttpResponse {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>, rquickjs::Error> {
        let obj = Object::new(ctx.clone())?;
        obj.set("status", self.status_code)?;
        obj.set("ok", (200..300).contains(&self.status_code))?;
        obj.set("headers", self.headers)?;
        obj.set("body", self.body)?;
        Ok(obj.into_value())
    }
}
