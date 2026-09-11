use std::{fs, sync::Arc, thread::{JoinHandle}};

use plugin::Plugin;
use rquickjs::{AsyncContext, AsyncRuntime, Ctx, Result, Value};

pub struct HostFunction {
    pub name: &'static str,
    pub build: Box<dyn for<'js> Fn(&Ctx<'js>, &Plugin) -> Result<Value<'js>> + Send>,
}

pub struct Sandbox {
    plugin: Arc<Plugin>,
    memory_limit: usize,
    functions: Vec<HostFunction>,
    runtime: Option<AsyncRuntime>,
    context: Option<AsyncContext>,
}

impl Sandbox {

    pub fn new(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<HostFunction>) -> Self {
        Self { plugin, memory_limit, functions, runtime: None, context: None }
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

    async fn start_host(&self) -> Result<()> {
        let Some(host_js) = self.plugin.host_js() else {
            return Ok(());
        };

        let content = fs::read(host_js)?;
        let context = self.context.as_ref().ok_or(rquickjs::Error::Unknown)?;
        context.async_with(async |ctx| {
            ctx.eval::<(), _>(content)?;
            ctx.eval::<(), _>("main();")?;
            Ok::<(), rquickjs::Error>(())
        }).await?;

        Ok(())
    }

    pub fn start_thread(plugin: Arc<Plugin>, memory_limit: usize, functions: Vec<HostFunction>,) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let id = plugin.manifest.id.clone();

            let tokio_rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("[{id}] could not start async runtime: {e}");
                    return;
                }
            };

            tokio_rt.block_on(async move {
                let mut sandbox = Sandbox::new(plugin, memory_limit, functions);

                if let Err(e) = sandbox.build().await {
                    eprintln!("[{id}] build failed: {e}");
                    return;
                }

                if let Err(e) = sandbox.start_host().await {
                    eprintln!("[{id}] host.js failed: {e}");
                    return;
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
