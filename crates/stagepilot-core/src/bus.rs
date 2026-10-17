//! Failure-isolated, typed in-process event bus: port of `core/event_bus.py`.
//!
//! * Every subscriber runs in its own tokio task, concurrently with the others, so a slow,
//!   panicking or failing subscriber cannot block or crash another one. A handler that outlives
//!   `handler_timeout` is reported as a failure and abandoned.
//! * `publish` returns once every handler has finished (Python `gather` semantics), with a
//!   [`PublishReport`] instead of raising.
//! * Passive observers (`observe`) get a bounded broadcast stream; an observer that falls behind
//!   loses the oldest events (it is told how many) and never slows the publisher.

use crate::events::{EventType, StagePilotEvent};
use futures_util::future::BoxFuture;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

pub type HandlerResult = Result<(), String>;
type Handler = Arc<dyn Fn(Arc<StagePilotEvent>) -> BoxFuture<'static, HandlerResult> + Send + Sync>;
type Tap = Arc<dyn Fn(&StagePilotEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subscription {
    pub id: Uuid,
    pub event_type: Option<EventType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriberFailure {
    pub subscription_id: Uuid,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct PublishReport {
    pub event: Arc<StagePilotEvent>,
    pub delivered: usize,
    pub failures: Vec<SubscriberFailure>,
}

struct Inner {
    subscribers: Mutex<Vec<(Subscription, Handler)>>,
    taps: Mutex<Vec<Tap>>,
    observers: broadcast::Sender<Arc<StagePilotEvent>>,
    handler_timeout: Duration,
}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

pub const DEFAULT_HANDLER_TIMEOUT: Duration = Duration::from_secs(30);
pub const OBSERVER_CAPACITY: usize = 256;

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_handler_timeout(DEFAULT_HANDLER_TIMEOUT)
    }

    pub fn with_handler_timeout(handler_timeout: Duration) -> Self {
        let (observers, _) = broadcast::channel(OBSERVER_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                subscribers: Mutex::new(Vec::new()),
                taps: Mutex::new(Vec::new()),
                observers,
                handler_timeout,
            }),
        }
    }

    /// Subscribe to one event type, or to all events when `event_type` is `None`.
    pub fn subscribe<F, Fut>(&self, event_type: Option<EventType>, handler: F) -> Subscription
    where
        F: Fn(Arc<StagePilotEvent>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = HandlerResult> + Send + 'static,
    {
        let subscription = Subscription {
            id: Uuid::new_v4(),
            event_type,
        };
        let handler: Handler = Arc::new(move |event| Box::pin(handler(event)));
        lock(&self.inner.subscribers).push((subscription, handler));
        subscription
    }

    /// Remove a subscription, returning whether it was still registered.
    pub fn unsubscribe(&self, subscription: &Subscription) -> bool {
        let mut subscribers = lock(&self.inner.subscribers);
        let before = subscribers.len();
        subscribers.retain(|(s, _)| s.id != subscription.id);
        subscribers.len() != before
    }

    /// A synchronous hook called, in publish order, before any handler runs. Used by the state
    /// service to keep "recent events" in true publication order.
    pub fn add_tap<F: Fn(&StagePilotEvent) + Send + Sync + 'static>(&self, tap: F) {
        lock(&self.inner.taps).push(Arc::new(tap));
    }

    /// A bounded stream of every published event for passive consumers.
    pub fn observe(&self) -> broadcast::Receiver<Arc<StagePilotEvent>> {
        self.inner.observers.subscribe()
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.inner.subscribers).len()
    }

    /// Deliver an event concurrently and report failures without raising them.
    pub async fn publish(&self, event: StagePilotEvent) -> PublishReport {
        let event = Arc::new(event);
        let taps: Vec<Tap> = lock(&self.inner.taps).clone();
        for tap in taps {
            tap(&event);
        }
        // No receivers is not an error.
        let _ = self.inner.observers.send(event.clone());

        let targets: Vec<(Uuid, Handler)> = lock(&self.inner.subscribers)
            .iter()
            .filter(|(s, _)| s.event_type.is_none() || s.event_type == Some(event.event_type))
            .map(|(s, h)| (s.id, h.clone()))
            .collect();

        let timeout = self.inner.handler_timeout;
        let tasks: Vec<_> = targets
            .iter()
            .map(|(id, handler)| {
                let handler = handler.clone();
                let event = event.clone();
                let id = *id;
                tokio::spawn(async move {
                    let outcome = tokio::time::timeout(timeout, handler(event)).await;
                    (id, outcome)
                })
            })
            .collect();

        let mut failures = Vec::new();
        for (task, (id, _)) in tasks.into_iter().zip(targets.iter()) {
            match task.await {
                Ok((_, Ok(Ok(())))) => {}
                Ok((_, Ok(Err(error)))) => {
                    tracing::warn!(subscription = %id, event_type = %event.event_type.as_str(), %error, "event_subscriber_failed");
                    failures.push(SubscriberFailure {
                        subscription_id: *id,
                        error,
                    });
                }
                Ok((_, Err(_elapsed))) => {
                    tracing::warn!(subscription = %id, event_type = %event.event_type.as_str(), "event_subscriber_timed_out");
                    failures.push(SubscriberFailure {
                        subscription_id: *id,
                        error: format!("subscriber timed out after {}s", timeout.as_secs_f32()),
                    });
                }
                Err(join_error) => {
                    tracing::warn!(subscription = %id, event_type = %event.event_type.as_str(), "event_subscriber_panicked");
                    failures.push(SubscriberFailure {
                        subscription_id: *id,
                        error: if join_error.is_panic() {
                            "subscriber panicked".to_string()
                        } else {
                            "subscriber was cancelled".to_string()
                        },
                    });
                }
            }
        }
        PublishReport {
            event,
            delivered: targets.len() - failures.len(),
            failures,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Convenience for tests and plugins: count of subscriptions per event type.
pub fn summarize(bus: &EventBus) -> HashMap<Option<EventType>, usize> {
    let mut out = HashMap::new();
    for (s, _) in lock(&bus.inner.subscribers).iter() {
        *out.entry(s.event_type).or_insert(0) += 1;
    }
    out
}
