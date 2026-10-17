//! Benchmark client. Usage: bench <port>. Prints one JSON line.
//! Scenario: connect, wait for 200 heartbeats, then 50 commands (alternating
//! waveformSeek-to-end / transportReturnToStart, the two the Python client can send),
//! measuring command -> status-change latency.
use stagepilot_playback::client::{Command, ConnectionOptions, PlaybackClient};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn wall_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.0
}

fn rss_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args().nth(1).unwrap().parse().unwrap();
    let opts = ConnectionOptions {
        port,
        host_override: Some("127.0.0.1".into()),
        ..Default::default()
    };
    let mut client = PlaybackClient::start(opts, None).unwrap();
    let connected_at = loop {
        if client.status().connected {
            break wall_ms();
        }
        tokio::time::sleep(Duration::from_micros(500)).await;
    };
    // Let the 200 scripted heartbeats arrive (fake sends them right after connect).
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let steady_rss = rss_kb("VmRSS:");
    let mut lat = Vec::new();
    for i in 0..50 {
        let before = client.status().heartbeat.unwrap().position;
        let cmd = if i % 2 == 0 {
            Command::SeekEnd
        } else {
            Command::ReturnToStart
        };
        let t0 = Instant::now();
        client.send(cmd).await.unwrap();
        loop {
            let p = client.status().heartbeat.unwrap().position;
            if p != before {
                break;
            }
            if t0.elapsed() > Duration::from_secs(3) {
                panic!("no state change")
            }
            tokio::time::sleep(Duration::from_micros(200)).await;
        }
        lat.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{}",
        serde_json::json!({
            "connected_at_ms": connected_at,
            "rss_kb": steady_rss,
            "peak_rss_kb": rss_kb("VmHWM:"),
            "lat_ms": lat,
        })
    );
    client.stop().await;
}
