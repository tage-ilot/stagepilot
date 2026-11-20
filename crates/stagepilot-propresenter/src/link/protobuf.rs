//! A small streaming reader for the one thing StagePilot needs from a ProPresenter song file: its MultiTracks song number,
//! `Presentation.multi_tracks_licensing.song_identifier`. It reads the protobuf wire format only (no protobuf library, no
//! schema): `Presentation` is field 21 of the file and `MultiTracksLicensing.song_identifier` is its field 1 (see
//! `proto/presentation.proto` of ProPresenter 19). Every other field is skipped by seeking, so a song of any size costs a
//! few small reads and is never loaded into memory.
//!
//! A file without the record, a file that is not a protobuf and a truncated file all give `None`, never an error.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// `Presentation.multi_tracks_licensing`.
const LICENSING_FIELD: u64 = 21;
/// `MultiTracksLicensing.song_identifier`.
const SONG_IDENTIFIER_FIELD: u64 = 1;
/// The licence record is a few dozen bytes; a bigger one is not what it should be.
const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// A song file larger than this is not read.
const MAX_FILE_BYTES: u64 = 40 * 1024 * 1024;

/// The song number of a `.pro` file on disk.
pub fn multitracks_id_of_file(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size > MAX_FILE_BYTES {
        return None;
    }
    multitracks_id(std::io::BufReader::new(file), size)
}

/// The song number in `size` bytes of protobuf read from `source`.
pub fn multitracks_id<R: Read + Seek>(mut source: R, size: u64) -> Option<u64> {
    let mut position = 0u64;
    while position < size {
        let (tag, used) = read_varint(&mut source)?;
        position += used;
        let (field, wire) = (tag >> 3, tag & 7);
        if field == 0 {
            return None;
        }
        match wire {
            0 => position += read_varint(&mut source)?.1,
            1 | 5 => {
                let skip = if wire == 1 { 8 } else { 4 };
                position += skip;
                source.seek(SeekFrom::Current(skip as i64)).ok()?;
            }
            2 => {
                let (length, used) = read_varint(&mut source)?;
                position += used;
                if length > size.saturating_sub(position) {
                    return None;
                }
                if field != LICENSING_FIELD {
                    position += length;
                    source
                        .seek(SeekFrom::Current(i64::try_from(length).ok()?))
                        .ok()?;
                    continue;
                }
                if length > MAX_RECORD_BYTES {
                    return None;
                }
                let mut record = vec![0u8; length as usize];
                source.read_exact(&mut record).ok()?;
                return song_identifier(&record);
            }
            _ => return None,
        }
    }
    None
}

/// Field 1 (a varint) of the licence record.
fn song_identifier(record: &[u8]) -> Option<u64> {
    let mut at = 0usize;
    while at < record.len() {
        let (tag, used) = varint_at(record, at)?;
        at += used;
        let (field, wire) = (tag >> 3, tag & 7);
        match wire {
            0 => {
                let (value, used) = varint_at(record, at)?;
                at += used;
                if field == SONG_IDENTIFIER_FIELD {
                    // An int64: a negative number or zero is not a song.
                    return (value > 0 && value < (1 << 63)).then_some(value);
                }
            }
            1 => at += 8,
            5 => at += 4,
            2 => {
                let (length, used) = varint_at(record, at)?;
                at += used;
                at = at.checked_add(usize::try_from(length).ok()?)?;
            }
            _ => return None,
        }
    }
    None
}

/// One varint from a stream: (value, bytes used). `None` at the end of the file or for one longer than 10 bytes.
fn read_varint<R: Read>(source: &mut R) -> Option<(u64, u64)> {
    let mut value = 0u64;
    for index in 0..10u32 {
        let mut byte = [0u8; 1];
        source.read_exact(&mut byte).ok()?;
        value |= u64::from(byte[0] & 0x7f).checked_shl(7 * index)?;
        if byte[0] & 0x80 == 0 {
            return Some((value, u64::from(index) + 1));
        }
    }
    None
}

fn varint_at(data: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for index in 0..10usize {
        let byte = *data.get(start + index)?;
        value |= u64::from(byte & 0x7f).checked_shl(7 * index as u32)?;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    pub(crate) fn varint(mut value: u64) -> Vec<u8> {
        let mut out = vec![];
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn field(number: u64, wire: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = varint(number << 3 | wire);
        out.extend_from_slice(payload);
        out
    }

    fn length_delimited(number: u64, body: &[u8]) -> Vec<u8> {
        let mut payload = varint(body.len() as u64);
        payload.extend_from_slice(body);
        field(number, 2, &payload)
    }

    /// A tiny synthetic `.pro` (never a real song): some other fields, then the licence record.
    pub(crate) fn song_with_id(id: u64) -> Vec<u8> {
        let mut out = vec![];
        out.extend(length_delimited(1, b"application info"));
        out.extend(length_delimited(2, &vec![7u8; 200_000]));
        out.extend(field(3, 0, &varint(300)));
        out.extend(field(4, 1, &[1; 8]));
        out.extend(field(5, 5, &[2; 4]));
        let mut record = field(2, 0, &varint(99));
        record.extend(field(1, 0, &varint(id)));
        out.extend(length_delimited(LICENSING_FIELD, &record));
        out.extend(length_delimited(22, b"after"));
        out
    }

    fn read(bytes: &[u8]) -> Option<u64> {
        multitracks_id(Cursor::new(bytes.to_vec()), bytes.len() as u64)
    }

    #[test]
    fn a_valid_licence_record_gives_its_song_number() {
        assert_eq!(read(&song_with_id(1_234_567)), Some(1_234_567));
        assert_eq!(read(&song_with_id(1)), Some(1));
        assert_eq!(read(&song_with_id((1 << 62) + 5)), Some((1 << 62) + 5));
    }

    #[test]
    fn a_song_without_the_record_or_with_an_empty_one_has_none() {
        let mut without = length_delimited(1, b"x");
        without.extend(length_delimited(22, b"y"));
        assert_eq!(read(&without), None);
        assert_eq!(read(&length_delimited(LICENSING_FIELD, &[])), None);
        let other_field_only = length_delimited(LICENSING_FIELD, &field(2, 0, &varint(5)));
        assert_eq!(read(&other_field_only), None);
        assert_eq!(read(&song_with_id(0)), None, "zero is not a song");
        assert_eq!(read(&[]), None);
    }

    #[test]
    fn garbage_and_truncated_files_have_none_and_never_panic() {
        assert_eq!(read(b"this is not a protobuf at all\n"), None);
        assert_eq!(read(&[0xff; 64]), None, "a varint that never ends");
        assert_eq!(read(&[0x00, 0x01]), None, "field number 0");
        assert_eq!(read(&[0x0b, 0x01]), None, "a group start");
        let whole = song_with_id(42);
        for cut in [1, 5, 100, 200_020, whole.len() - 3, whole.len() - 12] {
            // Truncated anywhere before the end of the licence record: no answer (or the right one when the record is whole).
            let found = read(&whole[..cut]);
            assert!(
                found.is_none() || found == Some(42),
                "cut at {cut}: {found:?}"
            );
        }
        assert_eq!(read(&whole[..200_020]), None);
        // A length that runs past the end of the file.
        let mut lying = varint(LICENSING_FIELD << 3 | 2);
        lying.extend(varint(1_000_000));
        lying.extend([8, 5]);
        assert_eq!(read(&lying), None);
        // A record far bigger than a licence record.
        let big = length_delimited(LICENSING_FIELD, &vec![0u8; 70_000]);
        assert_eq!(read(&big), None);
    }

    #[test]
    fn a_file_on_disk_is_read_without_loading_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pro");
        std::fs::write(&path, song_with_id(777)).unwrap();
        assert_eq!(multitracks_id_of_file(&path), Some(777));
        assert_eq!(
            multitracks_id_of_file(&dir.path().join("missing.pro")),
            None
        );
    }
}
