use rquickjs::{Array, Ctx, IntoJs, Object, Value};
use serde_json::Value as JsonValue;

pub struct Json(pub JsonValue);

impl<'js> IntoJs<'js> for Json {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        match self.0 {
            JsonValue::Null => Ok(Value::new_null(ctx.clone())),

            JsonValue::Bool(b) => b.into_js(ctx),

            JsonValue::Number(n) => n.as_f64().unwrap_or(f64::NAN).into_js(ctx),

            JsonValue::String(s) => s.into_js(ctx),

            JsonValue::Array(items) => {
                let array = Array::new(ctx.clone())?;
                for (i, item) in items.into_iter().enumerate() {
                    array.set(i, Json(item))?;
                }
                Ok(array.into_value())
            }

            JsonValue::Object(fields) => {
                let object = Object::new(ctx.clone())?;
                for (key, value) in fields {
                    object.set(key, Json(value))?;
                }
                Ok(object.into_value())
            }
        }
    }
}
