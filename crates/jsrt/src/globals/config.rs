use rquickjs::{Ctx, IntoJs, Value};
use serde_json::{Map, Value as JsonValue};

use crate::json::Json;

pub fn config<'js>(ctx: &Ctx<'js>, values: Map<String, JsonValue>) -> rquickjs::Result<Value<'js>> {
    Json(JsonValue::Object(values)).into_js(ctx)
}
