use rquickjs::{Array, Ctx, Exception, FromJs, IntoJs, Object, Type, Value};
use serde_json::{Map, Number, Value as JsonValue};

const MAX_DEPTH: u32 = 32;

pub struct Json(pub JsonValue);

impl Default for Json {
   fn default() -> Self {
       Self(JsonValue::Null)
   } 
}

impl<'js> IntoJs<'js> for Json {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        match self.0 {
            JsonValue::Null => Ok(Value::new_null(ctx.clone())),

            JsonValue::Bool(b) => b.into_js(ctx),

            JsonValue::Number(n) => n.as_f64().unwrap_or(f64::NAN).into_js(ctx),

            JsonValue::String(s) => s.into_js(ctx),

            JsonValue::Array(items) => {
                let array: Array<'js> = Array::new(ctx.clone())?;
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

impl<'js> FromJs<'js> for Json {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<Self> {
        from_js_at(ctx, value, 0)
    }
}

fn from_js_at<'js>(ctx: &Ctx<'js>, value: Value<'js>, depth: u32) -> rquickjs::Result<Json> {
    if depth > MAX_DEPTH {
        return Err(Exception::throw_range(
            ctx,
            &format!("value nested deeper than {MAX_DEPTH} levels"),
        ));
    }

    match value.type_of() {
        Type::Uninitialized | Type::Undefined | Type::Null => Ok(Json(JsonValue::Null)),

        Type::Bool => {
            let b = value
                .as_bool()
                .ok_or_else(|| Exception::throw_type(ctx, "expected a boolean"))?;
            Ok(Json(JsonValue::Bool(b)))
        }

        Type::Int => {
            let i = value
                .as_int()
                .ok_or_else(|| Exception::throw_type(ctx, "expected an integer"))?;
            Ok(Json(JsonValue::Number(Number::from(i))))
        }

        Type::Float => {
            let f = value
                .as_float()
                .ok_or_else(|| Exception::throw_type(ctx, "expected a number"))?;
            Number::from_f64(f)
                .map(|n| Json(JsonValue::Number(n)))
                .ok_or_else(|| {
                    Exception::throw_type(ctx, &format!("{f} has no JSON representation"))
                })
        }

        Type::String => {
            let s = value
                .as_string()
                .ok_or_else(|| Exception::throw_type(ctx, "expected a string"))?
                .to_string()?;
            Ok(Json(JsonValue::String(s)))
        }

        Type::Array => {
            let array = value
                .as_array()
                .ok_or_else(|| Exception::throw_type(ctx, "expected an array"))?;

            let mut items = Vec::with_capacity(array.len());
            for item in array.iter::<Value>() {
                items.push(from_js_at(ctx, item?, depth + 1)?.0);
            }
            Ok(Json(JsonValue::Array(items)))
        }

        Type::Object => {
            let object = value
                .as_object()
                .ok_or_else(|| Exception::throw_type(ctx, "expected an object"))?;

            let mut fields = Map::new();
            for prop in object.props::<String, Value>() {
                let (key, value) = prop?;
                fields.insert(key, from_js_at(ctx, value, depth + 1)?.0);
            }
            Ok(Json(JsonValue::Object(fields)))
        }

        other => Err(Exception::throw_type(
            ctx,
            &format!("cannot convert {} to JSON", other.as_str()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};
    use serde_json::json;

    fn convert(source: &str) -> rquickjs::Result<JsonValue> {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let value: Value = ctx.eval(source)?;
            Ok(Json::from_js(&ctx, value)?.0)
        })
    }

    #[test]
    fn converts_nested_values() {
        let got = convert(r#"({ a: 1, b: [true, "x", null], c: { d: 2.5 } })"#).unwrap();
        assert_eq!(got, json!({ "a": 1, "b": [true, "x", null], "c": { "d": 2.5 } }));
    }

    #[test]
    fn integers_keep_precision() {
        let got = convert("({ n: 2147483647 })").unwrap();
        assert_eq!(got, json!({ "n": 2147483647 }));
    }

    #[test]
    fn undefined_becomes_null() {
        assert_eq!(convert("undefined").unwrap(), JsonValue::Null);
        assert_eq!(convert("({ a: undefined }).a").unwrap(), JsonValue::Null);
    }

    #[test]
    fn empty_containers_survive() {
        assert_eq!(convert("([])").unwrap(), json!([]));
        assert_eq!(convert("({})").unwrap(), json!({}));
    }

    #[test]
    fn cycles_are_rejected_not_hung() {
        let err = convert("(() => { const a = {}; a.self = a; return a; })()").unwrap_err();
        assert!(matches!(err, rquickjs::Error::Exception));
    }

    #[test]
    fn non_finite_numbers_are_rejected() {
        assert!(convert("({ x: 0 / 0 })").is_err());
        assert!(convert("({ x: 1 / 0 })").is_err());
    }

    #[test]
    fn functions_are_rejected() {
        assert!(convert("(() => 1)").is_err());
        assert!(convert("({ cb: () => 1 })").is_err());
    }
}
