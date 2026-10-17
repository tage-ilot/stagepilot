//! Smoke test for `midir`: compile + list MIDI input/output ports. Never sends MIDI.
//! Exit 0 even when there are no ports (CI runners have none); non-zero only if the
//! backend cannot be initialised.

use midir::{MidiInput, MidiOutput};

fn main() {
    let input = match MidiInput::new("stagepilot-spike-in") {
        Ok(i) => i,
        Err(e) => {
            eprintln!("midir input init failed: {e}");
            std::process::exit(1);
        }
    };
    let output = match MidiOutput::new("stagepilot-spike-out") {
        Ok(o) => o,
        Err(e) => {
            eprintln!("midir output init failed: {e}");
            std::process::exit(1);
        }
    };
    let ins = input.ports();
    let outs = output.ports();
    println!(
        "midir backend ok: {} input port(s), {} output port(s)",
        ins.len(),
        outs.len()
    );
    for p in &ins {
        println!(
            "  in : {}",
            input.port_name(p).unwrap_or_else(|_| "?".into())
        );
    }
    for p in &outs {
        println!(
            "  out: {}",
            output.port_name(p).unwrap_or_else(|_| "?".into())
        );
    }
}
