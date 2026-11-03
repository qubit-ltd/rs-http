// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded output assembly for diagnostic messages.

/// Appends safe message fragments while enforcing an output byte budget.
///
/// Plain text is escaped character by character so control and directional
/// formatting characters cannot alter the appearance of a log record. Safe
/// fragments, including already-redacted URLs, are appended unchanged.
pub(in crate::redact) struct MessageEmitter {
    /// Output accumulated so far; its length is always at most `maximum` bytes.
    output: String,
    /// Maximum number of output bytes permitted.
    maximum: usize,
}

impl MessageEmitter {
    /// Creates an empty emitter with the specified maximum output size in
    /// bytes.
    pub(in crate::redact) fn new(maximum: usize) -> Self {
        Self {
            output: String::new(),
            maximum,
        }
    }

    /// Appends a safe fragment if it fits within the remaining byte budget.
    ///
    /// Returns `false` without changing the output when the fragment does not
    /// fit, allowing the caller to fail closed with a marker or an empty
    /// string.
    pub(in crate::redact) fn push_safe(&mut self, value: &str) -> bool {
        if value.len() > self.maximum.saturating_sub(self.output.len()) {
            return false;
        }
        self.output.push_str(value);
        true
    }

    /// Escapes dangerous characters and appends the result within the byte
    /// budget.
    ///
    /// Returns `false` if any escaped character would exceed the budget.
    /// Earlier characters may already have been appended; callers should
    /// then discard this partial output and use their fail-closed fallback.
    pub(in crate::redact) fn push_plain(&mut self, value: &str) -> bool {
        for character in value.chars() {
            if is_log_dangerous(character) {
                let escaped = character.escape_debug().to_string();
                if !self.push_safe(&escaped) {
                    return false;
                }
            } else {
                let character_length = character.len_utf8();
                if character_length > self.maximum.saturating_sub(self.output.len()) {
                    return false;
                }
                self.output.push(character);
            }
        }
        true
    }

    /// Returns the assembled output.
    pub(in crate::redact) fn finish(self) -> String {
        self.output
    }
}

/// Reports characters that can disrupt the visual or structural form of logs.
fn is_log_dangerous(character: char) -> bool {
    character.is_control() || matches!(character as u32, 0x061c | 0x200e..=0x200f | 0x2028..=0x202e | 0x2066..=0x2069)
}

#[cfg(test)]
mod tests {
    use super::MessageEmitter;

    #[test]
    fn push_plain_respects_empty_and_ascii_budgets() {
        let mut emitter = MessageEmitter::new(0);
        assert!(emitter.push_plain(""));
        assert!(!emitter.push_plain("a"));
        assert_eq!(emitter.finish(), "");

        let mut emitter = MessageEmitter::new(3);
        assert!(emitter.push_plain("abc"));
        assert_eq!(emitter.finish(), "abc");

        let mut emitter = MessageEmitter::new(2);
        assert!(!emitter.push_plain("abc"));
        assert_eq!(emitter.finish(), "ab");
    }

    #[test]
    fn push_plain_respects_unicode_and_accumulated_budgets() {
        let mut emitter = MessageEmitter::new(2);
        assert!(emitter.push_plain("é"));
        assert_eq!(emitter.finish(), "é");

        let mut emitter = MessageEmitter::new(1);
        assert!(!emitter.push_plain("é"));
        assert_eq!(emitter.finish(), "");

        let mut emitter = MessageEmitter::new(4);
        assert!(emitter.push_plain("aé"));
        assert!(emitter.push_plain("b"));
        assert!(!emitter.push_plain("c"));
        assert_eq!(emitter.finish(), "aéb");
    }

    #[test]
    fn push_plain_escapes_newlines_within_budget() {
        let mut emitter = MessageEmitter::new(2);
        assert!(emitter.push_plain("\n"));
        assert_eq!(emitter.finish(), r"\n");

        let mut emitter = MessageEmitter::new(1);
        assert!(!emitter.push_plain("\n"));
        assert_eq!(emitter.finish(), "");
    }

    #[test]
    fn push_plain_preserves_escape_debug_for_directional_controls() {
        let character = '\u{202e}';
        let expected = character.escape_debug().to_string();

        let mut emitter = MessageEmitter::new(expected.len());
        assert!(emitter.push_plain(&character.to_string()));
        assert_eq!(emitter.finish(), expected);

        let mut emitter = MessageEmitter::new(expected.len() - 1);
        assert!(!emitter.push_plain(&character.to_string()));
        assert_eq!(emitter.finish(), "");
    }
}
