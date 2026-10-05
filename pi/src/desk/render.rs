use jsrt::Json;
use jsrt::rquickjs::{self, Ctx, Function, Value, function::Async};
use plugin::Plugin;

use crate::link::RenderSender;

pub fn render<'js>(
    ctx: &Ctx<'js>,
    plugin: &Plugin,
    render_tx: RenderSender,
) -> rquickjs::Result<Value<'js>> {
    let module_id = plugin.manifest.id.clone();

    let function = Function::new(
        ctx.clone(),
        Async(move |_ctx: Ctx<'js>, payload: Json| {
            let module_id = module_id.clone();
            let render_tx = render_tx.clone();

            async move {
                let _ = render_tx.send((module_id, payload.0));
                Ok::<_, rquickjs::Error>(())
            }
        }),
    )?;

    Ok(function.into_value())
}
