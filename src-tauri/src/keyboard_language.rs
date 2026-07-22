//! Transcription-language selection that can follow the active keyboard layout.

use log::{debug, warn};
use std::collections::HashSet;

pub const FOLLOW_KEYBOARD_LANGUAGE: &str = "follow_keyboard";
pub const AUTO_LANGUAGE: &str = "auto";
/// Fallback locale used when no keyboard layout maps to a usable language.
pub const ENGLISH_LANGUAGE: &str = "en-US";

#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyboardInputSource {
    description: String,
    languages: Vec<String>,
    fallback_language: Option<String>,
}

#[cfg(test)]
impl KeyboardInputSource {
    fn test(description: &str, languages: &[&str]) -> Self {
        Self {
            description: description.to_string(),
            languages: languages
                .iter()
                .map(|language| language.to_string())
                .collect(),
            fallback_language: None,
        }
    }

    fn test_with_fallback(description: &str, fallback_language: &str) -> Self {
        Self {
            description: description.to_string(),
            languages: Vec::new(),
            fallback_language: Some(fallback_language.to_string()),
        }
    }
}

/// Keyboard-following mode, automatic detection, and any well-formed BCP-47 tag
/// are all valid persisted choices.
pub fn is_allowed_persisted_language(language: &str) -> bool {
    matches!(language, FOLLOW_KEYBOARD_LANGUAGE | AUTO_LANGUAGE)
        || canonicalize_bcp47(language).is_some()
}

/// Normalize a persisted language for use. Keyboard-following mode and
/// automatic detection pass through untouched, explicit locales are
/// canonicalized, and only genuinely malformed values fall back to automatic
/// detection.
pub fn normalize_persisted_language(language: &str) -> String {
    match language {
        FOLLOW_KEYBOARD_LANGUAGE => FOLLOW_KEYBOARD_LANGUAGE.to_string(),
        AUTO_LANGUAGE => AUTO_LANGUAGE.to_string(),
        other => canonicalize_bcp47(other).unwrap_or_else(|| AUTO_LANGUAGE.to_string()),
    }
}

/// Resolve and freeze the language to use for one recording.
pub fn language_for_recording(persisted_language: &str) -> String {
    if persisted_language != FOLLOW_KEYBOARD_LANGUAGE {
        return normalize_persisted_language(persisted_language);
    }

    match current_keyboard_input_source()
        .as_ref()
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
                "Current keyboard input source does not map to a supported language; \
                 using the configured English fallback for this recording"
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
    language_candidates_from_sources(current_keyboard_input_source().as_ref(), &enabled_sources)
}

fn language_candidates_from_sources(
    active_source: Option<&KeyboardInputSource>,
    enabled_sources: &[KeyboardInputSource],
) -> Vec<String> {
    let primary = active_source
        .and_then(language_for_input_source)
        .or_else(|| {
            enabled_sources
                .iter()
                .find_map(|source| language_for_input_source(source))
        })
        .unwrap_or_else(|| ENGLISH_LANGUAGE.to_string());

    ordered_language_candidates(&primary, enabled_sources.iter())
}

fn ordered_language_candidates<'a>(
    primary: &str,
    input_sources: impl IntoIterator<Item = &'a KeyboardInputSource>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut languages = Vec::with_capacity(3);
    let mut add = |language: String| {
        let base = language
            .split('-')
            .next()
            .unwrap_or(&language)
            .to_ascii_lowercase();
        if seen.insert(base) {
            languages.push(language);
        }
    };

    add(primary.to_string());
    for source in input_sources {
        if let Some(language) = language_for_input_source(source) {
            add(language);
        }
    }
    languages
}

fn language_for_input_source(source: &KeyboardInputSource) -> Option<String> {
    match source.languages.first().map(|language| language.trim()) {
        Some(language) if !language.is_empty() => canonicalize_bcp47(language),
        _ => source
            .fallback_language
            .as_deref()
            .and_then(canonicalize_bcp47),
    }
}

fn canonicalize_bcp47(language: &str) -> Option<String> {
    let normalized = language.trim().replace('_', "-");
    let mut subtags = normalized.split('-');
    let primary = subtags.next()?;
    if !(2..=8).contains(&primary.len())
        || !primary
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }

    let mut canonical = primary.to_ascii_lowercase();
    for subtag in subtags {
        if !(1..=8).contains(&subtag.len())
            || !subtag
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        {
            return None;
        }
        canonical.push('-');
        canonical.push_str(subtag);
    }
    Some(canonical)
}

#[cfg(target_os = "macos")]
mod macos_input_sources {
    use super::KeyboardInputSource;
    use std::ffi::{c_char, c_void, CStr, CString};

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
        static kTISPropertyInputSourceLanguages: CFStringRef;
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
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            value: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFLocaleCreateCanonicalLanguageIdentifierFromString(
            allocator: CFTypeRef,
            locale_identifier: CFStringRef,
        ) -> CFStringRef;
    }

    unsafe fn cf_string(value: CFStringRef) -> Option<String> {
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

    unsafe fn string_property(source: TISInputSourceRef, property: CFStringRef) -> Option<String> {
        let value = unsafe { TISGetInputSourceProperty(source, property) } as CFStringRef;
        unsafe { cf_string(value) }
    }

    unsafe fn string_property_from_rust(value: &str) -> Option<CFStringRef> {
        let value = CString::new(value).ok()?;
        let string = unsafe {
            CFStringCreateWithCString(std::ptr::null(), value.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        (!string.is_null()).then_some(string)
    }

    unsafe fn string_array_property(
        source: TISInputSourceRef,
        property: CFStringRef,
    ) -> Vec<String> {
        let values = unsafe { TISGetInputSourceProperty(source, property) } as CFArrayRef;
        if values.is_null() {
            return Vec::new();
        }

        let count = unsafe { CFArrayGetCount(values) };
        let mut strings = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
        for index in 0..count {
            let value = unsafe { CFArrayGetValueAtIndex(values, index) } as CFStringRef;
            if let Some(value) = unsafe { cf_string(value) } {
                strings.push(value);
            }
        }
        strings
    }

    unsafe fn source_info(source: TISInputSourceRef) -> Option<KeyboardInputSource> {
        let id = unsafe { string_property(source, kTISPropertyInputSourceID) };
        let name = unsafe { string_property(source, kTISPropertyLocalizedName) };
        let description = match (&id, &name) {
            (Some(id), Some(name)) => Some(format!("{id} {name}")),
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (None, None) => None,
        }
        .unwrap_or_default();
        let languages = unsafe { string_array_property(source, kTISPropertyInputSourceLanguages) };
        let fallback_language = name.as_deref().and_then(|name| unsafe {
            let name = string_property_from_rust(name)?;
            let canonical =
                CFLocaleCreateCanonicalLanguageIdentifierFromString(std::ptr::null(), name);
            CFRelease(name);
            if canonical.is_null() {
                return None;
            }
            let result = cf_string(canonical);
            CFRelease(canonical);
            result
        });

        if description.is_empty() && languages.is_empty() {
            None
        } else {
            Some(KeyboardInputSource {
                description,
                languages,
                fallback_language,
            })
        }
    }

    pub(super) fn current() -> Option<KeyboardInputSource> {
        unsafe {
            let source = TISCopyCurrentKeyboardInputSource();
            if source.is_null() {
                return None;
            }
            let result = source_info(source);
            CFRelease(source);
            result
        }
    }

    pub(super) fn enabled() -> Vec<KeyboardInputSource> {
        unsafe {
            // Passing include_all_installed=false restricts the result to input
            // sources the user enabled in Keyboard settings.
            let sources = TISCreateInputSourceList(std::ptr::null(), false);
            if sources.is_null() {
                return Vec::new();
            }

            let count = CFArrayGetCount(sources);
            let mut input_sources = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
            for index in 0..count {
                let source = CFArrayGetValueAtIndex(sources, index) as TISInputSourceRef;
                if !source.is_null() {
                    if let Some(input_source) = source_info(source) {
                        input_sources.push(input_source);
                    }
                }
            }
            CFRelease(sources);
            input_sources
        }
    }
}

#[cfg(target_os = "macos")]
fn current_keyboard_input_source() -> Option<KeyboardInputSource> {
    macos_input_sources::current()
}

#[cfg(target_os = "macos")]
fn enabled_keyboard_input_sources() -> Vec<KeyboardInputSource> {
    macos_input_sources::enabled()
}

#[cfg(not(target_os = "macos"))]
fn current_keyboard_input_source() -> Option<KeyboardInputSource> {
    None
}

#[cfg(not(target_os = "macos"))]
fn enabled_keyboard_input_sources() -> Vec<KeyboardInputSource> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_macos_input_sources() {
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test(
                "renamed third-party layout",
                &["en-US"],
            )),
            Some(ENGLISH_LANGUAGE.to_string())
        );
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test("anything", &["de"])),
            Some("de".to_string())
        );
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test("anything", &["bg-BG"])),
            Some("bg-BG".to_string())
        );
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test("anything", &["fr-FR"])),
            Some("fr-FR".to_string())
        );
        assert_eq!(canonicalize_bcp47("es_419"), Some("es-419".to_string()));
        assert_eq!(canonicalize_bcp47("sr-Latn"), Some("sr-Latn".to_string()));
        assert_eq!(
            canonicalize_bcp47("en-u-nu-latn"),
            Some("en-u-nu-latn".to_string())
        );
    }

    #[test]
    fn uses_generic_canonicalized_name_fallback_only_without_metadata() {
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test(
                "com.apple.keylayout.German",
                &["fr"],
            )),
            Some("fr".to_string())
        );
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test_with_fallback(
                "renamed third-party layout",
                "uk-UA",
            )),
            Some("uk-UA".to_string())
        );
        assert_eq!(
            language_for_input_source(&KeyboardInputSource::test("unrecognized layout", &[])),
            None
        );
    }

    #[test]
    fn normalization_preserves_every_valid_persisted_choice() {
        // Automatic detection and keyboard-following mode are both first-class
        // choices and must survive a settings load untouched.
        assert_eq!(normalize_persisted_language(AUTO_LANGUAGE), AUTO_LANGUAGE);
        assert_eq!(
            normalize_persisted_language(FOLLOW_KEYBOARD_LANGUAGE),
            FOLLOW_KEYBOARD_LANGUAGE
        );

        // An explicit language the user picked is never rewritten, whether it is
        // a bare code or a full locale.
        assert_eq!(normalize_persisted_language("fr"), "fr");
        assert_eq!(normalize_persisted_language("es"), "es");
        assert_eq!(normalize_persisted_language("bg-BG"), "bg-BG");
        assert_eq!(normalize_persisted_language("zh-Hans"), "zh-Hans");
        assert_eq!(normalize_persisted_language("en_US"), "en-US");

        // Only genuinely malformed values fall back, and they fall back to
        // automatic detection rather than to keyboard-following mode.
        assert_eq!(normalize_persisted_language(""), AUTO_LANGUAGE);
        assert_eq!(normalize_persisted_language("!!"), AUTO_LANGUAGE);

        assert!(is_allowed_persisted_language(AUTO_LANGUAGE));
        assert!(is_allowed_persisted_language(FOLLOW_KEYBOARD_LANGUAGE));
        assert!(is_allowed_persisted_language("es"));
        assert!(!is_allowed_persisted_language("!!"));
    }

    #[test]
    fn retry_candidates_follow_active_then_enabled_keyboards_only() {
        let sources = vec![
            KeyboardInputSource::test("renamed layout", &["de-DE"]),
            KeyboardInputSource::test("French", &["fr"]),
            KeyboardInputSource::test("another renamed layout", &["en"]),
            KeyboardInputSource::test("third-party layout", &["bg"]),
            KeyboardInputSource::test("duplicate German", &["de"]),
        ];

        assert_eq!(
            ordered_language_candidates("de-DE", sources.iter()),
            vec!["de-DE", "fr", "en", "bg"]
        );
    }

    #[test]
    fn explicit_language_has_no_retry_candidates() {
        assert_eq!(language_candidates_for_recording("bg-BG"), vec!["bg-BG"]);
    }

    #[test]
    fn candidates_remain_general_until_the_model_intersection() {
        let sources = vec![
            KeyboardInputSource::test("French", &["fr"]),
            KeyboardInputSource::test("renamed German", &["de"]),
            KeyboardInputSource::test("renamed Bulgarian", &["bg"]),
        ];

        assert_eq!(
            language_candidates_from_sources(
                Some(&KeyboardInputSource::test("French", &["fr"])),
                &sources,
            ),
            vec!["fr", "de", "bg"]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_current_macos_input_source() {
        let current = current_keyboard_input_source().expect("current keyboard input source");
        assert!(!current.languages.is_empty());

        let enabled = enabled_keyboard_input_sources();
        assert!(!enabled.is_empty());
        assert!(enabled
            .iter()
            .any(|source| language_for_input_source(source).is_some()));
    }
}
