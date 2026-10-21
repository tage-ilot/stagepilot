//! Plugin lifecycle contract and the failure-isolated plugin manager: port of `core/plugin.py`.
//!
//! Python semantics kept: a failed start marks the plugin `error`, publishes `plugin.failed` and
//! never stops the others; `restart_failed` retries plugins whose first start failed; `add` /
//! `remove` / `replace` are serialised by one lifecycle lock; `health` merges a live probe with the
//! managed status and never lets one failing probe hide the others.
//!
//! Rust additions (the Python manager has no automatic restart): every start/stop/health call runs
//! in its **own task** with a timeout, so a plugin that panics or hangs is contained, and
//! [`PluginManager::supervise`] restarts a failed plugin with exponential backoff.

use crate::bus::EventBus;
use crate::events::{new_event, EventPayload, EventType};
use crate::state::{PluginHealth, PluginStatus};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    /// Initialise resources and event subscriptions.
    async fn start(&self) -> Result<(), String>;
    /// Release resources and event subscriptions.
    async fn stop(&self) -> Result<(), String>;
    /// The plugin's current health.
    async fn health(&self) -> Result<PluginHealth, String>;
}

#[derive(Debug, Clone, Copy)]
pub struct SupervisorConfig {
    pub start_timeout: Duration,
    pub stop_timeout: Duration,
    pub health_timeout: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            start_timeout: Duration::from_secs(30),
            stop_timeout: Duration::from_secs(10),
            health_timeout: Duration::from_secs(5),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
        }
    }
}

struct Entry {
    plugin: Arc<dyn Plugin>,
}

struct Inner {
    bus: EventBus,
    config: SupervisorConfig,
    plugins: Mutex<Vec<Entry>>,
    health: Mutex<HashMap<String, PluginHealth>>,
    lifecycle: tokio::sync::Mutex<()>,
    supervisors: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct PluginManager {
    inner: Arc<Inner>,
}

/// Run `fut` in its own task under a timeout. A panic or a hang becomes an `Err`.
async fn isolated<T, F>(timeout: Duration, fut: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let task = tokio::spawn(fut);
    let abort = task.abort_handle();
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(join)) if join.is_panic() => Err("plugin panicked".to_string()),
        Ok(Err(_)) => Err("plugin task was cancelled".to_string()),
        Err(_) => {
            abort.abort();
            Err(format!("plugin timed out after {}s", timeout.as_secs_f32()))
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl PluginManager {
    pub fn new(bus: EventBus) -> Self {
        Self::with_config(bus, SupervisorConfig::default())
    }

    pub fn with_config(bus: EventBus, config: SupervisorConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                config,
                plugins: Mutex::new(Vec::new()),
                health: Mutex::new(HashMap::new()),
                lifecycle: tokio::sync::Mutex::new(()),
                supervisors: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn has(&self, name: &str) -> bool {
        lock(&self.inner.plugins)
            .iter()
            .any(|e| e.plugin.name() == name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        lock(&self.inner.plugins)
            .iter()
            .find(|e| e.plugin.name() == name)
            .map(|e| e.plugin.clone())
    }

    fn all(&self) -> Vec<Arc<dyn Plugin>> {
        lock(&self.inner.plugins)
            .iter()
            .map(|e| e.plugin.clone())
            .collect()
    }

    pub fn register(&self, plugin: Arc<dyn Plugin>) -> Result<(), String> {
        let mut plugins = lock(&self.inner.plugins);
        if plugins.iter().any(|e| e.plugin.name() == plugin.name()) {
            return Err(format!("Plugin already registered: {}", plugin.name()));
        }
        lock(&self.inner.health).insert(
            plugin.name().to_string(),
            PluginHealth {
                name: plugin.name().to_string(),
                version: plugin.version().to_string(),
                status: PluginStatus::Stopped,
                last_error: None,
                last_activity_at: None,
            },
        );
        plugins.push(Entry { plugin });
        Ok(())
    }

    fn managed(&self, name: &str) -> Option<PluginHealth> {
        lock(&self.inner.health).get(name).cloned()
    }

    fn forget(&self, name: &str) {
        lock(&self.inner.plugins).retain(|e| e.plugin.name() != name);
        lock(&self.inner.health).remove(name);
    }

    /// Register and start a plugin while the application is running. A failed start never
    /// leaves the plugin registered.
    pub async fn add(&self, plugin: Arc<dyn Plugin>) -> Result<PluginHealth, String> {
        let _guard = self.inner.lifecycle.lock().await;
        self.add_locked(plugin).await
    }

    pub async fn remove(&self, name: &str) -> Option<PluginHealth> {
        let _guard = self.inner.lifecycle.lock().await;
        self.remove_locked(name).await
    }

    /// Swap the named plugin for `plugin` as one serialised change.
    pub async fn replace(
        &self,
        name: &str,
        plugin: Arc<dyn Plugin>,
    ) -> Result<PluginHealth, String> {
        let _guard = self.inner.lifecycle.lock().await;
        self.remove_locked(name).await;
        self.add_locked(plugin).await
    }

    async fn add_locked(&self, plugin: Arc<dyn Plugin>) -> Result<PluginHealth, String> {
        self.register(plugin.clone())?;
        self.start_plugin(&plugin).await;
        let health = self.managed(plugin.name()).ok_or("plugin vanished")?;
        if health.status == PluginStatus::Running {
            return Ok(health);
        }
        let failed = health;
        let p = plugin.clone();
        // Best-effort cleanup of a plugin that never started.
        let _ = isolated(
            self.inner.config.stop_timeout,
            async move { p.stop().await },
        )
        .await;
        self.forget(plugin.name());
        Ok(failed)
    }

    async fn remove_locked(&self, name: &str) -> Option<PluginHealth> {
        self.cancel_supervisor(name);
        let plugin = self.get(name)?;
        self.stop_plugin(&plugin).await;
        let result = self.managed(name);
        self.forget(name);
        result
    }

    /// Start every plugin, each in its own task, in registration order of results.
    pub async fn start_all(&self) {
        let plugins = self.all();
        let tasks: Vec<_> = plugins
            .into_iter()
            .map(|plugin| {
                let manager = self.clone();
                tokio::spawn(async move { manager.start_plugin(&plugin).await })
            })
            .collect();
        for task in tasks {
            let _ = task.await;
        }
    }

    pub async fn stop_all(&self) {
        self.cancel_all_supervisors();
        let mut plugins = self.all();
        plugins.reverse();
        let tasks: Vec<_> = plugins
            .into_iter()
            .map(|plugin| {
                let manager = self.clone();
                tokio::spawn(async move { manager.stop_plugin(&plugin).await })
            })
            .collect();
        for task in tasks {
            let _ = task.await;
        }
    }

    /// Retry plugins whose first startup attempt failed after dependencies settle.
    pub async fn restart_failed(&self) {
        let failed: Vec<_> = self
            .all()
            .into_iter()
            .filter(|p| {
                self.managed(p.name())
                    .is_some_and(|h| h.status == PluginStatus::Error)
            })
            .collect();
        for plugin in failed {
            self.stop_plugin(&plugin).await;
            if self
                .managed(plugin.name())
                .is_some_and(|h| h.status == PluginStatus::Stopped)
            {
                self.start_plugin(&plugin).await;
            }
        }
    }

    /// Start supervision of one plugin: while the plugin is registered and not running, retry its
    /// start with exponential backoff (`initial_backoff` doubling up to `max_backoff`). Other
    /// plugins are never touched. Stopped by `remove` / `stop_all`.
    pub fn supervise(&self, name: &str) {
        let Some(plugin) = self.get(name) else {
            return;
        };
        let manager = self.clone();
        let name = name.to_string();
        let key = name.clone();
        let handle = tokio::spawn(async move {
            let config = manager.inner.config;
            let mut backoff = config.initial_backoff;
            loop {
                let status = manager.managed(&name).map(|h| h.status);
                match status {
                    None => return,
                    Some(PluginStatus::Running) => {
                        backoff = config.initial_backoff;
                        tokio::time::sleep(config.initial_backoff).await;
                    }
                    Some(PluginStatus::Error) => {
                        tokio::time::sleep(backoff).await;
                        manager.stop_plugin(&plugin).await;
                        if manager
                            .managed(&name)
                            .is_some_and(|h| h.status == PluginStatus::Stopped)
                        {
                            manager.start_plugin(&plugin).await;
                        }
                        backoff = (backoff * 2).min(config.max_backoff);
                    }
                    Some(_) => tokio::time::sleep(config.initial_backoff).await,
                }
            }
        });
        if let Some(old) = lock(&self.inner.supervisors).insert(key, handle) {
            old.abort();
        }
    }

    fn cancel_supervisor(&self, name: &str) {
        if let Some(handle) = lock(&self.inner.supervisors).remove(name) {
            handle.abort();
        }
    }

    fn cancel_all_supervisors(&self) {
        for (_, handle) in lock(&self.inner.supervisors).drain() {
            handle.abort();
        }
    }

    /// Live health of every plugin, in registration order.
    pub async fn health(&self) -> Vec<PluginHealth> {
        let probes: Vec<_> = self
            .all()
            .into_iter()
            .map(|plugin| {
                let manager = self.clone();
                tokio::spawn(async move { manager.plugin_health(&plugin).await })
            })
            .collect();
        let mut out = Vec::new();
        for probe in probes {
            if let Ok(h) = probe.await {
                out.push(h);
            }
        }
        out
    }

    async fn plugin_health(&self, plugin: &Arc<dyn Plugin>) -> PluginHealth {
        let name = plugin.name().to_string();
        let managed = self.managed(&name).unwrap_or(PluginHealth {
            name: name.clone(),
            version: plugin.version().to_string(),
            status: PluginStatus::Stopped,
            last_error: None,
            last_activity_at: None,
        });
        if managed.status != PluginStatus::Running {
            return managed;
        }
        let p = plugin.clone();
        let reported = isolated(
            self.inner.config.health_timeout,
            async move { p.health().await },
        )
        .await;
        let current = self.managed(&name).unwrap_or_else(|| managed.clone());
        let reported = match reported {
            Ok(r) => r,
            Err(error) => {
                tracing::warn!(plugin = %name, %error, "plugin_health_failed");
                if current.status != PluginStatus::Running {
                    return current;
                }
                return PluginHealth {
                    name,
                    version: plugin.version().to_string(),
                    status: PluginStatus::Error,
                    last_error: Some("Plugin health check failed.".to_string()),
                    last_activity_at: current.last_activity_at,
                };
            }
        };
        if current.status != PluginStatus::Running {
            return current;
        }
        PluginHealth {
            name,
            version: plugin.version().to_string(),
            status: reported.status,
            last_error: reported.last_error,
            last_activity_at: reported.last_activity_at.or(current.last_activity_at),
        }
    }

    async fn start_plugin(&self, plugin: &Arc<dyn Plugin>) {
        self.set_health(plugin, PluginStatus::Starting, None).await;
        let p = plugin.clone();
        let result = isolated(
            self.inner.config.start_timeout,
            async move { p.start().await },
        )
        .await;
        if let Err(error) = result {
            tracing::warn!(plugin = %plugin.name(), %error, "plugin_start_failed");
            self.set_health(plugin, PluginStatus::Error, Some(error.clone()))
                .await;
            self.inner
                .bus
                .publish(new_event(
                    EventType::PluginFailed,
                    "plugin_manager",
                    EventPayload::Plugin {
                        name: plugin.name().to_string(),
                        version: plugin.version().to_string(),
                        status: PluginStatus::Error,
                        error: Some(error),
                    },
                ))
                .await;
            return;
        }
        self.set_health(plugin, PluginStatus::Running, None).await;
    }

    async fn stop_plugin(&self, plugin: &Arc<dyn Plugin>) {
        let Some(health) = self.managed(plugin.name()) else {
            return;
        };
        if health.status == PluginStatus::Stopped {
            return;
        }
        self.set_health(plugin, PluginStatus::Stopping, None).await;
        let p = plugin.clone();
        let result = isolated(
            self.inner.config.stop_timeout,
            async move { p.stop().await },
        )
        .await;
        match result {
            Ok(()) => self.set_health(plugin, PluginStatus::Stopped, None).await,
            Err(error) => {
                tracing::warn!(plugin = %plugin.name(), %error, "plugin_stop_failed");
                self.set_health(plugin, PluginStatus::Error, Some(error))
                    .await;
            }
        }
    }

    async fn set_health(
        &self,
        plugin: &Arc<dyn Plugin>,
        status: PluginStatus,
        error: Option<String>,
    ) {
        let health = PluginHealth {
            name: plugin.name().to_string(),
            version: plugin.version().to_string(),
            status,
            last_error: error.clone(),
            last_activity_at: Some(Utc::now()),
        };
        lock(&self.inner.health).insert(plugin.name().to_string(), health);
        self.inner
            .bus
            .publish(new_event(
                EventType::PluginStatusChanged,
                "plugin_manager",
                EventPayload::Plugin {
                    name: plugin.name().to_string(),
                    version: plugin.version().to_string(),
                    status,
                    error,
                },
            ))
            .await;
    }
}
