use plugin::Plugin;
use jsrt::Json;
use jsrt::rquickjs::{self, Ctx, Function, Value, function::{Async, Opt}};

use crate::link::ActionSender;

pub fn send_action<'js>(
    ctx: &Ctx<'js>,
    plugin: &Plugin,
    action_tx: ActionSender,
) -> rquickjs::Result<Value<'js>> {
    let module_id = plugin.manifest.id.clone();

    let function = Function::new(ctx.clone(), Async(move |ctx: Ctx<'js>, name: String, payload: Opt<Json>| {
        let module_id = module_id.clone();
        let action_tx = action_tx.clone();

        async move {
            let payload = payload.0.map_or(serde_json::Value::Null, |json| json.0);
            let _ = action_tx.send((module_id, name, payload));

            Ok::<_, rquickjs::Error>(())
        }
    }))?;
    Ok(function.into_value())
}
