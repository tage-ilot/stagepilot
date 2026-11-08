//! The starting look of a capture: what every fixture looked like when the recording began.
//!
//! The recorder only sees fixture CHANGES. A house light that the operator sets with a Lightkey
//! preset before the song and that does not move during it never changes, so it produced no
//! sample and a replay could not recreate the picture. The relay's own state of every fixture
//! (the recorder's `baseline`) is taken as the song's first look, at position 0.
//!
//! # Which properties are stored (one generic rule, from the property names)
//!
//! A property name (the part after `/overrides/`, compared without case) belongs to one class:
//!
//! * **level**: names containing `dimmer`, `intensity`, `fog` or `haze` (not `speed`/`rate`);
//! * **colour**: `color`/`colour`, anything containing `white`, and `amber`, `uv`, `lime`, `red`,
//!   `green`, `blue`, `cyan`, `magenta`, `yellow`, `indigo`;
//! * **position** (movers): names starting with `pan`, `tilt`, `zoom` or `focus` (not `speed`);
//! * **wheel**: names starting with `gobo`, `prism`, `frost`, `iris`, `wheel` or `colorN` /
//!   `colorwheel` (not `rotation`, `speed`, `mode`, `angle`). A wheel is stored only when it is
//!   *in use*: its value is not zero, `false`, empty, or the word `open`/`none`/`off`.
//!
//! Everything else (`lit`, `off`, `command`, speeds, programs, macros, built-in effects) is not
//! an appearance property and is not stored.
//!
//! A fixture that **never changes in the song and is dark at the start** keeps only what keeps it
//! dark: its levels, or, when it has no level, its colour. *Dark* means every level is 0, or, with
//! no level at all, every colour is black. Every other fixture is stored with all its appearance
//! properties (a fixture that does change keeps, for the properties that change, the start value
//! the recorder already used).

use crate::lightkey_fixtures::{FixtureArg, FixtureCommand};
use std::collections::{BTreeSet, HashSet};

/// A value this close to 0 counts as 0 (DMX resolution is 1/255).
const ZERO: f64 = 1e-4;

/// The class of one property of the starting look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Level,
    Colour,
    Position,
    Wheel,
}

/// The property of an address (`/fixture/<name>/overrides/<property>`).
fn property_of(address: &str) -> &str {
    address.rsplit('/').next().unwrap_or("")
}

/// The class of a property name, or `None` for a property that is not part of the look.
pub fn classify(property: &str) -> Option<Class> {
    let p = property.to_lowercase();
    if p.contains("speed") || p.ends_with("rate") {
        return None;
    }
    if ["pan", "tilt", "zoom", "focus"]
        .iter()
        .any(|s| p.starts_with(s))
    {
        return Some(Class::Position);
    }
    if ["dimmer", "intensity", "fog", "haze"]
        .iter()
        .any(|s| p.contains(s))
    {
        return Some(Class::Level);
    }
    let numbered_colour = p
        .strip_prefix("color")
        .or_else(|| p.strip_prefix("colour"))
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    let wheel_name = ["gobo", "prism", "frost", "iris", "wheel", "colorwheel"]
        .iter()
        .any(|s| p.starts_with(s));
    if (wheel_name || numbered_colour)
        && !["rotation", "mode", "angle"].iter().any(|s| p.contains(s))
    {
        return Some(Class::Wheel);
    }
    if matches!(
        p.as_str(),
        "color"
            | "colour"
            | "amber"
            | "uv"
            | "lime"
            | "red"
            | "green"
            | "blue"
            | "cyan"
            | "magenta"
            | "yellow"
            | "indigo"
    ) || p.contains("white")
    {
        return Some(Class::Colour);
    }
    None
}

fn number(arg: &FixtureArg) -> Option<f64> {
    match arg {
        FixtureArg::Float(v) => Some(*v),
        FixtureArg::Int(v) => Some(*v as f64),
        FixtureArg::Bool(v) => Some(f64::from(u8::from(*v))),
        FixtureArg::Text(_) => None,
    }
}

fn all_zero(command: &FixtureCommand) -> bool {
    !command.v.is_empty()
        && command.v.iter().all(|a| match a {
            FixtureArg::Text(t) => {
                let t = t.trim().to_lowercase();
                t.is_empty() || matches!(t.as_str(), "open" | "none" | "off")
            }
            other => number(other).is_some_and(|v| v.abs() <= ZERO),
        })
}

/// A wheel (gobo, prism, frost, colour wheel) that is doing something.
fn wheel_in_use(command: &FixtureCommand) -> bool {
    !command.v.is_empty() && !all_zero(command)
}

/// What [`start_look`] decided.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StartLook {
    /// The values to store at the start, in the state's order: for every fixture that never
    /// changes, its stored appearance properties (all of them, or only what keeps it dark).
    pub commands: Vec<FixtureCommand>,
    /// Fixtures that are static and dark and so were reduced.
    pub dark_fixtures: usize,
    /// Fixtures in the state.
    pub fixtures: usize,
}

/// The values to store at the start for every fixture whose addresses are NOT in `changing`.
/// `state` is the relay's state when the recording began.
pub fn start_look(state: &[FixtureCommand], changing: &HashSet<&str>) -> StartLook {
    let mut fixtures: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for c in state {
        if let Some(f) = c.fixture() {
            if seen.insert(f) {
                fixtures.push(f);
            }
        }
    }
    let changing_fixtures: BTreeSet<&str> = state
        .iter()
        .filter(|c| changing.contains(c.a.as_str()))
        .filter_map(FixtureCommand::fixture)
        .collect();
    let mut out = StartLook {
        fixtures: fixtures.len(),
        ..StartLook::default()
    };
    for fixture in fixtures {
        let mine: Vec<(&FixtureCommand, Option<Class>)> = state
            .iter()
            .filter(|c| c.fixture() == Some(fixture))
            .map(|c| (c, classify(property_of(&c.a))))
            .collect();
        let levels: Vec<&FixtureCommand> = mine
            .iter()
            .filter(|(_, k)| *k == Some(Class::Level))
            .map(|(c, _)| *c)
            .collect();
        let colours: Vec<&FixtureCommand> = mine
            .iter()
            .filter(|(_, k)| *k == Some(Class::Colour))
            .map(|(c, _)| *c)
            .collect();
        let dark = if !levels.is_empty() {
            levels.iter().all(|c| all_zero(c))
        } else {
            !colours.is_empty() && colours.iter().all(|c| all_zero(c))
        };
        let static_and_dark = dark && !changing_fixtures.contains(fixture);
        if static_and_dark {
            out.dark_fixtures += 1;
        }
        for (command, kind) in mine {
            if changing.contains(command.a.as_str()) {
                continue; // recorded with its changes; the recorder starts it at this value
            }
            let keep = match kind {
                None => false,
                Some(Class::Level) => true,
                Some(Class::Colour) => !static_and_dark || levels.is_empty(),
                Some(Class::Position) => !static_and_dark,
                Some(Class::Wheel) => !static_and_dark && wheel_in_use(command),
            };
            if keep {
                out.commands.push(command.clone());
            }
        }
    }
    out
}

/// What [`apply`] put into the changes of a pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// Song position the starting look is stamped at: 0 normally, later when the recorder began
    /// late.
    pub at: f64,
    /// Fixtures in the relay's state when recording began.
    pub fixtures: usize,
    /// Static fixtures that were dark and so reduced to what keeps them dark.
    pub dark_fixtures: usize,
    /// Values stored for fixtures that never change (the rest are the first values of the
    /// fixtures that do).
    pub static_values: usize,
}

/// Put the starting look into the pass: every fixture that never changes after `at` gets its
/// stored properties (see the module docs) as a change at `at`, and anything the recorder
/// already stamped at the start for such a fixture is replaced by that. `snapshot` is the relay's
/// state when recording began. Fixtures that do change keep their start value through the fit's
/// baseline, as before.
pub fn apply(
    changes: &mut Vec<(f64, FixtureCommand)>,
    at: f64,
    snapshot: &[FixtureCommand],
) -> Applied {
    let later: HashSet<&str> = changes
        .iter()
        .filter(|(t, _)| *t > at + 1e-6)
        .map(|(_, c)| c.a.as_str())
        .collect();
    let look = start_look(snapshot, &later);
    let static_addresses: HashSet<String> = snapshot
        .iter()
        .filter(|c| !later.contains(c.a.as_str()))
        .map(|c| c.a.clone())
        .collect();
    changes.retain(|(t, c)| *t > at + 1e-6 || !static_addresses.contains(&c.a));
    let mut with_start: Vec<(f64, FixtureCommand)> =
        look.commands.iter().map(|c| (at, c.clone())).collect();
    let static_values = with_start.len();
    with_start.append(changes);
    *changes = with_start;
    Applied {
        at,
        fixtures: look.fixtures,
        dark_fixtures: look.dark_fixtures,
        static_values,
    }
}

/// The first look as ONE row: the fit makes a row per fixture (rows that are exactly alike share
/// one), so the starting look of a rig would be dozens of rows at the same time. Rows at the
/// first time that hold steps (no fade) are joined, in order, into the first of them. The replay
/// sends the same messages; the timeline shows one "Look - N fixtures" instead of dozens.
pub fn merge_first_look(rows: &mut Vec<crate::lightkey_smooth::SmoothLook>, at: f64) {
    let first = rows
        .iter()
        .map(|r| r.at_seconds)
        .filter(|t| *t <= at + 0.05)
        .fold(f64::INFINITY, f64::min);
    if !first.is_finite() {
        return;
    }
    let same = |r: &crate::lightkey_smooth::SmoothLook| {
        (r.at_seconds - first).abs() < 1e-6 && r.fade_seconds <= 0.0
    };
    let Some(target) = rows.iter().position(same) else {
        return;
    };
    let mut joined: Vec<FixtureCommand> = Vec::new();
    let mut kept = Vec::with_capacity(rows.len());
    for (i, row) in std::mem::take(rows).into_iter().enumerate() {
        if same(&row) {
            joined.extend(row.messages.iter().cloned());
            if i == target {
                kept.push(row);
            }
        } else {
            kept.push(row);
        }
    }
    if let Some(row) = kept.iter_mut().find(|r| same(r)) {
        row.messages = joined;
    }
    *rows = kept;
}

/// Fixtures that have a value at the starting look of fitted rows (`at` is where it was stamped).
pub fn lights_at_start(rows: &[crate::lightkey_smooth::SmoothLook], at: f64) -> usize {
    let first = rows
        .iter()
        .map(|r| r.at_seconds)
        .filter(|t| *t <= at + 0.05)
        .fold(f64::INFINITY, f64::min);
    if !first.is_finite() {
        return 0;
    }
    rows.iter()
        .filter(|r| (r.at_seconds - first).abs() < 1e-6)
        .flat_map(|r| r.messages.iter())
        .filter_map(FixtureCommand::fixture)
        .collect::<BTreeSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_follow_the_names() {
        for (name, class) in [
            ("dimmer", Some(Class::Level)),
            ("innerDimmerFine", Some(Class::Level)),
            ("fogAmount", Some(Class::Level)),
            ("color", Some(Class::Colour)),
            ("warmWhite", Some(Class::Colour)),
            ("coolWhite", Some(Class::Colour)),
            ("amber", Some(Class::Colour)),
            ("panAngle", Some(Class::Position)),
            ("tiltAngle", Some(Class::Position)),
            ("zoomAngle", Some(Class::Position)),
            ("focus", Some(Class::Position)),
            ("gobo1", Some(Class::Wheel)),
            ("color1", Some(Class::Wheel)),
            ("color2", Some(Class::Wheel)),
            ("frostAmount", Some(Class::Wheel)),
            ("prismType2", Some(Class::Wheel)),
            ("panTiltSpeed", None),
            ("goboRotationMode1", None),
            ("goboRotationSpeed1", None),
            ("goboAngle1", None),
            ("Color_for_Built-in_Effects", None),
            ("lit", None),
            ("off", None),
            ("command", None),
            ("strobe", None),
            ("programSpeedSlowFast", None),
        ] {
            assert_eq!(classify(name), class, "{name}");
        }
    }
}
