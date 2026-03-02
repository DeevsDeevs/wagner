use tracing::info;

use crate::core::WagnerCore;
use crate::model::Task;
use crate::store::Store;
use crate::terminal::Terminal;

use super::CoreEvent;

#[allow(async_fn_in_trait)]
pub trait Adapter: Send {
    fn name(&self) -> &str;
    async fn handle_events(
        &mut self,
        events: &[CoreEvent],
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> crate::Result<()>;
    async fn poll_and_handle(
        &mut self,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> crate::Result<()>;
}

pub struct LogAdapter;

impl Adapter for LogAdapter {
    fn name(&self) -> &str {
        "log"
    }

    async fn handle_events(
        &mut self,
        events: &[CoreEvent],
        _core: &WagnerCore,
        _terminal: &dyn Terminal,
        _store: &Store,
        _tasks: &[Task],
    ) -> crate::Result<()> {
        for event in events {
            info!(?event, "event");
        }
        Ok(())
    }

    async fn poll_and_handle(
        &mut self,
        _core: &WagnerCore,
        _terminal: &dyn Terminal,
        _store: &Store,
        _tasks: &[Task],
    ) -> crate::Result<()> {
        Ok(())
    }
}

pub enum DaemonAdapter {
    Log(LogAdapter),
    #[cfg(feature = "telegram")]
    Telegram(Box<super::telegram::TelegramAdapter>),
}

impl Adapter for DaemonAdapter {
    fn name(&self) -> &str {
        match self {
            Self::Log(a) => a.name(),
            #[cfg(feature = "telegram")]
            Self::Telegram(a) => a.name(),
        }
    }

    async fn handle_events(
        &mut self,
        events: &[CoreEvent],
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> crate::Result<()> {
        match self {
            Self::Log(a) => a.handle_events(events, core, terminal, store, tasks).await,
            #[cfg(feature = "telegram")]
            Self::Telegram(a) => a.handle_events(events, core, terminal, store, tasks).await,
        }
    }

    async fn poll_and_handle(
        &mut self,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> crate::Result<()> {
        match self {
            Self::Log(a) => a.poll_and_handle(core, terminal, store, tasks).await,
            #[cfg(feature = "telegram")]
            Self::Telegram(a) => a.poll_and_handle(core, terminal, store, tasks).await,
        }
    }
}
