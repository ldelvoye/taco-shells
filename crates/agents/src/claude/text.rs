use serde_json::Value;

use crate::CUT_MARK;

// A command or path is cut to this many characters in a log entry.
pub(super) const MAX_ENTRY_CHARACTERS: usize = 1000;

pub(super) fn cut_to(text: &str, limit: usize) -> String {
    let kept = text.trim().chars().take(limit);
    kept.collect()
}

pub(super) fn text_at(parsed: &Value, pointer: &str) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    let cut = cut_to(text, MAX_ENTRY_CHARACTERS);
    if cut.is_empty() {
        return None;
    }
    Some(cut)
}

pub(super) fn written_at(parsed: &Value, pointer: &str, limit: usize) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    written(text, limit)
}

// Prose someone wrote (a prompt, a message, a note, a now line): trimmed, none when blank, and ending in CUT_MARK when cut.
pub(super) fn written(text: &str, limit: usize) -> Option<String> {
    let kept = cut_to(text, limit + 1);
    if kept.is_empty() {
        return None;
    }
    let length = kept.chars().count();
    if length <= limit {
        return Some(kept);
    }
    let shortened: String = kept.chars().take(limit).collect();
    Some(format!("{shortened}{CUT_MARK}"))
}

// A text that is only any use whole, such as a path or a link: none when it is blank or longer than `limit`.
pub(super) fn whole_text_at(parsed: &Value, pointer: &str, limit: usize) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    let trimmed = text.trim();
    let length = trimmed.chars().count();
    if trimmed.is_empty() || length > limit {
        return None;
    }
    Some(trimmed.to_string())
}
