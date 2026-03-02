pub mod command_executor;
pub mod status_engine;

use crate::config::Config;
use crate::model::Task;
use crate::plugins::PluginProvider;
use crate::terminal::Terminal;
use crate::transport::{CoreCommand, CoreEvent, CoreResponse};

use self::status_engine::StatusEngine;

pub struct WagnerCore {
    pub status_engine: StatusEngine,
    pub config: Config,
    plugins: Vec<Box<dyn PluginProvider>>,
}

impl WagnerCore {
    pub fn new(config: Config) -> Self {
        let status_engine = StatusEngine::new(&config.monitor);
        Self {
            status_engine,
            config,
            plugins: Vec::new(),
        }
    }

    pub fn register_plugin(&mut self, provider: Box<dyn PluginProvider>) {
        self.plugins.push(provider);
    }

    pub fn plugin(&self, id: &str) -> Option<&dyn PluginProvider> {
        self.plugins.iter().find(|p| p.id() == id).map(|p| &**p)
    }

    pub fn tick(&mut self, terminal: &dyn Terminal, tasks: &[Task]) -> Vec<CoreEvent> {
        self.status_engine.poll_transitions(terminal, tasks)
    }

    /// Returns CoreResponse without adapter-specific buttons.
    pub fn execute(
        &self,
        terminal: &dyn Terminal,
        store: &crate::store::Store,
        cmd: &CoreCommand,
        tasks: &[Task],
    ) -> CoreResponse {
        command_executor::execute(
            terminal,
            store,
            &self.status_engine,
            &self.config,
            &self.plugins,
            cmd,
            tasks,
        )
    }
}
