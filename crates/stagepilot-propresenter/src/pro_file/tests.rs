use super::synthetic::{fake_uuid, SyntheticSong};
use super::*;

type Case = (&'static str, &'static str, &'static [(usize, usize)]);

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_string()).collect()
}

/// New uuids in a fixed order: `BBBBBBBB-0000-4000-8000-<n>`.
fn ids() -> impl FnMut() -> String {
    let mut n = 0u32;
    move || {
        n += 1;
        fake_uuid(0xBBBB_BBBB, n)
    }
}

fn song_with_full() -> SyntheticSong {
    // groups: 0 blank (count off), 1 Intro, 2 Verse 1, 3 Chorus, 4 Verse 2, 5 Bridge
    SyntheticSong::new("Test Song")
        .id(777)
        .group("", &["one two three four"])
        .group("Intro", &["intro text"])
        .group("Verse 1", &["v1 a", "v1 b"])
        .group("Chorus", &["chorus a", "chorus b"])
        .group("Verse 2", &["v2 a"])
        .group("Bridge", &["bridge a"])
        .arrangement("Full", &[0, 1, 2, 3, 4, 3, 5, 3])
        .arrangement("Short", &[1, 3])
        .selected(0)
        .key(
            KeyScale {
                key: 7,
                minor: false,
            },
            "C",
        )
}

fn mapped_names(map: &[Mapped]) -> Vec<(String, How, Option<String>)> {
    map.iter()
        .map(|m| (m.section.clone(), m.how, m.group_name.clone()))
        .collect()
}

#[test]
fn a_song_is_read_whole() {
    let source = song_with_full();
    let song = Song::read(&source.build()).unwrap();
    assert_eq!(song.name, "Test Song");
    assert_eq!(song.uuid, source.uuid());
    assert_eq!(song.multitracks_id, Some(777));
    assert_eq!(song.groups.len(), 6);
    assert_eq!(song.groups[2].name, "Verse 1");
    assert_eq!(song.groups[2].cue_ids.len(), 2);
    assert_eq!(song.arrangements.len(), 2);
    assert_eq!(song.arrangements[0].name, "Full");
    assert_eq!(song.arrangements[0].group_ids.len(), 8);
    assert_eq!(song.selected_arrangement().unwrap().name, "Full");
    assert_eq!(song.music.original_text, "C");
    assert_eq!(
        song.music.user,
        Some(KeyScale {
            key: 7,
            minor: false
        })
    );
    assert_eq!(
        song.arrangement_group_names(&song.arrangements[1]),
        s(&["Intro", "Chorus"])
    );
}

#[test]
fn bytes_that_are_not_a_song_are_refused_and_never_panic() {
    for bad in [
        &b""[..],
        b"this is not a protobuf at all\n",
        &[0xff; 40],
        &[0x0b, 0x01],
        &[0x00, 0x01],
    ] {
        assert_eq!(Song::read(bad), Err(ProError::NotASong), "{bad:?}");
    }
    let whole = song_with_full().build();
    for cut in (1..whole.len()).step_by(37) {
        // Never a panic; a cut in the middle of a record is refused.
        let _ = Song::read(&whole[..cut]);
    }
    assert!(Song::read(&whole[..whole.len() - 2]).is_err());
}

#[test]
fn the_longest_matching_runs_are_the_ones_python_difflib_finds() {
    // Expected pairs come from `difflib.SequenceMatcher(None, a, b, autojunk=False)` run in Python.
    let cases: [Case; 8] = [
        (
            "a b c d c e c",
            "a b c d c c e c f",
            &[(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 6), (6, 7)],
        ),
        ("a b a b a", "b a b a b", &[(0, 1), (1, 2), (2, 3), (3, 4)]),
        ("x y z", "p q r", &[]),
        ("a a a b", "a b a a", &[(0, 2), (1, 3)]),
        (
            "c a b a b a c",
            "a b a b c c",
            &[(1, 0), (2, 1), (3, 2), (4, 3), (6, 4)],
        ),
        ("", "a b", &[]),
        ("a b", "", &[]),
        (
            "a b c a b c d a b c",
            "d a b c a b c a b c a",
            &[
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 4),
                (4, 5),
                (5, 6),
                (7, 7),
                (8, 8),
                (9, 9),
            ],
        ),
    ];
    for (a, b, expected) in cases {
        let a: Vec<String> = a.split_whitespace().map(str::to_string).collect();
        let b: Vec<String> = b.split_whitespace().map(str::to_string).collect();
        let mut got = matching_pairs(&a, &b);
        got.sort_unstable();
        assert_eq!(got, expected, "{a:?} against {b:?}");
    }
    let more: [Case; 5] = [
        (
            "b c b c c c c a b a c a",
            "a b b a b",
            &[(7, 0), (8, 1), (9, 3)],
        ),
        (
            "a c a a c a b b a b a",
            "a c c b",
            &[(0, 0), (1, 1), (4, 2), (6, 3)],
        ),
        ("a a a a a", "a b b a c", &[(0, 0), (1, 3)]),
        (
            "a c a b b a",
            "b a a b a b b c",
            &[(0, 1), (2, 4), (3, 5), (4, 6)],
        ),
        (
            "a c c c b a b b b b c b",
            "b b c a a",
            &[(8, 0), (9, 1), (10, 2)],
        ),
    ];
    for (a, b, expected) in more {
        let a: Vec<String> = a.split_whitespace().map(str::to_string).collect();
        let b: Vec<String> = b.split_whitespace().map(str::to_string).collect();
        let mut got = matching_pairs(&a, &b);
        got.sort_unstable();
        assert_eq!(got, expected, "{a:?} against {b:?}");
    }
}

#[test]
fn sections_that_line_up_with_the_full_arrangement_map_exactly() {
    let song = Song::read(&song_with_full().build()).unwrap();
    let map = resolve(
        &song,
        &s(&[
            "Intro", "Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus",
        ]),
    );
    assert!(map.iter().all(|m| m.how == How::Full), "{map:?}");
    // The three choruses are the one group, each time.
    let chorus = song.groups[3].uuid.clone();
    assert_eq!(map[2].group_uuid.as_deref(), Some(chorus.as_str()));
    assert_eq!(map[6].group_uuid.as_deref(), Some(chorus.as_str()));
    assert_eq!(map.len(), 7);
}

#[test]
fn a_repeat_stays_its_own_entry_and_an_extra_group_only_in_full_is_not_needed() {
    let song = Song::read(&song_with_full().build()).unwrap();
    // Playback plays the chorus twice in a row and skips Verse 2, Bridge and the second chorus of the song.
    let map = resolve(&song, &s(&["Intro", "Chorus", "Chorus", "Verse 1"]));
    assert_eq!(map.len(), 4);
    assert!(map.iter().all(Mapped::is_mapped), "{map:?}");
    assert_eq!(map[1].group_name.as_deref(), Some("Chorus"));
    assert_eq!(map[2].group_name.as_deref(), Some("Chorus"));
    assert_eq!(map[1].group_uuid, map[2].group_uuid);
}

#[test]
fn count_off_is_the_group_with_a_blank_name() {
    let song = Song::read(&song_with_full().build()).unwrap();
    let map = resolve(&song, &s(&["Count Off", "Intro", "Verse 1"]));
    assert_eq!(map[0].group_name.as_deref(), Some(""));
    assert!(map.iter().all(Mapped::is_mapped), "{map:?}");
    // Nothing blank in the song: the section is missing.
    let without = SyntheticSong::new("No Count")
        .group("Intro", &["a"])
        .arrangement("Full", &[0])
        .selected(0)
        .build();
    let song = Song::read(&without).unwrap();
    let map = resolve(&song, &s(&["Count Off", "Intro"]));
    assert_eq!(map[0].how, How::Missing);
    assert_eq!(map[1].how, How::Full);
}

#[test]
fn a_name_with_one_group_maps_by_name_when_full_does_not_line_up() {
    // Playback plays Bridge first: nothing lines up with Full there, the name is unique.
    let song = Song::read(&song_with_full().build()).unwrap();
    let map = resolve(&song, &s(&["Bridge", "Verse 2"]));
    let hows: Vec<How> = map.iter().map(|m| m.how).collect();
    assert!(
        hows.iter().all(|h| matches!(h, How::Only | How::Full)),
        "{hows:?}"
    );
    assert_eq!(map[0].group_name.as_deref(), Some("Bridge"));
    assert_eq!(map[1].group_name.as_deref(), Some("Verse 2"));
    // Names are compared as letters and digits only, in lower case.
    let map = resolve(&song, &s(&["VERSE-2!", " chorus "]));
    assert!(map.iter().all(Mapped::is_mapped), "{map:?}");
}

#[test]
fn a_section_no_group_has_is_missing_and_the_copy_is_not_made() {
    let source = song_with_full().build();
    let song = Song::read(&source).unwrap();
    let map = resolve(&song, &s(&["Intro", "Tag", "Instrumental"]));
    assert_eq!(map[1].how, How::Missing);
    assert_eq!(map[2].how, How::Missing);
    let request = CopyRequest {
        sections: &s(&["Intro", "Tag"]),
        key: None,
        arrangement_name: "Playback",
        now: 1,
    };
    match build_copy(&source, &request, &mut ids()) {
        Err(ProError::Unmapped(map)) => {
            assert_eq!(
                unmapped_names(&map),
                ["Tag (no group has that name)".to_string()]
            );
        }
        other => panic!("{other:?}"),
    }
    let none = CopyRequest {
        sections: &[],
        ..request
    };
    assert!(matches!(
        build_copy(&source, &none, &mut ids()),
        Err(ProError::Unmapped(_))
    ));
}

#[test]
fn groups_of_one_name_are_fine_when_identical_and_guessed_when_not() {
    let identical = SyntheticSong::new("Twins")
        .group("Chorus", &["same words"])
        .group("Chorus", &["same words"])
        .group("Verse", &["v"])
        .arrangement("Full", &[2])
        .selected(0)
        .build();
    let song = Song::read(&identical).unwrap();
    let map = resolve(&song, &s(&["Chorus", "Chorus"]));
    assert_eq!(map[0].how, How::Identical);
    assert_eq!(map[1].how, How::Identical);
    // The first time is the first group, the second time the second.
    assert_eq!(
        map[0].group_uuid.as_deref(),
        Some(song.groups[0].uuid.as_str())
    );
    assert_eq!(
        map[1].group_uuid.as_deref(),
        Some(song.groups[1].uuid.as_str())
    );

    let different = SyntheticSong::new("Different")
        .group("Chorus", &["first words"])
        .group("Chorus", &["other words"])
        .group("Verse", &["v"])
        .arrangement("Full", &[2])
        .selected(0)
        .build();
    let song = Song::read(&different).unwrap();
    let map = resolve(&song, &s(&["Verse", "Chorus"]));
    // Groups that differ are not refused: one is guessed, counted as mapped, and named.
    assert_eq!(map[1].how, How::Guessed);
    assert!(map[1].is_mapped());
    assert_eq!(
        map[1].group_uuid.as_deref(),
        Some(song.groups[0].uuid.as_str())
    );
    assert!(unmapped_names(&map).is_empty());
    let guess = map[1].guess.as_ref().unwrap();
    assert_eq!(
        (guess.position, guess.uses, guess.group_index, guess.groups),
        (1, 1, 1, 2)
    );
    assert_eq!(guess.rule, GuessRule::InOrder);
    assert_eq!(map[0].guess, None);
    // When Full says where the chorus goes, the song's own arrangement settles it.
    let with_full = SyntheticSong::new("Different")
        .group("Chorus", &["first words"])
        .group("Chorus", &["other words"])
        .arrangement("Full", &[0, 1])
        .selected(0)
        .build();
    let song = Song::read(&with_full).unwrap();
    let map = resolve(&song, &s(&["Chorus", "Chorus"]));
    assert!(map.iter().all(|m| m.how == How::Full));
    assert_ne!(map[0].group_uuid, map[1].group_uuid);
}

#[test]
fn key_text_becomes_the_key_and_scale() {
    let table = [
        ("Ab", 0, false),
        ("A", 1, false),
        ("A#", 2, false),
        ("Bb", 3, false),
        ("B", 4, false),
        ("B#", 5, false),
        ("Cb", 6, false),
        ("C", 7, false),
        ("C#", 8, false),
        ("Db", 9, false),
        ("D", 10, false),
        ("D#", 11, false),
        ("Eb", 12, false),
        ("E", 13, false),
        ("E#", 14, false),
        ("Fb", 15, false),
        ("F", 16, false),
        ("F#", 17, false),
        ("Gb", 18, false),
        ("G", 19, false),
        ("G#", 20, false),
        ("Am", 1, true),
        ("Bbm", 3, true),
        ("F#m", 17, true),
        ("C♯", 8, false),
        ("E♭", 12, false),
        ("D minor", 10, true),
        ("G major", 19, false),
        ("em", 13, true),
        (" Eb ", 12, false),
        ("Bb", 3, false),
        ("Dm", 10, true),
        ("A min", 1, true),
        ("C-", 7, true),
    ];
    for (text, key, minor) in table {
        assert_eq!(parse_key(text), Some(KeyScale { key, minor }), "{text:?}");
    }
    for (text, key, minor) in table.iter().filter(|t| t.0.len() <= 3 && t.0.is_ascii()) {
        let scale = KeyScale {
            key: *key,
            minor: *minor,
        };
        // The name written back is one StagePilot reads again.
        assert_eq!(parse_key(&scale.name()), Some(scale), "{text:?}");
    }
    for junk in [
        "", " ", "H", "Cx", "C##", "Hm", "Abm7", "key", "1", "Cmaj7", "Fb#", "-",
    ] {
        assert_eq!(parse_key(junk), None, "{junk:?}");
    }
}

#[test]
fn a_copy_adds_playback_selects_it_and_keeps_everything_else() {
    let source = song_with_full().build();
    let sections = s(&[
        "Count Off",
        "Intro",
        "Verse 1",
        "Chorus",
        "Verse 2",
        "Chorus",
    ]);
    let request = CopyRequest {
        sections: &sections,
        key: Some("Eb"),
        arrangement_name: "Playback",
        now: 1_800_000_000,
    };
    let copy = build_copy(&source, &request, &mut ids()).unwrap();
    assert!(copy.mapping.iter().all(Mapped::is_mapped));
    assert_eq!(copy.key, KeyOutcome::Written("Eb".into()));
    let before = Song::read(&source).unwrap();
    let after = Song::read(&copy.bytes).unwrap();
    // The name and the number are unchanged; the presentation has a new uuid.
    assert_eq!(after.name, "Test Song");
    assert_eq!(after.multitracks_id, Some(777));
    assert_ne!(after.uuid, before.uuid);
    assert_eq!(after.uuid, copy.uuid);
    // Every original arrangement is kept (by name and by what it plays), one is added and selected.
    assert_eq!(after.arrangements.len(), 3);
    for (old, new) in before.arrangements.iter().zip(&after.arrangements) {
        assert_eq!(old.name, new.name);
        assert_eq!(
            before.arrangement_group_names(old),
            after.arrangement_group_names(new)
        );
    }
    let playback = after.selected_arrangement().unwrap();
    assert_eq!(playback.name, "Playback");
    assert_eq!(
        after.arrangement_group_names(playback),
        s(&["", "Intro", "Verse 1", "Chorus", "Verse 2", "Chorus"])
    );
    // The key: the song's own key stays, the user's key is Playback's.
    assert_eq!(
        after.music.original,
        Some(KeyScale {
            key: 7,
            minor: false
        })
    );
    assert_eq!(after.music.original_text, "C");
    assert_eq!(
        after.music.user,
        Some(KeyScale {
            key: 12,
            minor: false
        })
    );
    assert_eq!(after.music.user_text, "Eb");
    // Dates are now.
    let top = split(&copy.bytes).unwrap();
    let date = |n: u32| top.iter().find(|(k, _)| *k == n).unwrap().1.clone();
    assert_eq!(date(4), len_record(4, &varint_record(1, 1_800_000_000)));
    assert_eq!(date(5), len_record(5, &varint_record(1, 1_800_000_000)));
    // The copy plays what Playback plays, in the key.
    assert!(after.already_plays(&sections, Some("Eb")));
    assert!(!after.already_plays(&sections, Some("F")));
    assert!(!before.already_plays(&sections, Some("Eb")));
}

#[test]
fn every_inner_uuid_is_new_and_the_song_is_still_consistent() {
    let source_song = song_with_full();
    let source = source_song.build();
    let request = CopyRequest {
        sections: &s(&["Intro", "Verse 1", "Chorus"]),
        key: None,
        arrangement_name: "Playback",
        now: 5,
    };
    let copy = build_copy(&source, &request, &mut ids()).unwrap();
    let out = String::from_utf8_lossy(&copy.bytes).into_owned();
    // No uuid of the song is left, whichever of its places it was in.
    let mut old: Vec<String> = vec![source_song.uuid()];
    for i in 0..6 {
        old.push(source_song.group_uuid(i));
        old.push(fake_uuid(0xA000_0000, 900 + i as u32));
    }
    for i in 0..2 {
        old.push(source_song.arrangement_uuid(i));
    }
    for n in 1..=8 {
        old.push(fake_uuid(0xA000_0000, 1000 + n));
    }
    for uuid in &old {
        assert!(!out.contains(uuid), "{uuid} is still in the copy");
    }
    // The same old uuid became the same new one everywhere: every id an arrangement or a group names exists.
    let song = Song::read(&copy.bytes).unwrap();
    for arrangement in &song.arrangements {
        for id in &arrangement.group_ids {
            assert!(song.groups.iter().any(|g| same(&g.uuid, id)), "{id}");
        }
    }
    // Cues still belong to their groups (their text is still found through the new ids).
    for group in &song.groups {
        assert!(
            group.signature.iter().all(|cue| !cue.is_empty()),
            "{}",
            group.name
        );
    }
    let before = Song::read(&source).unwrap();
    for (a, b) in before.groups.iter().zip(&song.groups) {
        assert_eq!(a.signature, b.signature);
        assert_eq!(a.name, b.name);
        assert_eq!(a.cue_ids.len(), b.cue_ids.len());
    }
    // The new uuids come from the source that was given and nowhere else.
    assert!(out.contains("BBBBBBBB-0000-4000-8000-"));
}

#[test]
fn unknown_fields_and_messages_survive_byte_for_byte() {
    let source = song_with_full().build();
    let request = CopyRequest {
        sections: &s(&["Intro", "Chorus"]),
        key: Some("G"),
        arrangement_name: "Playback",
        now: 9,
    };
    let copy = build_copy(&source, &request, &mut ids()).unwrap();
    let before = split(&source).unwrap();
    let after = split(&copy.bytes).unwrap();
    // The fields that are not StagePilot's to change are the very same bytes, in the same order.
    for number in [1u32, 6, 21, 40, 41, 42] {
        let old: Vec<_> = before.iter().filter(|(n, _)| *n == number).collect();
        let new: Vec<_> = after.iter().filter(|(n, _)| *n == number).collect();
        assert!(!old.is_empty(), "field {number} is in the source");
        assert_eq!(old, new, "field {number}");
    }
    // The groups and cues differ only in the uuid texts: the colour and the unknown field inside a group are the same bytes.
    let blank_uuids = |bytes: &[u8]| -> Vec<u8> {
        let mut out = bytes.to_vec();
        let mut i = 0;
        while i + 38 <= out.len() {
            if out[i] == 0x0a && out[i + 1] == 36 && exact_uuid(&out[i..i + 38]).is_some() {
                for b in &mut out[i + 2..i + 38] {
                    *b = b'0';
                }
                i += 38;
            } else {
                i += 1;
            }
        }
        out
    };
    for number in [12u32, 13] {
        let old: Vec<_> = before
            .iter()
            .filter(|(n, _)| *n == number)
            .map(|(_, b)| blank_uuids(b))
            .collect();
        let new: Vec<_> = after
            .iter()
            .filter(|(n, _)| *n == number)
            .map(|(_, b)| blank_uuids(b))
            .collect();
        assert_eq!(old, new, "field {number}");
    }
    let group_extra = len_record(15, b"group extra");
    assert!(copy
        .bytes
        .windows(group_extra.len())
        .any(|w| w == group_extra.as_slice()));
    // Only these fields changed in the copy.
    let changed: Vec<u32> = after
        .iter()
        .filter(|record| !before.contains(record))
        .map(|(n, _)| *n)
        .collect();
    assert!(
        changed
            .iter()
            .all(|n| [2, 4, 5, 10, 11, 12, 13, 23].contains(n)),
        "{changed:?}"
    );
    // Fields that are not a message at all (varint, fixed32) survive too.
    assert!(after
        .iter()
        .any(|(n, b)| *n == 40 && b == &varint_record(40, 7)));
}

#[test]
fn the_same_inputs_make_the_same_bytes() {
    let source = song_with_full().build();
    let request = CopyRequest {
        sections: &s(&["Intro", "Verse 1"]),
        key: Some("Am"),
        arrangement_name: "Playback",
        now: 100,
    };
    let a = build_copy(&source, &request, &mut ids()).unwrap();
    let b = build_copy(&source, &request, &mut ids()).unwrap();
    assert_eq!(a.bytes, b.bytes);
    let mut other = {
        let mut n = 0u32;
        move || {
            n += 1;
            fake_uuid(0xCCCC_CCCC, n)
        }
    };
    let c = build_copy(&source, &request, &mut other).unwrap();
    assert_ne!(a.bytes, c.bytes);
    assert_eq!(a.bytes.len(), c.bytes.len());
    let later = CopyRequest {
        now: 101,
        ..request
    };
    assert_ne!(
        a.bytes,
        build_copy(&source, &later, &mut ids()).unwrap().bytes
    );
}

#[test]
fn an_unknown_key_text_leaves_the_key_alone_and_says_so() {
    let source = song_with_full().build();
    let sections = s(&["Intro"]);
    let mut request = CopyRequest {
        sections: &sections,
        key: Some("H#"),
        arrangement_name: "Playback",
        now: 3,
    };
    let copy = build_copy(&source, &request, &mut ids()).unwrap();
    assert_eq!(copy.key, KeyOutcome::Unknown("H#".into()));
    let song = Song::read(&copy.bytes).unwrap();
    assert_eq!(song.music, Song::read(&source).unwrap().music);
    request.key = None;
    assert_eq!(
        build_copy(&source, &request, &mut ids()).unwrap().key,
        KeyOutcome::NotGiven
    );
    request.key = Some("  ");
    assert_eq!(
        build_copy(&source, &request, &mut ids()).unwrap().key,
        KeyOutcome::NotGiven
    );
}

#[test]
fn a_song_with_no_music_gets_one_and_a_minor_key_is_written() {
    let source = SyntheticSong::new("Bare")
        .group("Verse", &["a"])
        .arrangement("Full", &[0])
        .selected(0)
        .build();
    assert_eq!(Song::read(&source).unwrap().music, Music::default());
    let sections = s(&["Verse"]);
    let request = CopyRequest {
        sections: &sections,
        key: Some("F#m"),
        arrangement_name: "Playback",
        now: 3,
    };
    let copy = build_copy(&source, &request, &mut ids()).unwrap();
    let song = Song::read(&copy.bytes).unwrap();
    assert_eq!(
        song.music.user,
        Some(KeyScale {
            key: 17,
            minor: true
        })
    );
    assert_eq!(song.music.user_text, "F#m");
    assert_eq!(song.music.original, None);
    // A song with no selected arrangement gets one.
    assert_eq!(song.selected_arrangement().unwrap().name, "Playback");
    assert!(!Song::read(&source)
        .unwrap()
        .already_plays(&sections, Some("F#m")));
}

#[test]
fn a_song_with_no_full_arrangement_maps_by_name() {
    let source = SyntheticSong::new("NoFull")
        .group("Verse", &["a"])
        .group("Chorus", &["b"])
        .build();
    let song = Song::read(&source).unwrap();
    assert!(song.selected_arrangement().is_none());
    let map = resolve(&song, &s(&["Chorus", "Verse", "Chorus"]));
    assert_eq!(
        mapped_names(&map),
        vec![
            ("Chorus".to_string(), How::Only, Some("Chorus".to_string())),
            ("Verse".to_string(), How::Only, Some("Verse".to_string())),
            ("Chorus".to_string(), How::Only, Some("Chorus".to_string())),
        ]
    );
}

#[test]
fn a_new_playback_copy_of_a_copy_keeps_the_earlier_playback_arrangement_too() {
    let source = song_with_full().build();
    let first = build_copy(
        &source,
        &CopyRequest {
            sections: &s(&["Intro", "Chorus"]),
            key: Some("D"),
            arrangement_name: "Playback",
            now: 10,
        },
        &mut ids(),
    )
    .unwrap();
    let second = build_copy(
        &first.bytes,
        &CopyRequest {
            sections: &s(&["Intro", "Verse 1", "Chorus"]),
            key: Some("D"),
            arrangement_name: "Playback",
            now: 11,
        },
        &mut ids(),
    )
    .unwrap();
    let song = Song::read(&second.bytes).unwrap();
    let names: Vec<&str> = song.arrangements.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Full", "Short", "Playback", "Playback"]);
    assert_eq!(
        song.arrangement_group_names(song.selected_arrangement().unwrap())
            .len(),
        3
    );
}

#[test]
fn only_a_whole_uuid_message_is_replaced_never_text_that_looks_like_one() {
    // A text field that holds a uuid is not a UUID message.
    let uuid = fake_uuid(1, 1);
    let message = len_record(6, uuid.as_bytes());
    let mut map = HashMap::new();
    assert_eq!(remap_uuids(&message, &mut map, &mut ids(), 0), None);
    // A UUID message nested two deep is, and the nil uuid is not.
    let inner = len_record(2, &uuid_message(&uuid));
    let outer = len_record(9, &inner);
    let changed = remap_uuids(&outer, &mut map, &mut ids(), 0).unwrap();
    assert!(!String::from_utf8_lossy(&changed).contains(&uuid));
    let nil = len_record(9, &len_record(2, &uuid_message(NIL_UUID)));
    assert_eq!(remap_uuids(&nil, &mut HashMap::new(), &mut ids(), 0), None);
    // The same uuid in another letter case becomes the same new one.
    let lower = uuid_message(&uuid.to_ascii_lowercase());
    let both = [len_record(2, &uuid_message(&uuid)), len_record(3, &lower)].concat();
    let out = remap_uuids(&len_record(9, &both), &mut HashMap::new(), &mut ids(), 0).unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    assert_eq!(
        text.matches("BBBBBBBB-0000-4000-8000-000000000001").count(),
        2
    );
}

// ---- guessing among groups of one name -------------------------------------------------------------------------------

/// A song whose `Interlude` groups all say different things, and a `Verse` that `Full` plays (so no section lines up with
/// `Full` unless a test wants it to).
fn interlude_song(interludes: usize) -> Vec<u8> {
    let mut song = SyntheticSong::new("Interludes").group("Verse", &["v"]);
    for n in 0..interludes {
        song = song.group("Interlude", &[&format!("interlude words {n}")]);
    }
    song.arrangement("Full", &[0]).selected(0).build()
}

fn group_numbers(song: &Song, map: &[Mapped]) -> Vec<usize> {
    // 1 for the first Interlude group of the song, 2 for the second, ...
    map.iter()
        .map(|m| {
            song.groups
                .iter()
                .filter(|g| g.name == "Interlude")
                .position(|g| Some(&g.uuid) == m.group_uuid.as_ref())
                .map_or(0, |p| p + 1)
        })
        .collect()
}

fn rules(map: &[Mapped]) -> Vec<GuessRule> {
    map.iter()
        .filter_map(|m| m.guess.as_ref())
        .map(|g| g.rule)
        .collect()
}

#[test]
fn as_many_uses_as_groups_take_the_groups_in_order() {
    let song = Song::read(&interlude_song(2)).unwrap();
    let map = resolve(&song, &s(&["Interlude", "Interlude"]));
    assert!(map.iter().all(|m| m.how == How::Guessed), "{map:?}");
    assert_eq!(group_numbers(&song, &map), [1, 2]);
    assert_eq!(rules(&map), [GuessRule::InOrder; 2]);
}

#[test]
fn one_extra_use_takes_the_last_group_again() {
    // How Awesome: three Interludes in Playback, two in the song.
    let song = Song::read(&interlude_song(2)).unwrap();
    let map = resolve(&song, &s(&["Interlude", "Interlude", "Interlude"]));
    assert!(map.iter().all(Mapped::is_mapped));
    assert_eq!(group_numbers(&song, &map), [1, 2, 2]);
    assert_eq!(
        rules(&map),
        [
            GuessRule::InOrder,
            GuessRule::InOrder,
            GuessRule::LastReused
        ]
    );
    let third = map[2].guess.as_ref().unwrap();
    assert_eq!(
        (third.position, third.uses, third.group_index, third.groups),
        (3, 3, 2, 2)
    );
    assert_eq!(
        third.words("Interlude", "Interlude"),
        "Interlude (3rd use) uses the 2nd \"Interlude\" group: check it in ProPresenter. \
         Playback uses the name once more than the song has groups, so the last group is used again."
    );
}

#[test]
fn two_extra_uses_or_more_go_round_and_round() {
    let song = Song::read(&interlude_song(2)).unwrap();
    let four = resolve(&song, &s(&["Interlude"; 4]));
    assert_eq!(group_numbers(&song, &four), [1, 2, 1, 2]);
    assert_eq!(rules(&four), [GuessRule::RepeatInTurn; 4]);
    let five = resolve(&song, &s(&["Interlude"; 5]));
    assert_eq!(group_numbers(&song, &five), [1, 2, 1, 2, 1]);
    assert_eq!(rules(&five), [GuessRule::RepeatInTurn; 5]);
    // Three groups, five uses: one, two, three, one, two.
    let song = Song::read(&interlude_song(3)).unwrap();
    let map = resolve(&song, &s(&["Interlude"; 5]));
    assert_eq!(group_numbers(&song, &map), [1, 2, 3, 1, 2]);
}

#[test]
fn one_group_with_several_uses_is_simply_used_each_time() {
    let song = Song::read(&interlude_song(1)).unwrap();
    let map = resolve(&song, &s(&["Interlude"; 4]));
    assert!(map.iter().all(|m| m.how == How::Only), "{map:?}");
    assert!(map.iter().all(|m| m.guess.is_none()));
    assert_eq!(group_numbers(&song, &map), [1, 1, 1, 1]);
}

#[test]
fn four_blank_count_off_groups_are_guessed_not_refused() {
    // Thank God I'm Free: four blank-named groups.
    let mut song = SyntheticSong::new("Count Offs").group("Verse", &["v"]);
    for n in 0..4 {
        song = song.group("", &[&format!("count {n}")]);
    }
    let source = song.arrangement("Full", &[0]).selected(0).build();
    let parsed = Song::read(&source).unwrap();
    let sections = s(&["Count Off", "Verse"]);
    let map = resolve(&parsed, &sections);
    assert_eq!(map[0].how, How::Guessed);
    assert_eq!(map[0].group_name.as_deref(), Some(""));
    assert_eq!(
        map[0].group_uuid.as_deref(),
        Some(parsed.groups[1].uuid.as_str())
    );
    assert_eq!(map[1].how, How::Full);
    assert_eq!(
        map[0].guess.as_ref().unwrap().words("Count Off", ""),
        "Count Off (1st use) uses the 1st blank-named group: check it in ProPresenter. \
         The song's groups with that name are used in the order the song has them."
    );
    let request = CopyRequest {
        sections: &sections,
        key: None,
        arrangement_name: "Playback",
        now: 1,
    };
    let copy =
        build_copy(&source, &request, &mut ids()).expect("a song with four blank groups builds");
    assert_eq!(copy.mapping[0].how, How::Guessed);
}

#[test]
fn the_guess_follows_the_songs_own_full_order_when_full_has_every_group() {
    // Interludes X (group 1) and Y (group 2); Full plays Y before X.
    let build = |full: &[usize]| {
        let bytes = SyntheticSong::new("Order")
            .group("Verse", &["v"])
            .group("Interlude", &["x words"])
            .group("Interlude", &["y words"])
            .arrangement("Full", full)
            .selected(0)
            .build();
        Song::read(&bytes).unwrap()
    };
    let sections = s(&["Interlude", "Interlude", "Interlude"]);
    // Full = Verse, Y, X: the first two uses line up with Y then X; the third is guessed and, being one extra, takes the last of
    // the order Full gives (X).
    let song = build(&[0, 2, 1]);
    let map = resolve(&song, &sections);
    assert_eq!(
        map.iter().map(|m| m.how).collect::<Vec<_>>(),
        [How::Full, How::Full, How::Guessed]
    );
    assert_eq!(
        map[2].group_uuid.as_deref(),
        Some(song.groups[1].uuid.as_str())
    );
    assert_eq!(map[2].guess.as_ref().unwrap().rule, GuessRule::LastReused);
    // Full lacks Y: the song's own group order (X, Y) is used instead, so the last group is Y.
    let song = build(&[0, 1]);
    let map = resolve(&song, &sections);
    assert_eq!(map[2].how, How::Guessed);
    assert_eq!(
        map[2].group_uuid.as_deref(),
        Some(song.groups[2].uuid.as_str())
    );
    // Four Interlude uses, two groups, Full has both (order Y, X).
    let song = build(&[0, 2, 1]);
    let map = resolve(
        &song,
        &s(&["Verse", "Interlude", "Interlude", "Interlude", "Interlude"]),
    );
    // Verse and the first two Interludes line up with Full (Verse, Y, X); the 3rd and 4th Interlude uses are guessed.
    assert_eq!(map[3].how, How::Guessed);
    assert_eq!(map[4].how, How::Guessed);
    // Four uses, two extra: round and round in Full's order (Y, X, Y, X): the 3rd use is Y, the 4th X.
    assert_eq!(
        map[3].group_uuid.as_deref(),
        Some(song.groups[2].uuid.as_str())
    );
    assert_eq!(
        map[4].group_uuid.as_deref(),
        Some(song.groups[1].uuid.as_str())
    );
}

#[test]
fn a_section_with_no_group_of_that_name_still_blocks_and_guesses_never_hide_it() {
    let source = interlude_song(2);
    let song = Song::read(&source).unwrap();
    let map = resolve(&song, &s(&["Interlude", "Tag", "Interlude", "Interlude"]));
    assert_eq!(map[1].how, How::Missing);
    assert!(!map[1].is_mapped());
    assert_eq!(
        unmapped_names(&map),
        ["Tag (no group has that name)".to_string()]
    );
    let request = CopyRequest {
        sections: &s(&["Interlude", "Tag"]),
        key: None,
        arrangement_name: "Playback",
        now: 1,
    };
    assert!(matches!(
        build_copy(&source, &request, &mut ids()),
        Err(ProError::Unmapped(_))
    ));
}

#[test]
fn ordinals_read_naturally() {
    let shown: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111]
        .into_iter()
        .map(ordinal)
        .collect();
    assert_eq!(
        shown,
        [
            "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "101st",
            "111th"
        ]
    );
}
