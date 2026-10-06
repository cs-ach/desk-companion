use plugin::{Plugin};
use jsrt::Json;
use jsrt::rquickjs::{self, Ctx, Function, Value, function::{Async, Opt}};

use crate::{EventSender};

pub fn announce<'js>(
    ctx: &Ctx<'js>,
    plugin: &Plugin,
    event_tx: EventSender
) -> rquickjs::Result<Value<'js>> {
    let module_id = plugin.manifest.id.clone();

    let function = Function::new(ctx.clone(), Async(move |_: Ctx<'js>, event: String, payload: Opt<Json>| {
        let module_id = module_id.clone();
        let event_tx = event_tx.clone();

        async move {
            let json_payload = payload.0.unwrap_or_default();
            let _ = event_tx.send((module_id, event, json_payload.0));

            Ok::<_, rquickjs::Error>(())
        }
    }))?;
    Ok(function.into_value())
}