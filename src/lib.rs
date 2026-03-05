//! # resharp_rs
//!
//! A high-performance, automata-based regex engine with first-class support for
//! **intersection** and **complement** operations.
//!
//! This is a Rust port of the [RE# regex engine](https://github.com/ieviev/resharp-dotnet)
//! by Ievgenii Shcherbina, implementing the algorithms described in:
//!
//! > *Derivative-Based Nonbacktracking Real-World Regex Matching with Intersection,
//! > Complement, and Lookarounds* — Ievgenii Shcherbina, Margus Veanes, and Olli Saarikivi
//! > (POPL 2025)
//!
//! ## Features
//!
//! - Compiles patterns into deterministic finite automata (DFA)
//! - Non-backtracking with guaranteed linear-time matching
//! - Automatic DFA pre-computation for optimal throughput
//! - Extends standard regex syntax with:
//!   - `&` - intersection: both sides must match
//!   - `~(...)` - complement: matches everything the inner pattern does not
//!   - `_` - universal wildcard: matches any character including newlines
//!
//! ## Example
//!
//! ```rust
//! use resharp_rs::Regex;
//!
//! // Contains "cat" AND "dog" AND is 8-15 characters long
//! let mut re = Regex::new(r".*cat.*&.*dog.*&.{8,15}").unwrap();
//! assert!(re.is_match("the cat and dog"));
//! ```

mod algorithm;
mod builder;
mod charset;
mod engine;
mod parser;
mod types;

pub use engine::Regex;
pub use parser::ParseError;
pub use types::{NodeFlags, RegexNodeId, StateFlags, ASCII_RANGE, BYTE_RANGE};

/// Result type for regex operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for regex operations
#[derive(Debug, Clone)]
pub enum Error {
    /// Pattern parsing failed
    Parse(ParseError),
    /// Pattern compilation failed
    Compile(String),
    /// DFA state limit exceeded
    DfaStateLimitExceeded,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "{}", e),
            Error::Compile(msg) => write!(f, "Compilation error: {}", msg),
            Error::DfaStateLimitExceeded => {
                write!(f, "DFA state limit exceeded (pattern too complex)")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self {
        Error::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        let mut re = Regex::new("hello").unwrap();
        assert!(re.is_match("hello"));
        assert!(re.is_match("say hello world"));
        assert!(!re.is_match("world"));
    }

    #[test]
    fn test_alternation() {
        let mut re = Regex::new("cat|dog").unwrap();
        assert!(re.is_match("cat"));
        assert!(re.is_match("dog"));
        assert!(re.is_match("my cat is cute"));
        assert!(!re.is_match("bird"));
    }

    #[test]
    fn test_star() {
        let mut re = Regex::new("ab*c").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("abbc"));
        assert!(re.is_match("abbbc"));
        assert!(!re.is_match("adc"));
    }

    #[test]
    fn test_plus() {
        let mut re = Regex::new("ab+c").unwrap();
        assert!(!re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(re.is_match("abbc"));
    }

    #[test]
    fn test_question() {
        let mut re = Regex::new("ab?c").unwrap();
        assert!(re.is_match("ac"));
        assert!(re.is_match("abc"));
        assert!(!re.is_match("abbc"));
    }

    #[test]
    fn test_dot() {
        let mut re = Regex::new("a.c").unwrap();
        assert!(re.is_match("abc"));
        assert!(re.is_match("axc"));
        assert!(!re.is_match("ac"));
        assert!(!re.is_match("a\nc")); // dot doesn't match newline
    }

    #[test]
    fn test_universal_wildcard() {
        let mut re = Regex::new("a_c").unwrap();
        assert!(re.is_match("abc"));
        assert!(re.is_match("a\nc")); // underscore matches newline
    }

    #[test]
    fn test_char_class() {
        let mut re = Regex::new("[abc]").unwrap();
        assert!(re.is_match("a"));
        assert!(re.is_match("b"));
        assert!(re.is_match("c"));
        assert!(!re.is_match("d"));
    }

    #[test]
    fn test_char_range() {
        let mut re = Regex::new("[a-z]+").unwrap();
        assert!(re.is_match("hello"));
        assert!(!re.is_match("123"));
    }

    #[test]
    fn test_negated_class() {
        let mut re = Regex::new("[^0-9]+").unwrap();
        assert!(re.is_match("hello"));
        assert!(re.is_match(" abc "));
    }

    #[test]
    fn test_repeat_range() {
        let mut re = Regex::new("a{2,4}").unwrap();
        assert!(!re.is_match("a"));
        assert!(re.is_match("aa"));
        assert!(re.is_match("aaa"));
        assert!(re.is_match("aaaa"));
        assert!(re.is_match("aaaaa")); // matches aaaa within it
    }

    #[test]
    fn test_intersection() {
        // Match strings that contain both "cat" and "dog"
        let mut re = Regex::new(".*cat.*&.*dog.*").unwrap();
        assert!(re.is_match("cat and dog"));
        assert!(re.is_match("dog and cat"));
        assert!(!re.is_match("only cat here"));
        assert!(!re.is_match("only dog here"));
    }

    #[test]
    fn test_complement() {
        // ~(_*) is equivalent to BOT - never matches anything
        // because _* matches everything including empty string
        let mut re = Regex::new("~(_*)").unwrap();
        assert!(!re.is_match("hello"));
        assert!(!re.is_match(""));
        assert!(!re.is_match("any string at all"));

        // Verify _* does match everything
        let mut re_all = Regex::new("_*").unwrap();
        assert!(re_all.is_match("hello"));
        assert!(re_all.is_match(""));
        assert!(re_all.is_match("anything"));

        // Test double negation: ~(~(_*)) should equal _* (matches everything)
        let mut re_double_neg = Regex::new("~(~(_*))").unwrap();
        assert!(re_double_neg.is_match("hello"));
        assert!(re_double_neg.is_match(""));
    }

    #[test]
    fn test_find_match() {
        let mut re = Regex::new("\\d+").unwrap();
        let m = re.find("abc123def").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
        assert_eq!(m.as_str("abc123def"), "123");
    }

    #[test]
    fn test_find_all() {
        let mut re = Regex::new("\\d+").unwrap();
        let matches = re.find_all("a1b22c333");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].as_str("a1b22c333"), "1");
        assert_eq!(matches[1].as_str("a1b22c333"), "22");
        assert_eq!(matches[2].as_str("a1b22c333"), "333");
    }

    #[test]
    fn test_precomputed_dfa() {
        // Test that DFA pre-computation happens automatically and produces correct results
        let patterns = ["[0-9]+", "[a-zA-Z]+", "the|and|or", "ab*c"];
        let input = "the abc 123 and or 456 abbbc";
        let expected = [
            vec![(8, 11), (19, 22)],                            // [0-9]+
            vec![(0, 3), (4, 7), (12, 15), (16, 18), (23, 28)], // [a-zA-Z]+
            vec![(0, 3), (12, 15), (16, 18)],                   // the|and|or
            vec![(4, 7), (23, 28)],                             // ab*c
        ];

        for (pattern, exp) in patterns.iter().zip(expected.iter()) {
            let mut re = Regex::new(pattern).unwrap();
            let matches = re.find_all(input);

            assert_eq!(
                matches.len(),
                exp.len(),
                "Match count differs for pattern '{}'",
                pattern
            );

            for (m, &(start, end)) in matches.iter().zip(exp.iter()) {
                assert_eq!(m.start, start, "Start differs for pattern '{}'", pattern);
                assert_eq!(m.end, end, "End differs for pattern '{}'", pattern);
            }
        }
    }
}
