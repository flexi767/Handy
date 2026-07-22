# Wrong Keyboard Language Recovery

**Status:** Implemented for Parakeet V3 GGUF in Follow Keyboard mode on macOS.

## User feature

This feature is for a user who starts speaking a language different from the
language indicated by the active keyboard. Recovery is limited to languages
represented by the user's enabled keyboards and advertised by the models; it
never opens unrestricted language detection or invents a language allowlist.

Selecting an explicit transcription language disables this feature. Translation
to English continues through the established transcription path because its
output language intentionally differs from the spoken-language candidates.

## Recovery flow

At recording start, Handy freezes the active keyboard language followed by the
other enabled keyboard languages. For Parakeet V3, transcription then proceeds
as follows:

1. Run Parakeet once using the active keyboard language as its initial hint.
   Parakeet V3 automatically detects language, so repeating its decoder for each
   hint can produce the same text and provides no genuinely forced retry.
2. Validate that single result against every frozen candidate in active-first
   order using Unicode script, CLDR exemplar characters, and macOS offline text
   language recognition.
3. Return the Parakeet text when it matches a candidate. Log when the matching
   candidate differs from the active keyboard, because that is a successful
   wrong-keyboard recovery.
4. If no candidate matches—or Parakeet is empty or fails—load Nemotron once and
   run the retained audio once per candidate with that exact language forced.
5. Accept only a usable Nemotron transcript that passes all three language
   validations for its forced candidate.
6. If every forced attempt fails, preserve the first usable Parakeet text and
   the source recording rather than losing the dictation.

The selected and cached primary model remains Parakeet. The temporary Nemotron
session exists only for the recovery sequence and is reused across candidate
attempts.

## Saved regression

History entry 138 was captured while the German keyboard was active. Parakeet
returned `Št je svârsza spokojna.` Apple Natural Language classifies the
complete text as Croatian at about 92.5% confidence and `svârsza spokojna` as
Polish at about 98.2%, so the text matches none of the installed candidates
`de`, `en`, and `bg`. The text is preserved as a unit regression, but its WAV
was pruned after the 50-entry history advanced and cannot be replayed.

The recovery sequence must run Nemotron with `de-DE`, then `en-US`, then
`bg-BG` (subject to each model's advertised exact locale), stopping at the first
validated result. In particular, an empty German result must not prevent the
retained audio from reaching forced Bulgarian.

The installed production build was replayed with retained history entry 182,
an English recording that Parakeet rendered in Cyrillic as
`Ю шуднт хит ютуб, райт.` The primary output matched no candidate, one Nemotron
session was loaded, forced `en-US` produced `He youtube`, and that validated
English result was returned. This verifies the real primary-rejection and
forced-language recovery path; a future live reproduction should additionally
exercise continuation past an empty first candidate into a later language.

## Boundaries and future work

- Mixed-language output is valid. The invariant is that each substantial span
  must be explainable by an enabled language; the transcript is not required to
  use only one language or script. Isolated low-confidence foreign tokens and
  brand names such as `OpenAI тест` remain allowed. A confident multi-word span
  in an unsupported language remains a recovery signal.
- Text-language span recognition currently uses Apple's offline Natural
  Language framework; other platforms retain script and alphabet validation.
- Very short same-script utterances may be ambiguous and intentionally prefer
  the active keyboard.
- Suspicious-history labeling and an automated checked-in WAV replay corpus are
  separate future improvements. The current fail-open behavior still preserves
  a non-empty primary result when every validated recovery fails.
