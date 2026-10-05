use std::sync::Arc;

use jsrt::rquickjs::{self, Ctx, Exception, Function, Value, function::Async};
use plugin::{ConfigStore, Manifest, Secrets};
use serde_json::{Map, Value as Json};

pub fn set_secret<'js>(
    ctx: &Ctx<'js>,
    manifest: Arc<Manifest>,
    store: Arc<ConfigStore>,
    secrets: Secrets,
) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(
        ctx.clone(),
        Async(move |ctx: Ctx<'js>, key: String, value: String| {
            let manifest = manifest.clone();
            let store = store.clone();
            let secrets = secrets.clone();

            async move {
                let declared = manifest
                    .config
                    .iter()
                    .any(|field| field.secret && field.key == key);

                if !declared {
                    return Err(Exception::throw_message(
                        &ctx,
                        &format!(
                            "{key} is not declared as a secret config field of {}",
                            manifest.id
                        ),
                    ));
                }

                let mut values = Map::new();
                for existing in secrets.keys() {
                    if let Some(stored) = secrets.get(existing) {
                        values.insert(existing.to_owned(), Json::String(stored.to_owned()));
                    }
                }
                values.insert(key.clone(), Json::String(value));

                store.save_secrets(&manifest, &values).map_err(|e| {
                    Exception::throw_message(&ctx, &format!("saving {key}: {e}"))
                })?;

                Ok::<_, rquickjs::Error>(())
            }
        }),
    )?;

    Ok(function.into_value())
}
