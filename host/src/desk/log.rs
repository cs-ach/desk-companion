use jsrt::rquickjs::{self, Ctx, Function, Value};
use plugin::Plugin;

pub fn log<'js>(ctx: &Ctx<'js>, plugin: &Plugin) -> rquickjs::Result<Value<'js>> {
    let id = plugin.manifest.id.clone();
    let function = Function::new(ctx.clone(), move |level: String, message: String| {
        println!("[{id}] {level}: {message}");
    })?;
    Ok(function.into_value())
}
