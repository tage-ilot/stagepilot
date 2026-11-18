//! The song order from the Library Link: the decision, and nothing else.
//!
//! The Playback API tells StagePilot which song is playing by `songId`; the Library Link's setlist
//! (`/setlists/current`, the one Playback opened last) lists the same numbers as `setlist_song_id`,
//! in Playback's order, with each song's length. With both in hand StagePilot can know the order and
//! the lengths without a scan, but a wrong order starts the wrong timer, so the order is used only
//! when every one of these holds (the first that does not is the reason given):
//!
//! 1. the Library Link is ready (or outdated, which still works) and its answer is current;
//! 2. the Playback API is connected and has said which song is selected and its setlist version;
//! 3. the setlist is the one Playback has open, and the selected song is in it;
//! 4. the loaded service plan pairs with it: every plan song pairs exactly or closely by title
//!    (`library::match_setlist`, the one matcher; songs only Playback has are allowed);
//! 5. no `setlist_song_id` is missing or repeated.
//!
//! A sixth guard keeps condition 4 honest: the countdown pairs the Nth plan song that is not skipped
//! with the Nth Playback song that is not skipped (`arrangement::pair_songs`; neither list is ever
//! reordered). Every plan song must therefore sit where its own song sits in Playback's order, with the
//! skips already saved in Song arrangement. If it does not, the library order would start the wrong
//! timer, so it is not used and the learned order or the scan stays what decides, as today.
//!
//! This module is pure: it reads what it is given and returns a verdict. Saving the order is the
//! plugin's job (`PlaybackPlugin::apply_library_order`), asking the helper is the server's.

use crate::arrangement::pair_songs;
use crate::library::{match_setlist, ParsedSetlist, PlanMatch, PlanTitle};
use crate::normalizer::Heartbeat;
use std::collections::BTreeSet;

/// What every sentence of the Song order section starts with when the Library Link's order is not used.
pub const NOT_USING: &str = "Not using the Library Link's order:";

/// What the decision reads. The caller fills it from the helper, the Playback plugin and the plan.
#[derive(Debug, Clone)]
pub struct Evidence<'a> {
    /// The Library Link answers with a ready or outdated helper and a readable library. When it
    /// does not, the Library Link is simply not part of this and the verdict is `Quiet`.
    pub link_in_use: bool,
    /// The helper's answer is a fresh one, not the last good copy shown while it is silent.
    pub link_current: bool,
    /// The setlist Playback has open (`/setlists/current`), when the helper could say.
    pub current: Option<&'a ParsedSetlist>,
    pub api_connected: bool,
    /// Playback is connected but has no song yet.
    pub loading_setlist: bool,
    pub heartbeat: Option<Heartbeat>,
    /// The loaded service plan's songs, in plan order; `None` when no plan is loaded.
    pub plan: Option<&'a [PlanTitle]>,
    /// The plan songs' own ids (`Song::id`), in the same order as `plan`.
    pub plan_ids: &'a [String],
    /// The skips saved in Song arrangement: plan songs by id, Playback songs by number.
    pub skipped_plan: &'a [String],
    pub skipped_playback: &'a [i64],
}

/// An order ready to be saved the way a finished scan saves one.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryOrder {
    /// Playback's song numbers, in Playback's order.
    pub order: Vec<i64>,
    /// The countdown length of each song (`LibrarySong::countdown_length_s`), `None` when unknown.
    pub lengths: Vec<Option<f64>>,
    /// The API's `setlistVersion` at the moment the order was decided.
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// No Library Link in use: nothing to say, nothing changes.
    Quiet,
    /// One plain sentence, ready to show ("Not using the Library Link's order: ...").
    NotUsed(String),
    Use(LibraryOrder),
}

fn not_used(reason: &str) -> Verdict {
    Verdict::NotUsed(format!("{NOT_USING} {reason}"))
}

/// Titles come from a plan or from Playback; keep the sentence a sentence.
fn quoted(title: &str) -> String {
    let clean: String = title.chars().filter(|c| !c.is_control()).take(60).collect();
    clean.trim().to_string()
}

pub fn decide(evidence: &Evidence<'_>) -> Verdict {
    if !evidence.link_in_use {
        return Verdict::Quiet;
    }
    // 1. The Library Link answers, and it is a current answer.
    if !evidence.link_current {
        return not_used("The Library Link did not answer just now.");
    }
    let Some(setlist) = evidence.current else {
        return not_used("The Library Link cannot read the setlist Playback has open.");
    };
    // 2. The Playback API.
    if !evidence.api_connected {
        return not_used("Playback is not connected.");
    }
    let heartbeat = match evidence.heartbeat {
        Some(heartbeat) if !evidence.loading_setlist => heartbeat,
        _ => return not_used("Playback has not said which song is selected yet."),
    };
    let Some(version) = heartbeat.setlist_version else {
        return not_used("Playback has not said which version of its setlist is open.");
    };
    // 5. One number per song, none twice. (Checked before the song is looked up in the list.)
    let mut ids = Vec::with_capacity(setlist.songs.len());
    for song in &setlist.songs {
        match song.setlist_song_id {
            Some(id) => ids.push(id),
            None => {
                return not_used(&format!(
                    "{} has no number in Playback's setlist.",
                    quoted(&song.title)
                ))
            }
        }
    }
    if ids.is_empty() {
        return not_used("Playback's setlist has no songs.");
    }
    let mut seen = BTreeSet::new();
    if let Some(repeated) = ids.iter().position(|id| !seen.insert(*id)) {
        return not_used(&format!(
            "{} is in Playback's setlist more than once.",
            quoted(&setlist.songs[repeated].title)
        ));
    }
    // 3. The setlist is the one the API is playing from.
    if !ids.contains(&heartbeat.song_id) {
        return not_used(
            "The song Playback has selected is not in the setlist the Library Link says is open.",
        );
    }
    // 4. Every song of the plan pairs.
    let Some(plan) = evidence.plan.filter(|plan| !plan.is_empty()) else {
        return not_used("No service plan is loaded.");
    };
    let matched = match_setlist(plan, &setlist.songs);
    if let Some(song) = matched
        .songs
        .iter()
        .find(|song| song.status != PlanMatch::Matched)
    {
        let title = quoted(&song.title);
        let words = match song.status {
            PlanMatch::Uncertain => format!(
                "{title} may be {} in Playback's setlist, but the titles are only alike.",
                song.candidate
                    .as_ref()
                    .map_or_else(String::new, |c| format!("“{}”", quoted(&c.title)))
            ),
            PlanMatch::DuplicateInPlan => format!("{title} is in the plan more than once."),
            PlanMatch::DuplicateInPlayback => {
                format!("{title} is in Playback's setlist more than once.")
            }
            _ => format!("{title} is not in Playback's setlist."),
        };
        return not_used(&words);
    }
    // 6. The countdown's pairing (the Nth with the Nth, skips left out) is the pairing by title.
    if plan.len() == evidence.plan_ids.len() {
        let plan_ids: Vec<&str> = evidence.plan_ids.iter().map(String::as_str).collect();
        let skipped_plan: Vec<&str> = evidence.skipped_plan.iter().map(String::as_str).collect();
        let pairs = pair_songs(&plan_ids, &ids, &skipped_plan, evidence.skipped_playback);
        let crossed = |plan_index: usize| {
            pairs.iter().find(|(p, _)| *p == plan_index).map_or(
                !skipped_plan.contains(&plan_ids[plan_index]),
                |(_, playback_index)| {
                    matched.songs[plan_index]
                        .library
                        .as_ref()
                        .and_then(|song| song.setlist_song_id)
                        != Some(ids[*playback_index])
                },
            )
        };
        if let Some(plan_index) = (0..plan.len()).find(|i| crossed(*i)) {
            return not_used(&format!(
                "{} is not in the same place in the plan as in Playback's setlist.",
                quoted(&plan[plan_index].title)
            ));
        }
    }
    Verdict::Use(LibraryOrder {
        order: ids,
        lengths: setlist
            .songs
            .iter()
            .map(|song| {
                song.countdown_length_s
                    .filter(|s| s.is_finite() && *s > 0.0)
            })
            .collect(),
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::parse_setlist;
    use serde_json::json;

    fn setlist(items: &[(Option<i64>, &str)]) -> ParsedSetlist {
        let items: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(i, (id, title))| {
                json!({"setlist_song_id": id, "order": i + 1, "title": title,
                       "length_s": 300.0 + i as f64, "scan_length_s": 296.5 + i as f64})
            })
            .collect();
        parse_setlist(&json!({"setlist_id": 7, "title": "Sunday", "performance_date": "2026-11-22", "items": items})).unwrap()
    }

    fn titles(names: &[&str]) -> Vec<PlanTitle> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| PlanTitle {
                key: format!("k{i}"),
                title: (*name).into(),
                seconds: Some(300),
                key_name: None,
            })
            .collect()
    }

    fn ids_of(plan: &[PlanTitle]) -> &'static [String] {
        // The plan's own song ids, one per title (leaked: a few bytes in a test run).
        Box::leak(
            (0..plan.len())
                .map(|i| format!("song-{i}"))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    fn beat(song_id: i64) -> Heartbeat {
        Heartbeat {
            song_id,
            position: 0.0,
            playing: false,
            pad: false,
            setlist_version: Some(41),
        }
    }

    fn evidence<'a>(list: &'a ParsedSetlist, plan: &'a [PlanTitle]) -> Evidence<'a> {
        Evidence {
            link_in_use: true,
            link_current: true,
            current: Some(list),
            api_connected: true,
            loading_setlist: false,
            heartbeat: Some(beat(11)),
            plan: Some(plan),
            plan_ids: ids_of(plan),
            skipped_plan: &[],
            skipped_playback: &[],
        }
    }

    fn reason(verdict: Verdict) -> String {
        match verdict {
            Verdict::NotUsed(line) => line,
            other => panic!("expected a reason, got {other:?}"),
        }
    }

    #[test]
    fn all_holding_gives_playbacks_order_with_the_countdown_lengths_and_keeps_a_song_only_playback_has(
    ) {
        // Playback has one more song, at the end, which the plan does not: it stays in the order, as in a scan.
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Way"), (Some(13), "Extra")]);
        let plan = titles(&["Holy", "Way"]);
        match decide(&evidence(&list, &plan)) {
            Verdict::Use(order) => {
                assert_eq!(order.order, vec![11, 12, 13]);
                assert_eq!(order.version, 41);
                assert_eq!(order.lengths, vec![Some(296.5), Some(297.5), Some(298.5)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_plan_in_another_order_than_playbacks_is_not_used_because_the_countdown_pairs_by_place() {
        // Plan #1 is Playback #2: every song pairs by title, but the Nth plan song starts the Nth Playback
        // song, so the library order would start the wrong timer. Not used.
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Way")]);
        let plan = titles(&["Way", "Holy"]);
        assert_eq!(
            reason(decide(&evidence(&list, &plan))),
            "Not using the Library Link's order: Way is not in the same place in the plan as in Playback's setlist."
        );
    }

    #[test]
    fn a_song_only_playback_has_in_the_middle_is_used_once_it_is_skipped_in_song_arrangement() {
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Walk-in"), (Some(13), "Way")]);
        let plan = titles(&["Holy", "Way"]);
        let base = evidence(&list, &plan);
        assert!(reason(decide(&base)).contains("Way is not in the same place"));
        let skipped = [12];
        assert!(matches!(
            decide(&Evidence {
                skipped_playback: &skipped,
                ..base
            }),
            Verdict::Use(_)
        ));
    }

    #[test]
    fn a_skipped_plan_song_does_not_have_to_pair_in_place() {
        // The plan's second song was skipped (no track); its title is still looked up and pairs.
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Way")]);
        let plan = titles(&["Holy", "Way"]);
        let skipped = ["song-1".to_string()];
        assert!(matches!(
            decide(&Evidence {
                skipped_plan: &skipped,
                ..evidence(&list, &plan)
            }),
            Verdict::Use(_)
        ));
    }

    #[test]
    fn each_condition_failing_on_its_own_says_why_and_never_uses_the_order() {
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Way")]);
        let plan = titles(&["Holy", "Way"]);
        let base = evidence(&list, &plan);
        assert_eq!(
            decide(&Evidence {
                link_in_use: false,
                ..base.clone()
            }),
            Verdict::Quiet
        );
        let line = reason(decide(&Evidence {
            link_current: false,
            ..base.clone()
        }));
        assert_eq!(
            line,
            "Not using the Library Link's order: The Library Link did not answer just now."
        );
        assert!(reason(decide(&Evidence {
            current: None,
            ..base.clone()
        }))
        .contains("cannot read the setlist Playback has open"));
        assert!(reason(decide(&Evidence {
            api_connected: false,
            ..base.clone()
        }))
        .ends_with("Playback is not connected."));
        assert!(reason(decide(&Evidence {
            heartbeat: None,
            ..base.clone()
        }))
        .contains("which song is selected"));
        assert!(reason(decide(&Evidence {
            loading_setlist: true,
            ..base.clone()
        }))
        .contains("which song is selected"));
        let no_version = Heartbeat {
            setlist_version: None,
            ..beat(11)
        };
        assert!(reason(decide(&Evidence {
            heartbeat: Some(no_version),
            ..base.clone()
        }))
        .contains("version"));
        assert!(reason(decide(&Evidence {
            heartbeat: Some(beat(99)),
            ..base.clone()
        }))
        .contains("is not in the setlist the Library Link says is open"));
        assert!(reason(decide(&Evidence {
            plan: None,
            ..base.clone()
        }))
        .contains("No service plan"));
    }

    #[test]
    fn a_plan_song_that_does_not_pair_is_named() {
        let list = setlist(&[(Some(11), "Holy"), (Some(12), "Way")]);
        let missing = titles(&["Holy", "Faithfulness"]);
        assert_eq!(
            reason(decide(&evidence(&list, &missing))),
            "Not using the Library Link's order: Faithfulness is not in Playback's setlist."
        );
        let twice = titles(&["Holy", "Holy"]);
        assert!(
            reason(decide(&evidence(&list, &twice))).contains("Holy is in the plan more than once")
        );
        let alike = setlist(&[(Some(11), "Oceans Where Feet May Fail"), (Some(12), "Way")]);
        let plan = titles(&["Oceans", "Way"]);
        assert!(reason(decide(&evidence(&alike, &plan))).contains("“Oceans Where Feet May Fail”"));
        let twin = setlist(&[(Some(11), "Holy"), (Some(12), "Holy")]);
        let plan = titles(&["Holy"]);
        assert!(reason(decide(&evidence(&twin, &plan)))
            .contains("Holy is in Playback's setlist more than once"));
    }

    #[test]
    fn a_missing_or_repeated_song_number_is_refused() {
        let plan = titles(&["Holy", "Way"]);
        let repeated = setlist(&[(Some(11), "Holy"), (Some(11), "Way")]);
        assert!(reason(decide(&evidence(&repeated, &plan)))
            .contains("Way is in Playback's setlist more than once"));
        let blank = setlist(&[(Some(11), "Holy"), (None, "Way")]);
        assert!(reason(decide(&evidence(&blank, &plan))).contains("Way has no number"));
    }

    #[test]
    fn an_unknown_length_stays_unknown_and_never_zero() {
        let list = parse_setlist(&json!({"setlist_id": 7, "title": "S", "items": [
            {"setlist_song_id": 11, "order": 1, "title": "Holy"},
            {"setlist_song_id": 12, "order": 2, "title": "Way", "length_s": 0.0}]}))
        .unwrap();
        let plan = titles(&["Holy", "Way"]);
        match decide(&evidence(&list, &plan)) {
            Verdict::Use(order) => assert_eq!(order.lengths, vec![None, None]),
            other => panic!("{other:?}"),
        }
    }
}
