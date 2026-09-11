use std::{fs, sync::Arc, thread::JoinHandle, time::Duration};

use chrono::{Local, TimeDelta};
use plugin::Plugin;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Result as JsResult, Value,
    function::IntoArgs, promise::MaybePromise,
};

use crate::error::{Error, Result};
use crate::logger::Logger;

pub struct RustFunction {
    pub name: &'static str,
    pub build: Box<dyn for<'js> Fn(&Ctx<'js>, &Plugin) -> JsResult<Value<'js>> + Send>,
}

const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(30);
const MIN_TICK_INTERVAL: Duration = Duration::from_secs(1);
const KILL_AFTER_MINUTES: TimeDelta = TimeDelta::minutes(5);
const MAX_RETRY_ATTEMPTS: u16 = 5;

pub struct Sandbox {
    plugin: Arc<Plugin>,
    memory_limit: usize,
    functions: Vec<RustFunction>,
    runtime: Option<AsyncRuntime>,
    context: Option<AsyncContext>,
}

impl Sandbox {

    pub fn new(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<RustFunction>) -> Self {
        Self { plugin, memory_limit, functions, runtime: None, context: None}
    }

    pub async fn build(&mut self) -> Result<()> {
        let runtime = AsyncRuntime::new()?;
        runtime.set_memory_limit(self.memory_limit).await;
    
        let context = AsyncContext::full(&runtime).await?;

        context.async_with(async |ctx| {
            let globals = ctx.globals();
            for function in &self.functions {
                let js_value = (function.build)(&ctx, &self.plugin)?;
                globals.set(function.name, js_value)?;
            }
            Ok::<(), rquickjs::Error>(())
        }).await?;

        self.runtime = Some(runtime);
        self.context = Some(context);

        Ok(())
    }

    async fn load_host(&self) -> Result<()> {
        let Some(host_js) = self.plugin.host_js() else {
            return Ok(());
        };

        let content = fs::read(&host_js).map_err(|source| Error::Io {
            path: host_js,
            source,
        })?;

        let context = self.context.as_ref().ok_or(rquickjs::Error::Unknown)?;
        context
            .async_with(async move |ctx| {
                ctx.eval::<(), _>(content)
                    .catch(&ctx)
                    .map_err(Error::from_caught)
            })
            .await
    }

    async fn call_global<A>(&self, name: &str, args: A) -> Result<bool>
    where
        A: for<'js> IntoArgs<'js>,
    {
        let context = self.context.as_ref().ok_or(rquickjs::Error::Unknown)?;
        context
            .async_with(async move |ctx| {
                let outcome = async {
                    let Some(func) = ctx.globals().get::<_, Option<Function>>(name)? else {
                        return Ok(false);
                    };

                    func.call::<_, MaybePromise>(args)?
                        .into_future::<()>()
                        .await?;

                    Ok::<bool, rquickjs::Error>(true)
                }
                .await;

                outcome.catch(&ctx).map_err(Error::from_caught)
            })
            .await
    }

    pub fn start_thread(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<RustFunction>, logger: Arc<Logger>,) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let mut tick_interval = DEFAULT_TICK_INTERVAL;
            let mut last_retry_at = Local::now();

            if let Some(plugin_interval) = plugin.manifest.tick_interval_ms {
                tick_interval = Duration::from_millis(plugin_interval.into());
            }
            

            let tokio_rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    logger.error(&format!("could not start async runtime: {e}"));
                    return;
                }
            };

            tokio_rt.block_on(async move {
                let mut sandbox = Sandbox::new(plugin, memory_limit, functions);
                let mut interval = tokio::time::interval(tick_interval.max(MIN_TICK_INTERVAL));


                if let Err(e) = sandbox.build().await {
                    logger.error(&format!("build failed: {e}"));
                    return;
                }

                if let Err(e) = sandbox.load_host().await {
                    logger.error(&format!("host.js failed: {e}"));
                    return;
                }

                if let Err(e) = sandbox.call_global("init", ()).await {
                    logger.error(&format!("init failed: {e}"));
                    return;
                }

                let mut retry = 0;

                loop {
                    interval.tick().await;
                    match sandbox.call_global("tick", ()).await {
                        Ok(true) => {},
                        Ok(false) => {
                            logger.warn("tick not defined");
                            break;
                        },
                        Err(e) => {
                            retry += 1;
                            if (Local::now() - last_retry_at) <= KILL_AFTER_MINUTES {
                                if retry > MAX_RETRY_ATTEMPTS {
                                    logger.error(&format!("max retries reached: {e}"));
                                    break;
                                }
                            }else{
                                retry = 0;
                            }

                            last_retry_at = Local::now();
                            logger.warn(&format!("tick failed: {e} (attempt {retry} of {MAX_RETRY_ATTEMPTS})"));
                            interval.reset();
                        }

                    }
                    interval.reset();
                }

                sandbox.run_until_idle().await;
            });
        })
    }

    async fn run_until_idle(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.idle().await
        }else{
            return
        }
    }

}
