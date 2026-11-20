//! Tiny made-up song files for tests (never a real song, never real lyrics): the same protobuf layout ProPresenter 19 uses, with
//! unknown fields on purpose so that a test can see them survive a copy byte for byte.

use super::{len_record, uuid_message, varint_record, KeyScale};

/// One made-up song.
#[derive(Debug, Clone)]
pub struct SyntheticSong {
    title: String,
    multitracks: Option<u64>,
    /// (group name, the text of each of its slides)
    groups: Vec<(String, Vec<String>)>,
    /// (arrangement name, indexes into `groups`)
    arrangements: Vec<(String, Vec<usize>)>,
    selected: Option<usize>,
    key: Option<(KeyScale, String)>,
    seed: u32,
    unknown: bool,
}

/// `8-4-4-4-12` uuid text made from two numbers.
pub fn fake_uuid(seed: u32, n: u32) -> String {
    format!("{seed:08X}-0000-4000-8000-{n:012X}")
}

impl SyntheticSong {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            multitracks: None,
            groups: Vec::new(),
            arrangements: Vec::new(),
            selected: None,
            key: None,
            seed: 0xA000_0000,
            unknown: true,
        }
    }

    #[must_use]
    pub fn id(mut self, id: u64) -> Self {
        self.multitracks = Some(id);
        self
    }

    /// Uuids are made from this number, so two songs never share one by accident.
    #[must_use]
    pub fn seed(mut self, seed: u32) -> Self {
        self.seed = seed;
        self
    }

    /// A group with one slide per text. An empty name is the blank group (Count Off).
    #[must_use]
    pub fn group(mut self, name: &str, slides: &[&str]) -> Self {
        self.groups.push((
            name.to_string(),
            slides.iter().map(|s| (*s).to_string()).collect(),
        ));
        self
    }

    /// An arrangement of the groups at these positions (0-based, in the order the groups were added).
    #[must_use]
    pub fn arrangement(mut self, name: &str, groups: &[usize]) -> Self {
        self.arrangements.push((name.to_string(), groups.to_vec()));
        self
    }

    /// The arrangement (position in the order added) that is selected.
    #[must_use]
    pub fn selected(mut self, arrangement: usize) -> Self {
        self.selected = Some(arrangement);
        self
    }

    /// The song's own key (`music.original` and `music.user`).
    #[must_use]
    pub fn key(mut self, scale: KeyScale, text: &str) -> Self {
        self.key = Some((scale, text.to_string()));
        self
    }

    /// No unknown fields (the default has some, at the top and inside a group).
    #[must_use]
    pub fn without_unknown_fields(mut self) -> Self {
        self.unknown = false;
        self
    }

    pub fn group_uuid(&self, index: usize) -> String {
        fake_uuid(self.seed, 100 + index as u32)
    }

    pub fn arrangement_uuid(&self, index: usize) -> String {
        fake_uuid(self.seed, 200 + index as u32)
    }

    pub fn uuid(&self) -> String {
        fake_uuid(self.seed, 1)
    }

    /// The bytes of the `.pro`.
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // 1 application_info: something opaque
        out.extend(len_record(1, &[0x08, 0x01, 0x12, 0x03, b'1', b'9', b'0']));
        out.extend(len_record(2, &uuid_message(&self.uuid())));
        out.extend(len_record(3, self.title.as_bytes()));
        out.extend(len_record(4, &varint_record(1, 1_600_000_000)));
        out.extend(len_record(5, &varint_record(1, 1_600_000_001)));
        out.extend(len_record(6, b"a category"));
        if let Some(i) = self.selected {
            out.extend(len_record(10, &uuid_message(&self.arrangement_uuid(i))));
        }
        for (i, (name, groups)) in self.arrangements.iter().enumerate() {
            let mut body = len_record(1, &uuid_message(&self.arrangement_uuid(i)));
            body.extend(len_record(2, name.as_bytes()));
            for g in groups {
                body.extend(len_record(3, &uuid_message(&self.group_uuid(*g))));
            }
            out.extend(len_record(11, &body));
        }
        let mut cue_number = 0u32;
        let mut cues = Vec::new();
        for (g, (name, slides)) in self.groups.iter().enumerate() {
            let mut group = len_record(1, &uuid_message(&self.group_uuid(g)));
            group.extend(len_record(2, name.as_bytes()));
            // a colour (an opaque message)
            group.extend(len_record(3, &[0x0d, 0, 0, 0x80, 0x3f]));
            if self.unknown {
                // an unknown field inside a group
                group.extend(len_record(15, b"group extra"));
            }
            // the same uuid again as the group's "application group" (it is remapped like every other)
            group.extend(len_record(
                5,
                &uuid_message(&fake_uuid(self.seed, 900 + g as u32)),
            ));
            let mut body = len_record(1, &group);
            for text in slides {
                cue_number += 1;
                let cue_uuid = fake_uuid(self.seed, 1000 + cue_number);
                body.extend(len_record(2, &uuid_message(&cue_uuid)));
                let rtf = len_record(5, text.as_bytes());
                let graphic = len_record(13, &rtf);
                let element = len_record(1, &graphic);
                let base = len_record(1, &element);
                let presentation = len_record(1, &base);
                let slide = len_record(2, &presentation);
                let action = len_record(23, &slide);
                let mut cue = len_record(1, &uuid_message(&cue_uuid));
                cue.extend(len_record(2, format!("cue {cue_number}").as_bytes()));
                cue.extend(len_record(10, &action));
                cues.push(len_record(13, &cue));
            }
            out.extend(len_record(12, &body));
        }
        for cue in cues {
            out.extend(cue);
        }
        if let Some(id) = self.multitracks {
            let mut record = varint_record(1, id);
            record.extend(len_record(2, b"customer"));
            record.extend(len_record(3, &varint_record(1, 1_900_000_000)));
            out.extend(len_record(21, &record));
        }
        if let Some((scale, text)) = &self.key {
            let mut music = len_record(1, text.as_bytes());
            music.extend(len_record(3, &scale.to_message()));
            music.extend(len_record(4, &scale.to_message()));
            out.extend(len_record(23, &music));
        }
        if self.unknown {
            out.extend(varint_record(40, 7));
            out.extend(len_record(41, b"unknown message \x00\xff"));
            let mut fixed = Vec::new();
            super::push_varint(&mut fixed, 42 << 3 | 5);
            fixed.extend([1, 2, 3, 4]);
            out.extend(fixed);
        }
        out
    }
}
