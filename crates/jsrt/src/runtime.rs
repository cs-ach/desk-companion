use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs, sync::Arc, thread::JoinHandle, time::Duration};

use plugin::Plugin;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Result as JsResult, Value,
    function::IntoArgs, promise::MaybePromise,
};

use crate::error::{Error, Result};
use crate::json::Json;
use crate::logger::Logger;

pub type Call = (String, serde_json::Value);
pub type CallSender = tokio::sync::mpsc::UnboundedSender<Call>;
pub type CallReceiver = tokio::sync::mpsc::UnboundedReceiver<Call>;

pub struct RunningPlugin {
    shutdown: Arc<AtomicBool>,
    calls: CallSender,
    handle: JoinHandle<()>,
}

impl RunningPlugin {
    pub fn new(shutdown: Arc<AtomicBool>, calls: CallSender, handle: JoinHandle<()>) -> Self {
        Self { shutdown, calls, handle }
    }

    pub fn signal(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn join(self, id: &str) {
        if self.handle.join().is_err() {
            eprintln!("plugin {id} panicked");
        }
    }

    pub fn stop(self, id: &str) {
        self.signal();
        self.join(id);
    }

    pub fn call(&self, function: String, payload: serde_json::Value) -> bool {
        self.calls.send((function, payload)).is_ok()
    }
}

#[derive(Default)]
pub struct Running {
    plugins: HashMap<String, RunningPlugin>,
}

impl Running {
    pub fn insert(&mut self, id: String, plugin: RunningPlugin) {
        self.plugins.insert(id, plugin);
    }

    pub fn contains(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.plugins.keys()
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(running) = self.plugins.remove(id) {
            running.stop(id);
        }
    }

    pub fn get_plugin(&self, module_id: &str) -> Option<&RunningPlugin> {
        self.plugins.get(module_id)
    }



    pub fn stop_all(self) {
        for running in self.plugins.values() {
            running.signal();
        }
        for (id, running) in self.plugins {
            running.join(&id);
        }
    }
}

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
const SHUTDOWN_POLL: Duration = Duration::from_millis(200);
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

    async fn has_global(&self, name: &str) -> Result<bool> {
        let context = self.context.as_ref().ok_or(rquickjs::Error::Unknown)?;
        context
            .async_with(async move |ctx| {
                let found = ctx.globals().get::<_, Option<Function>>(name)?.is_some();
                Ok::<bool, rquickjs::Error>(found)
                    .catch(&ctx)
                    .map_err(Error::from_caught)
            })
            .await
    }

    pub fn shutdown(&self) {
        self.shutdown_handler.store(true, Ordering::Relaxed);
    }

    pub fn start_thread(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<RustFunction>, logger: Arc<Logger>, shutdown_handler: Arc<AtomicBool>, entry: Entry) -> (CallSender, JoinHandle<()>) {
        let (calls, mut call_rx): (CallSender, CallReceiver) = tokio::sync::mpsc::unbounded_channel();

        let handle = std::thread::spawn(move || {
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
                let mut ticking = sandbox.has_global("tick").await.unwrap_or(false);

                loop {
                    if sandbox.shutdown_handler.load(Ordering::Relaxed) {
                        break;
                    }

                    tokio::select! {
                        Some((function, payload)) = call_rx.recv() => {
                            match sandbox.call_global(&function, (Json(payload),)).await {
                                Ok(true) => {}
                                Ok(false) => sandbox.logger.warn(&format!("{function} not defined")),
                                Err(e) => sandbox.logger.error(&format!("{function} failed: {e}")),
                            }
                        }

                        _ = interval.tick(), if ticking => {
                            match sandbox.call_global("tick", ()).await {
                                Ok(true) => retry = 0,
                                Ok(false) => ticking = false,
                                Err(e) => {
                                    retry += 1;

                                    if retry >= MAX_RETRY_ATTEMPTS {
                                        sandbox.logger.error(&format!("tick failed after {retry} restart attempts! | {e} | Stopping..."));
                                        break;
                                    }

                                    sandbox.logger.warn(&format!("tick failed: {e} (attempt {retry} of {MAX_RETRY_ATTEMPTS})"));
                                }
                            }
                            interval.reset();
                        }

                        _ = tokio::time::sleep(SHUTDOWN_POLL) => {}

                        else => break,
                    }
                }

                sandbox.run_until_idle().await;
                sandbox.logger.warn("Plugin stopped.");
            });
        });

        (calls, handle)
    }

    async fn run_until_idle(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.idle().await
        }else{
            return
        }
    }

}
