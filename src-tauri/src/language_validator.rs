//! Detect when a transcript is confidently in a language the user did not type.
//!
//! Used only by follow-keyboard wrong-language recovery: an auto-detecting
//! primary model (e.g. Parakeet) can return the wrong language, and this module
//! decides whether that happened by running Apple's NaturalLanguage recognizer
//! over the transcript and comparing the detected language against the user's
//! enabled keyboard languages. It is deliberately conservative — it only reports
//! a conflict when the evidence is strong, so legitimate mixed-language or
//! borrowed-word dictation is preserved.

use crate::keyboard_language::primary_subtag;

/// Minimum probability mass that must sit outside the candidate languages before
/// a short (single-word) transcript is treated as a conflict. Single words are
/// noisy, so the bar is high.
#[cfg(target_os = "macos")]
const SHORT_UTTERANCE_OUTSIDE_THRESHOLD: f64 = 0.85;

/// Minimum confidence for the dominant language of a multi-word transcript to
/// count as a conflict when it falls outside the candidate languages.
#[cfg(target_os = "macos")]
const DOMINANT_CONFLICT_CONFIDENCE: f64 = 0.90;

/// Returns `true` only when `text` is confidently in a language outside
/// `candidates` (base language subtags such as `"en"`, `"bg"`). Fails open:
/// empty input, no candidates, or ambiguous detection all return `false` so
/// that recovery never fires on uncertain evidence.
pub fn transcript_conflicts_with_candidates(text: &str, candidates: &[String]) -> bool {
    let candidate_bases: Vec<String> = candidates
        .iter()
        .filter_map(|language| primary_subtag(language))
        .collect();
    if candidate_bases.is_empty() {
        return false;
    }

    let word_count = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .count();
    if word_count == 0 {
        return false;
    }

    detect_conflict(text, word_count, &candidate_bases)
}

/// macOS implementation backed by `NLLanguageRecognizer`.
#[cfg(target_os = "macos")]
fn detect_conflict(text: &str, word_count: usize, candidate_bases: &[String]) -> bool {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::NSString;
    use objc2_natural_language::NLLanguageRecognizer;

    autoreleasepool(|_| {
        let ns_text = NSString::from_str(text);
        let recognizer = unsafe { NLLanguageRecognizer::new() };
        unsafe { recognizer.processString(&ns_text) };
        let hypotheses = unsafe { recognizer.languageHypothesesWithMaximum(5) };
        let (languages, confidences) = hypotheses.to_vecs();

        let mut outside_mass = 0.0;
        let mut dominant: Option<(String, f64)> = None;
        for (language, confidence) in languages.iter().zip(confidences.iter()) {
            let confidence = confidence.doubleValue();
            let Some(base) = primary_subtag(&language.to_string()) else {
                continue;
            };
            let inside = candidate_bases.iter().any(|candidate| candidate == &base);
            if !inside {
                outside_mass += confidence;
            }
            if dominant.as_ref().is_none_or(|(_, best)| confidence > *best) {
                dominant = Some((base, confidence));
            }
        }

        // A single word rarely establishes a positive language identity, so
        // require most of the probability mass to fall outside the candidates.
        if word_count == 1 {
            return outside_mass >= SHORT_UTTERANCE_OUTSIDE_THRESHOLD;
        }

        // Otherwise: a large outside mass, or a confident dominant language that
        // is not one the user types, both count as a conflict.
        if outside_mass >= SHORT_UTTERANCE_OUTSIDE_THRESHOLD {
            return true;
        }
        matches!(
            dominant,
            Some((base, confidence))
                if confidence >= DOMINANT_CONFLICT_CONFIDENCE
                    && !candidate_bases.iter().any(|candidate| candidate == &base)
        )
    })
}

/// Non-macOS: follow-keyboard recovery is macOS-only, so never report a
/// conflict here.
#[cfg(not(target_os = "macos"))]
fn detect_conflict(_text: &str, _word_count: usize, _candidate_bases: &[String]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_candidates_never_conflicts() {
        assert!(!transcript_conflicts_with_candidates("hello world", &[]));
    }

    #[test]
    fn empty_text_never_conflicts() {
        assert!(!transcript_conflicts_with_candidates(
            "   ",
            &["bg".to_string()]
        ));
        assert!(!transcript_conflicts_with_candidates(
            "12345",
            &["bg".to_string()]
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn confident_foreign_sentence_conflicts() {
        // Clearly English text is not something a Bulgarian-only typist produces.
        assert!(transcript_conflicts_with_candidates(
            "the quick brown fox jumps over the lazy dog every morning",
            &["bg".to_string()]
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn matching_language_does_not_conflict() {
        assert!(!transcript_conflicts_with_candidates(
            "the quick brown fox jumps over the lazy dog every morning",
            &["en".to_string()]
        ));
    }
}
