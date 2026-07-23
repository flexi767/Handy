//! Resolve the transcription language from the active macOS keyboard layout.
//!
//! The user can persist the sentinel [`FOLLOW_KEYBOARD_LANGUAGE`] instead of a
//! concrete language. When they do, the language is snapshotted from the active
//! keyboard input source at the moment a recording starts and handed to the
//! normal model-language resolution (`managers::model::effective_language`),
//! which intersects it with the selected model's supported languages. Nothing
//! here hardcodes a language: whatever BCP-47 tag the keyboard advertises is
//! passed through, and unsupported tags fall back exactly as an explicit choice
//! would.

#[cfg(target_os = "macos")]
use log::debug;

/// Persisted `selected_language` value meaning "use the active keyboard layout".
pub const FOLLOW_KEYBOARD_LANGUAGE: &str = "follow_keyboard";

/// A keyboard input source and the languages it advertises. `languages` holds
/// the BCP-47 tags macOS reports for the layout (e.g. `["de"]`, `["bg-BG"]`);
/// `fallback_language` is the canonical language identifier derived from the
/// source's localized name, used only when `languages` is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyboardInputSource {
    languages: Vec<String>,
    fallback_language: Option<String>,
}

impl KeyboardInputSource {
    /// The primary language this source represents, as a lowercased base subtag
    /// (region and script dropped): `"en-US"` → `"en"`, `"zh-Hans"` → `"zh"`.
    fn primary_language(&self) -> Option<String> {
        self.languages
            .first()
            .map(String::as_str)
            .or(self.fallback_language.as_deref())
            .and_then(primary_subtag)
    }
}

/// Resolve a persisted `selected_language` for one recording against a specific
/// model's capabilities.
///
/// Keyboard-following means "transcribe whichever of my keyboard languages I am
/// speaking", not "assume I am speaking the layout that happens to be active".
/// Forcing the active layout is actively harmful when it does not match the
/// speech: Whisper *translates* into the forced language rather than
/// transcribing. So a model that can auto-detect is left on `"auto"`, and the
/// result is constrained to the enabled keyboard languages afterwards by
/// wrong-language recovery. Only a must-pick model (no detection) falls back to
/// forcing the active layout, since it has to be told something.
pub fn resolve_language_intent_for_model(
    persisted_language: &str,
    supports_language_detection: bool,
) -> String {
    if persisted_language == FOLLOW_KEYBOARD_LANGUAGE && supports_language_detection {
        return "auto".to_string();
    }
    resolve_language_intent(persisted_language)
}

/// Resolve a persisted `selected_language` for one recording. Concrete choices
/// (including `"auto"`) pass through untouched; [`FOLLOW_KEYBOARD_LANGUAGE`]
/// snapshots the active keyboard and resolves to its base language subtag, or
/// to `"auto"` when the keyboard advertises nothing usable.
pub fn resolve_language_intent(persisted_language: &str) -> String {
    if persisted_language != FOLLOW_KEYBOARD_LANGUAGE {
        return persisted_language.to_string();
    }

    match active_keyboard_language() {
        Some(language) => {
            #[cfg(target_os = "macos")]
            debug!("Follow-keyboard resolved active layout to '{}'", language);
            language
        }
        None => "auto".to_string(),
    }
}

/// The active keyboard layout's base language subtag, if one is available.
fn active_keyboard_language() -> Option<String> {
    current_keyboard_input_source().and_then(|source| source.primary_language())
}

/// Like [`enabled_keyboard_languages`], but guaranteed to run the Carbon
/// input-source *enumeration* on the main dispatch queue. `TISCreateInputSourceList`
/// calls `dispatch_assert_queue(dispatch_get_main_queue())` and aborts the
/// process on any other queue, and callers such as `transcribe` run on a
/// `spawn_blocking` worker — so hop onto the real main queue via libdispatch and
/// block for the result. Safe from any non-main thread: the main run loop
/// services the main queue, so this cannot deadlock.
#[cfg(target_os = "macos")]
pub fn enabled_keyboard_languages_on_main() -> Vec<String> {
    extern "C" {
        fn pthread_main_np() -> std::os::raw::c_int;
    }

    // Already on the main thread (e.g. the headless CLI transcribe path): the
    // enumeration is safe to run directly, and dispatching to the main queue
    // while blocking here would deadlock when no run loop is draining it.
    if unsafe { pthread_main_np() } != 0 {
        return enabled_keyboard_languages();
    }

    let (tx, rx) = std::sync::mpsc::channel();
    dispatch::Queue::main().exec_async(move || {
        let _ = tx.send(enabled_keyboard_languages());
    });
    rx.recv().unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub fn enabled_keyboard_languages_on_main() -> Vec<String> {
    enabled_keyboard_languages()
}

/// The ordered, de-duplicated base language subtags represented by the user's
/// enabled keyboard layouts, active layout first. Used by wrong-keyboard
/// recovery to know which languages the user actually types in. Must run on the
/// main dispatch queue on macOS (see [`enabled_keyboard_languages_on_main`]).
pub fn enabled_keyboard_languages() -> Vec<String> {
    let mut languages = Vec::new();
    let mut push = |language: Option<String>| {
        if let Some(language) = language {
            if !languages.contains(&language) {
                languages.push(language);
            }
        }
    };

    push(active_keyboard_language());
    for source in enabled_keyboard_input_sources() {
        push(source.primary_language());
    }
    languages
}

/// Lowercase a BCP-47 tag and keep only its primary language subtag. Returns
/// `None` for anything that is not a 2–3 letter language code.
pub(crate) fn primary_subtag(language: &str) -> Option<String> {
    let primary = language.trim().replace('_', "-");
    let primary = primary.split('-').next()?.trim();
    if (2..=3).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(primary.to_ascii_lowercase())
    } else {
        None
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

    unsafe fn canonical_language_from_name(name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let name = unsafe {
            CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        if name.is_null() {
            return None;
        }
        let canonical =
            unsafe { CFLocaleCreateCanonicalLanguageIdentifierFromString(std::ptr::null(), name) };
        unsafe { CFRelease(name) };
        if canonical.is_null() {
            return None;
        }
        let result = unsafe { cf_string(canonical) };
        unsafe { CFRelease(canonical) };
        result
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
        let languages = unsafe { string_array_property(source, kTISPropertyInputSourceLanguages) };
        let fallback_language = unsafe { string_property(source, kTISPropertyLocalizedName) }
            .and_then(|name| unsafe { canonical_language_from_name(&name) });

        if languages.is_empty() && fallback_language.is_none() {
            None
        } else {
            Some(KeyboardInputSource {
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
            // include_all_installed=false restricts the list to the layouts the
            // user actually enabled in Keyboard settings.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn source(languages: &[&str], fallback: Option<&str>) -> KeyboardInputSource {
        KeyboardInputSource {
            languages: languages.iter().map(|l| l.to_string()).collect(),
            fallback_language: fallback.map(str::to_string),
        }
    }

    #[test]
    fn primary_subtag_strips_region_and_script() {
        assert_eq!(primary_subtag("en-US").as_deref(), Some("en"));
        assert_eq!(primary_subtag("bg-BG").as_deref(), Some("bg"));
        assert_eq!(primary_subtag("zh-Hans").as_deref(), Some("zh"));
        assert_eq!(primary_subtag("DE").as_deref(), Some("de"));
        assert_eq!(primary_subtag("de_DE").as_deref(), Some("de"));
    }

    #[test]
    fn primary_subtag_rejects_non_language_codes() {
        assert_eq!(primary_subtag(""), None);
        assert_eq!(primary_subtag("-"), None);
        assert_eq!(primary_subtag("123"), None);
        assert_eq!(primary_subtag("english"), None);
    }

    #[test]
    fn primary_language_prefers_advertised_languages_over_name_fallback() {
        assert_eq!(
            source(&["fr-FR"], Some("de")).primary_language().as_deref(),
            Some("fr")
        );
        assert_eq!(
            source(&[], Some("uk")).primary_language().as_deref(),
            Some("uk")
        );
        assert_eq!(source(&[], None).primary_language(), None);
    }

    #[test]
    fn resolve_passes_concrete_choices_through_untouched() {
        assert_eq!(resolve_language_intent("auto"), "auto");
        assert_eq!(resolve_language_intent("de-DE"), "de-DE");
        assert_eq!(resolve_language_intent("en"), "en");
    }

    #[test]
    fn keyboard_following_prefers_auto_detection_over_the_active_layout() {
        // A detection-capable model must not be forced to the active layout:
        // forcing a language the speech is not in makes Whisper translate
        // instead of transcribe. Constraining to the user's keyboard languages
        // happens afterwards, in wrong-language recovery.
        assert_eq!(
            resolve_language_intent_for_model(FOLLOW_KEYBOARD_LANGUAGE, true),
            "auto"
        );
    }

    #[test]
    fn explicit_choices_are_still_forced_even_when_detection_exists() {
        for language in ["auto", "bg", "de-DE"] {
            assert_eq!(resolve_language_intent_for_model(language, true), language);
            assert_eq!(resolve_language_intent_for_model(language, false), language);
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn must_pick_models_still_fall_back_to_the_keyboard_layout() {
        // Without detection the engine has to be told something, so keyboard
        // following resolves to a concrete language (auto off-macOS).
        assert_eq!(
            resolve_language_intent_for_model(FOLLOW_KEYBOARD_LANGUAGE, false),
            "auto"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn follow_keyboard_falls_back_to_auto_without_keyboard_access() {
        assert_eq!(resolve_language_intent(FOLLOW_KEYBOARD_LANGUAGE), "auto");
        assert!(enabled_keyboard_languages().is_empty());
    }
}
