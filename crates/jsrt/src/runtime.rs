use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs, sync::Arc, thread::JoinHandle, time::Duration};

use plugin::Plugin;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Result as JsResult, Value,
    function::IntoArgs, promise::MaybePromise,
};

use crate::error::{Error, Result};
use crate::logger::Logger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Host,
    Pi,
}

impl Entry {
    fn script(&self, plugin: &Plugin) -> Option<std::path::PathBuf> {
        match self {
            Entry::Host => plugin.host_js(),
            Entry::Pi => Some(plugin.pi_js()),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Entry::Host => "host.js",
            Entry::Pi => "pi.js",
        }
    }
}

pub struct RustFunction {
    pub name: &'static str,
    pub build: Box<dyn for<'js> Fn(&Ctx<'js>, &Plugin) -> JsResult<Value<'js>> + Send>,
}

const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(30);
const MIN_TICK_INTERVAL: Duration = Duration::from_secs(1);
const MAX_RETRY_ATTEMPTS: u16 = 5;

pub struct Sandbox {
    pub plugin: Arc<Plugin>,
    pub memory_limit: usize,
    pub logger: Arc<Logger>,
    pub functions: Vec<RustFunction>,
    pub entry: Entry,
    runtime: Option<AsyncRuntime>,
    context: Option<AsyncContext>,
    pub shutdown_handler: Arc<AtomicBool>,
}

impl Sandbox {

    pub fn new(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<RustFunction>, logger: Arc<Logger>, shutdown_handler: Arc<AtomicBool>, entry: Entry) -> Self {
        Self { plugin, memory_limit, functions, runtime: None, context: None, logger, shutdown_handler, entry }
    }

    pub async fn build(&mut self) -> Result<()> {
        let runtime = AsyncRuntime::new()?;
        let shutdown_trigger = self.shutdown_handler.clone();

        runtime.set_memory_limit(self.memory_limit).await;
        runtime.set_interrupt_handler(Some(Box::new(move || {
            shutdown_trigger.load(Ordering::Relaxed)
        }))).await;
    
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

    async fn load_entry(&self) -> Result<()> {
        let Some(script) = self.entry.script(&self.plugin) else {
            return Ok(());
        };

        let content = fs::read(&script).map_err(|source| Error::Io {
            path: script,
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

    pub fn shutdown(&self) {
        self.shutdown_handler.store(true, Ordering::Relaxed);
    }

    pub fn start_thread(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<RustFunction>, logger: Arc<Logger>, shutdown_handler: Arc<AtomicBool>, entry: Entry) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let mut tick_interval = DEFAULT_TICK_INTERVAL;

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
                let mut sandbox = Sandbox::new(plugin, memory_limit, functions, logger, shutdown_handler, entry);
                let mut interval = tokio::time::interval(tick_interval.max(MIN_TICK_INTERVAL));


                if let Err(e) = sandbox.build().await {
                    sandbox.logger.error(&format!("build failed: {e}"));
                    return;
                }

                if let Err(e) = sandbox.load_entry().await {
                    sandbox.logger.error(&format!("{} failed: {e}", entry.name()));
                    return;
                }

                if let Err(e) = sandbox.call_global("init", ()).await {
                    sandbox.logger.error(&format!("init failed: {e}"));
                    return;
                }

                let mut retry = 0;

                loop {
                    if sandbox.shutdown_handler.load(Ordering::Relaxed) {
                        break;
                    }

                    interval.tick().await;
                    match sandbox.call_global("tick", ()).await {
                        Ok(true) => retry = 0,
                        Ok(false) => {
                            sandbox.logger.warn("tick not defined");
                            break;
                        },
                        Err(e) => {
                            retry += 1;

                            if retry >= MAX_RETRY_ATTEMPTS {
                                sandbox.logger.error(&format!("tick failed after {retry} restart attempts! | {e} | Stopping..."));
                                break;
                            }

                            sandbox.logger.warn(&format!("tick failed: {e} (attempt {retry} of {MAX_RETRY_ATTEMPTS})"));

                            interval.reset();
                        }

                    }
                    interval.reset();
                }

                sandbox.run_until_idle().await;
                sandbox.logger.warn("Plugin stopped.");
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
