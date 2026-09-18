use jsrt::rquickjs::{FromJs, IntoJs};
use plugin::Plugin;
use jsrt::Json;
use jsrt::rquickjs::{self, Ctx, Function, Value, function::{Async, Opt}};

use crate::link::StateSender;

pub fn send_state<'js>(
    ctx: &Ctx<'js>,
    plugin: &Plugin,
    state_tx: StateSender,
) -> rquickjs::Result<Value<'js>> {
    let module_id = plugin.manifest.id.clone();

    let function = Function::new(ctx.clone(), Async(move |ctx: Ctx<'js>, state: Opt<Json>| {
        let module_id = module_id.clone();
        let state_tx = state_tx.clone();

        async move {
            let Some(json_state) = state.0 else {
                return Ok(());
            };

            let _ = state_tx.send((module_id, json_state.0));

            Ok::<_, rquickjs::Error>(())
        }
    }))?;
    Ok(function.into_value())
}