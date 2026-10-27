//! Checks the Remote connector heartbeat and POSTs a webhook when it is stale. Meant for a
//! systemd timer or cron job; exit 0 healthy, 1 stale or undeliverable, 2 bad arguments.
//! See docs/heartbeat-alerts.md.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use stagepilot_remote::health_alert::{run_cli, send_alert, WEBHOOK_ENV};
use stagepilot_remote::http::{HttpTransport, ReqwestTransport};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let webhook = std::env::var(WEBHOOK_ENV).ok();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let transport: Arc<dyn HttpTransport> = Arc::new(ReqwestTransport::new());
    let sender = |url: &str, payload: &serde_json::Value| send_alert(&transport, url, payload);
    let (line, code) = run_cli(&args, webhook.as_deref(), now, &sender);
    if code == 2 {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    std::process::exit(code);
}
