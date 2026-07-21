//! Strict transcription-language selection for the local three-language setup.

use log::{debug, warn};
use std::collections::HashSet;

pub const FOLLOW_KEYBOARD_LANGUAGE: &str = "follow_keyboard";
pub const ENGLISH_LANGUAGE: &str = "en-US";
pub const GERMAN_LANGUAGE: &str = "de-DE";
pub const BULGARIAN_LANGUAGE: &str = "bg-BG";

pub fn is_allowed_persisted_language(language: &str) -> bool {
    matches!(
        language,
        FOLLOW_KEYBOARD_LANGUAGE | ENGLISH_LANGUAGE | GERMAN_LANGUAGE | BULGARIAN_LANGUAGE
    )
}

/// Fold legacy settings into the strict language set. Unknown values, including
/// the old unrestricted `auto`, become keyboard-following mode.
pub fn normalize_persisted_language(language: &str) -> &'static str {
    match language {
        FOLLOW_KEYBOARD_LANGUAGE => FOLLOW_KEYBOARD_LANGUAGE,
        "en" | ENGLISH_LANGUAGE => ENGLISH_LANGUAGE,
        "de" | GERMAN_LANGUAGE => GERMAN_LANGUAGE,
        "bg" | BULGARIAN_LANGUAGE => BULGARIAN_LANGUAGE,
        _ => FOLLOW_KEYBOARD_LANGUAGE,
    }
}

/// Resolve and freeze the language to use for one recording.
pub fn language_for_recording(persisted_language: &str) -> String {
    if persisted_language != FOLLOW_KEYBOARD_LANGUAGE {
        return normalize_persisted_language(persisted_language).to_string();
    }

    match current_keyboard_input_source()
        .as_deref()
        .and_then(language_for_input_source)
    {
        Some(language) => {
            debug!(
                "Keyboard-follow language snapshot resolved to '{}'",
                language
            );
            language.to_string()
        }
        None => {
            warn!(
                "Current keyboard layout is not ABC, German, or Bulgarian Phonetic; \
                 using English for this recording"
            );
            ENGLISH_LANGUAGE.to_string()
        }
    }
}

/// Freeze the ordered languages to try for one recording. Explicit language
/// choices remain single-shot. Keyboard-following mode starts with the active
/// keyboard, then adds only the other supported languages represented by the
/// user's enabled macOS keyboard input sources.
pub fn language_candidates_for_recording(persisted_language: &str) -> Vec<String> {
    if persisted_language != FOLLOW_KEYBOARD_LANGUAGE {
        return vec![language_for_recording(persisted_language)];
    }

    let enabled_sources = enabled_keyboard_input_sources();
    language_candidates_from_sources(current_keyboard_input_source().as_deref(), &enabled_sources)
}

fn language_candidates_from_sources(
    active_source: Option<&str>,
    enabled_sources: &[String],
) -> Vec<String> {
    let primary = active_source
        .and_then(language_for_input_source)
        .or_else(|| {
            enabled_sources
                .iter()
                .find_map(|source| language_for_input_source(source))
        })
        .unwrap_or(ENGLISH_LANGUAGE);

    ordered_language_candidates(primary, enabled_sources.iter())
}

fn ordered_language_candidates<'a>(
    primary: &str,
    input_sources: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut languages = Vec::with_capacity(3);
    let mut add = |language: &str| {
        if seen.insert(language.to_string()) {
            languages.push(language.to_string());
        }
    };

    add(primary);
    for source in input_sources {
        if let Some(language) = language_for_input_source(source) {
            add(language);
        }
    }
    languages
}

fn language_for_input_source(source: &str) -> Option<&'static str> {
    let normalized = source.to_ascii_lowercase();
    let has_source = |name: &str| {
        normalized
            .split_whitespace()
            .any(|part| part == name || part.ends_with(&format!(".{name}")))
    };
    let compact: String = normalized
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();

    if has_source("abc") {
        Some(ENGLISH_LANGUAGE)
    } else if has_source("german") {
        Some(GERMAN_LANGUAGE)
    } else if compact.contains("bulgarian") && compact.contains("phonetic") {
        Some(BULGARIAN_LANGUAGE)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos_input_sources {
    use std::ffi::{c_char, c_void, CStr};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type TISInputSourceRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
        fn TISCreateInputSourceList(
            properties: CFDictionaryRef,
            include_all_installed: bool,
        ) -> CFArrayRef;
        fn TISGetInputSourceProperty(
            input_source: TISInputSourceRef,
            property_key: CFStringRef,
        ) -> CFTypeRef;
        static kTISPropertyInputSourceID: CFStringRef;
        static kTISPropertyLocalizedName: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: CFTypeRef);
        fn CFArrayGetCount(array: CFArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
        fn CFStringGetLength(value: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            value: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
    }

    unsafe fn string_property(source: TISInputSourceRef, property: CFStringRef) -> Option<String> {
        let value = unsafe { TISGetInputSourceProperty(source, property) } as CFStringRef;
        if value.is_null() {
            return None;
        }

        let length = unsafe { CFStringGetLength(value) };
        let capacity =
            unsafe { CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) }
                .checked_add(1)?;
        let mut buffer = vec![0_u8; usize::try_from(capacity).ok()?];
        if !unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr().cast(),
                capacity,
                K_CF_STRING_ENCODING_UTF8,
            )
        } {
            return None;
        }

        unsafe { CStr::from_ptr(buffer.as_ptr().cast()) }
            .to_str()
            .ok()
            .map(ToOwned::to_owned)
    }

    unsafe fn source_description(source: TISInputSourceRef) -> Option<String> {
        let id = unsafe { string_property(source, kTISPropertyInputSourceID) };
        let name = unsafe { string_property(source, kTISPropertyLocalizedName) };
        match (id, name) {
            (Some(id), Some(name)) => Some(format!("{id} {name}")),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        }
    }

    pub(super) fn current() -> Option<String> {
        unsafe {
            let source = TISCopyCurrentKeyboardInputSource();
            if source.is_null() {
                return None;
            }
            let result = source_description(source);
            CFRelease(source);
            result
        }
    }

    pub(super) fn enabled() -> Vec<String> {
        unsafe {
            // Passing include_all_installed=false restricts the result to input
            // sources the user enabled in Keyboard settings.
            let sources = TISCreateInputSourceList(std::ptr::null(), false);
            if sources.is_null() {
                return Vec::new();
            }

            let count = CFArrayGetCount(sources);
            let mut descriptions = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
            for index in 0..count {
                let source = CFArrayGetValueAtIndex(sources, index) as TISInputSourceRef;
                if !source.is_null() {
                    if let Some(description) = source_description(source) {
                        descriptions.push(description);
                    }
                }
            }
            CFRelease(sources);
            descriptions
        }
    }
}

#[cfg(target_os = "macos")]
fn current_keyboard_input_source() -> Option<String> {
    macos_input_sources::current()
}

#[cfg(target_os = "macos")]
fn enabled_keyboard_input_sources() -> Vec<String> {
    macos_input_sources::enabled()
}

#[cfg(not(target_os = "macos"))]
fn current_keyboard_input_source() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn enabled_keyboard_input_sources() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_macos_input_sources() {
        assert_eq!(
            language_for_input_source("com.apple.keylayout.ABC"),
            Some(ENGLISH_LANGUAGE)
        );
        assert_eq!(
            language_for_input_source("com.apple.keylayout.German"),
            Some(GERMAN_LANGUAGE)
        );
        assert_eq!(
            language_for_input_source("Bulgarian – Phonetic"),
            Some(BULGARIAN_LANGUAGE)
        );
        assert_eq!(
            language_for_input_source("com.apple.keylayout.Bulgarian-Phonetic"),
            Some(BULGARIAN_LANGUAGE)
        );
        assert_eq!(
            language_for_input_source("com.apple.keylayout.ABC ABC"),
            Some(ENGLISH_LANGUAGE)
        );
    }

    #[test]
    fn rejects_unrestricted_and_unrelated_languages() {
        assert_eq!(
            normalize_persisted_language("auto"),
            FOLLOW_KEYBOARD_LANGUAGE
        );
        assert_eq!(normalize_persisted_language("fr"), FOLLOW_KEYBOARD_LANGUAGE);
        assert_eq!(language_for_input_source("French"), None);
    }

    #[test]
    fn retry_candidates_follow_active_then_enabled_keyboards_only() {
        let sources = vec![
            "com.apple.keylayout.German German".to_string(),
            "French".to_string(),
            "com.apple.keylayout.ABC ABC".to_string(),
            "Bulgarian - Phonetic".to_string(),
            "com.apple.keylayout.German German".to_string(),
        ];

        assert_eq!(
            ordered_language_candidates(GERMAN_LANGUAGE, sources.iter()),
            vec![GERMAN_LANGUAGE, ENGLISH_LANGUAGE, BULGARIAN_LANGUAGE]
        );
    }

    #[test]
    fn explicit_language_has_no_retry_candidates() {
        assert_eq!(
            language_candidates_for_recording(BULGARIAN_LANGUAGE),
            vec![BULGARIAN_LANGUAGE]
        );
    }

    #[test]
    fn unsupported_active_layout_uses_an_enabled_supported_keyboard() {
        let sources = vec![
            "French".to_string(),
            "com.apple.keylayout.German German".to_string(),
            "Bulgarian - Phonetic".to_string(),
        ];

        assert_eq!(
            language_candidates_from_sources(Some("French"), &sources),
            vec![GERMAN_LANGUAGE, BULGARIAN_LANGUAGE]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_current_macos_input_source() {
        assert!(current_keyboard_input_source().is_some());
        assert!(!enabled_keyboard_input_sources().is_empty());
    }
}
