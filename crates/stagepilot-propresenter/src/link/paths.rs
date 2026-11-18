//! The naming rules every host applies before it touches a file. A name is one path component and nothing else.

use super::{LinkError, SONGS_LIBRARY};

/// One path component: no separators, no `..`, no hidden names, no control characters.
fn plain_component(name: &str, what: &str) -> Result<(), LinkError> {
    let bad = name.is_empty()
        || name.len() > 200
        || name == "."
        || name == ".."
        || name.starts_with('.')
        || name.contains(['/', '\\'])
        || name.chars().any(|c| (c as u32) < 32 || c as u32 == 127);
    if bad {
        return Err(LinkError::BadPath(format!(
            "That {what} name is not allowed."
        )));
    }
    Ok(())
}

/// A library to read from: one plain component (ProPresenter's own libraries may hold more than letters and digits).
pub fn library_name(name: &str) -> Result<&str, LinkError> {
    plain_component(name, "library")?;
    Ok(name)
}

/// A song file: one plain component ending in `.pro`.
pub fn song_file_name(name: &str) -> Result<&str, LinkError> {
    plain_component(name, "file")?;
    if !name.ends_with(".pro") || name.len() <= 4 {
        return Err(LinkError::BadPath("Only .pro files can be used.".into()));
    }
    Ok(name)
}

/// A library folder StagePilot may make: letters, digits, spaces and dashes, starting with a letter or digit, and not `Songs`.
pub fn ensure_name(name: &str) -> Result<&str, LinkError> {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-');
    if !first_ok
        || !rest_ok
        || name.len() > 64
        || name != name.trim()
        || name.eq_ignore_ascii_case(SONGS_LIBRARY)
    {
        return Err(LinkError::BadPath(
            "A library name can have letters, digits, spaces and dashes, and it cannot be Songs."
                .into(),
        ));
    }
    Ok(name)
}

/// The name a song is imported under. ProPresenter cuts a name at its first dot, so there is none.
pub fn import_name(name: &str) -> Result<String, LinkError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > 120
        || name.contains(['.', '/', '\\'])
        || name.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
    {
        return Err(LinkError::BadPath(
            "The song name must be 1 to 120 characters, without dots or slashes.".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// A MultiTracks song number: digits only, 1 to 18 of them.
pub fn song_number(id: &str) -> Result<&str, LinkError> {
    if id.is_empty() || id.len() > 18 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(LinkError::BadPath(
            "The song number must be digits only.".into(),
        ));
    }
    Ok(id)
}

/// A playlist name typed into ProPresenter: text without control characters.
pub fn playlist_name(name: &str) -> Result<&str, LinkError> {
    if name.chars().count() > 120 || name.chars().any(|c| (c as u32) < 32 || c as u32 == 127) {
        return Err(LinkError::BadPath("The playlist name is not valid.".into()));
    }
    Ok(name)
}

/// A lower-case hex SHA-256.
pub fn sha256_hex(text: &str) -> Result<&str, LinkError> {
    if text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(text)
    } else {
        Err(LinkError::BadPath(
            "The checksum of the file is missing or not valid.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_with_separators_dots_or_control_characters_are_refused() {
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", ".hidden", "a\0b", "a\nb", "/abs",
        ] {
            assert!(library_name(bad).is_err(), "{bad:?}");
        }
        assert!(library_name("Songs").is_ok());
        assert!(library_name("Playback (old)").is_ok());
        assert!(song_file_name("Song (1).pro").is_ok());
        for bad in [
            "a.txt", ".pro", "x.PRO", "../a.pro", "a/b.pro", "a.pro/", "",
        ] {
            assert!(song_file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_library_can_be_made_only_with_letters_digits_spaces_and_dashes() {
        assert!(ensure_name("Playback").is_ok());
        assert!(ensure_name("Sunday Set-2").is_ok());
        for bad in [
            "Songs", "songs", "SONGS", "", " x", "x ", "-x", "a_b", "a.b", "a/b", "é",
        ] {
            assert!(ensure_name(bad).is_err(), "{bad:?}");
        }
        assert!(ensure_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn import_names_have_no_dots_and_numbers_only_digits() {
        assert_eq!(
            import_name(" Emptied (feat Tyana) ").unwrap(),
            "Emptied (feat Tyana)"
        );
        for bad in ["", "  ", "a.b", "a/b", "a\\b", "a\nb"] {
            assert!(import_name(bad).is_err(), "{bad:?}");
        }
        assert!(song_number("12345").is_ok());
        for bad in ["", "12a", "1 2", "-1", &"9".repeat(19)] {
            assert!(song_number(bad).is_err(), "{bad:?}");
        }
        assert!(playlist_name("Sunday").is_ok());
        assert!(playlist_name("a\nb").is_err());
        assert!(sha256_hex(&"a".repeat(64)).is_ok());
        assert!(sha256_hex(&"A".repeat(64)).is_err());
        assert!(sha256_hex("abc").is_err());
    }
}
