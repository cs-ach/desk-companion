use std::sync::Arc;

use rquickjs::{Ctx, Exception, Function, Value};

use crate::logger::Logger;

pub fn log<'js>(ctx: &Ctx<'js>, logger: Arc<Logger>) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, level: String, message: String| -> rquickjs::Result<()> {
            if !matches!((&level.to_lowercase()).as_str(), "debug" | "info" | "warn" | "error") {
                return Err(Exception::throw_message(
                    &ctx,
                    &format!("invalid log level: {level}"),
                ));
            }

            logger.log(&level, &message);
            Ok(())
        },
    )?;

    Ok(function.into_value())
}
