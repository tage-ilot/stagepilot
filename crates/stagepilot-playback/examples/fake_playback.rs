//! Standalone fake Playback for the benchmark. Usage: fake_playback <heartbeats> <interval_ms>
//! Prints `PORT <n>` on stdout, then sends <heartbeats> heartbeats on each new connection
//! (position advances 1 s per beat while playing) and answers commands like Playback.
use stagepilot_playback::testkit::FakePlayback;

#[tokio::main]
async fn main() {
    let beats: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let interval: u64 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let fake = FakePlayback::start().await;
    println!("PORT {}", fake.port);
    let mut seen = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let c = *fake.connections.lock().unwrap();
        if c != seen {
            seen = c;
            for _ in 0..beats {
                fake.beat();
                tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
            }
        }
        // Keep-alive: one heartbeat a second after the scripted burst.
        if seen > 0 {
            fake.beat();
            tokio::time::sleep(std::time::Duration::from_millis(990)).await;
        }
    }
}
