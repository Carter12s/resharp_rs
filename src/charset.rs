//! Character set representation and operations.
//!
//! Uses a bit-set representation for ASCII characters and falls back to
//! ranges for Unicode characters. This module also handles minterm computation
//! for efficient DFA transitions.

use crate::types::{ASCII_RANGE, BYTE_RANGE};
use std::fmt;

/// A character set, represented as either a bitset (for ASCII) or ranges.
/// This is the "solver" type in the F# implementation.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct CharSet {
    /// Bitmap for ASCII characters (0-127)
    pub(crate) ascii: u128,
    /// Ranges for non-ASCII characters [(start, end), ...]
    unicode_ranges: Vec<(char, char)>,
}

impl fmt::Debug for CharSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CharSet(")?;
        let mut first = true;

        // Print ASCII chars
        for i in 0..(ASCII_RANGE as u8) {
            if self.contains_ascii(i) {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                let c = i as char;
                if c.is_ascii_graphic() {
                    write!(f, "{:?}", c)?;
                } else {
                    write!(f, "\\x{:02x}", i)?;
                }
            }
        }

        // Print Unicode ranges
        for (start, end) in &self.unicode_ranges {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            if start == end {
                write!(f, "{:?}", start)?;
            } else {
                write!(f, "{:?}-{:?}", start, end)?;
            }
        }

        write!(f, ")")
    }
}

impl CharSet {
    /// Creates an empty character set (matches nothing).
    pub fn empty() -> Self {
        Self {
            ascii: 0,
            unicode_ranges: Vec::new(),
        }
    }

    /// Creates a full character set (matches any character).
    pub fn full() -> Self {
        Self {
            ascii: !0u128,
            unicode_ranges: vec![('\u{80}', '\u{10FFFF}')],
        }
    }

    /// Creates a character set containing a single character.
    pub fn singleton(c: char) -> Self {
        let mut set = Self::empty();
        set.insert(c);
        set
    }

    /// Returns a reference to a pre-computed singleton charset for ASCII bytes.
    /// This avoids allocation for the common case.
    #[inline]
    pub fn ascii_singleton(byte: u8) -> &'static CharSet {
        &ASCII_SINGLETONS[byte as usize]
    }

    /// Creates a character set from a range of characters.
    pub fn range(start: char, end: char) -> Self {
        let mut set = Self::empty();
        set.insert_range(start, end);
        set
    }

    /// Inserts a single character.
    pub fn insert(&mut self, c: char) {
        let code = c as u32;
        if code < ASCII_RANGE as u32 {
            self.ascii |= 1u128 << code;
        } else {
            self.insert_unicode_range(c, c);
        }
    }

    /// Inserts a range of characters.
    pub fn insert_range(&mut self, start: char, end: char) {
        let start_code = start as u32;
        let end_code = end as u32;
        let ascii_max = (ASCII_RANGE - 1) as u32;

        // Handle ASCII portion
        if start_code < ASCII_RANGE as u32 {
            let ascii_end = end_code.min(ascii_max);
            for i in start_code..=ascii_end {
                self.ascii |= 1u128 << i;
            }
        }

        // Handle Unicode portion
        if end_code >= ASCII_RANGE as u32 {
            let unicode_start = if start_code < ASCII_RANGE as u32 {
                '\u{80}'
            } else {
                start
            };
            self.insert_unicode_range(unicode_start, end);
        }
    }

    fn insert_unicode_range(&mut self, start: char, end: char) {
        self.unicode_ranges.push((start, end));
        self.normalize_unicode_ranges();
    }

    fn normalize_unicode_ranges(&mut self) {
        if self.unicode_ranges.len() <= 1 {
            return;
        }

        self.unicode_ranges.sort_by_key(|(s, _)| *s);

        let mut merged = Vec::with_capacity(self.unicode_ranges.len());
        let mut current = self.unicode_ranges[0];

        for &(start, end) in &self.unicode_ranges[1..] {
            let next_char = char::from_u32(current.1 as u32 + 1).unwrap_or(current.1);
            if start <= next_char {
                current.1 = current.1.max(end);
            } else {
                merged.push(current);
                current = (start, end);
            }
        }
        merged.push(current);
        self.unicode_ranges = merged;
    }

    /// Returns true if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.ascii == 0 && self.unicode_ranges.is_empty()
    }

    #[inline]
    fn contains_ascii(&self, code: u8) -> bool {
        (self.ascii & (1u128 << code)) != 0
    }

    /// Returns the union of two character sets.
    pub fn union(&self, other: &CharSet) -> CharSet {
        let mut result = self.clone();
        result.ascii |= other.ascii;
        for &(start, end) in &other.unicode_ranges {
            result.unicode_ranges.push((start, end));
        }
        result.normalize_unicode_ranges();
        result
    }

    /// Returns the intersection of two character sets.
    pub fn intersection(&self, other: &CharSet) -> CharSet {
        let mut result = CharSet::empty();
        result.ascii = self.ascii & other.ascii;

        // Intersect unicode ranges
        for &(s1, e1) in &self.unicode_ranges {
            for &(s2, e2) in &other.unicode_ranges {
                let start = s1.max(s2);
                let end = e1.min(e2);
                if start <= end {
                    result.unicode_ranges.push((start, end));
                }
            }
        }
        result.normalize_unicode_ranges();
        result
    }

    /// Returns the complement of a character set.
    pub fn complement(&self) -> CharSet {
        let mut result = CharSet::empty();
        result.ascii = !self.ascii;

        // Complement unicode ranges
        let mut prev_end = '\u{80}';
        for &(start, end) in &self.unicode_ranges {
            if prev_end < start {
                let gap_end = char::from_u32(start as u32 - 1).unwrap_or(prev_end);
                if prev_end <= gap_end {
                    result.unicode_ranges.push((prev_end, gap_end));
                }
            }
            prev_end = char::from_u32(end as u32 + 1).unwrap_or('\u{10FFFF}');
        }
        if prev_end <= '\u{10FFFF}' {
            result.unicode_ranges.push((prev_end, '\u{10FFFF}'));
        }

        result
    }
}

/// Pre-computed singleton charsets for all BYTE_RANGE bytes.
/// This avoids allocation during hot path matching.
/// For bytes >= ASCII_RANGE, we store them in unicode_ranges.
static ASCII_SINGLETONS: std::sync::LazyLock<[CharSet; BYTE_RANGE]> =
    std::sync::LazyLock::new(|| {
        let mut result: [CharSet; BYTE_RANGE] = std::array::from_fn(|_| CharSet::empty());
        for i in 0..(ASCII_RANGE as u8) {
            result[i as usize] = CharSet {
                ascii: 1u128 << i,
                unicode_ranges: Vec::new(),
            };
        }
        // For bytes ASCII_RANGE..BYTE_RANGE, create charsets with unicode_ranges
        for i in (ASCII_RANGE as u8)..=255 {
            let c = i as char;
            result[i as usize] = CharSet {
                ascii: 0,
                unicode_ranges: vec![(c, c)],
            };
        }
        result
    });
