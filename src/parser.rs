//! Regex pattern parser.
//!
//! Parses regex patterns into an AST (via the builder).
//! Supports standard regex syntax plus resharp extensions:
//! - `&` for intersection
//! - `~(...)` for complement
//! - `_` for universal wildcard (matches any character including newlines)

use crate::builder::RegexBuilder;
use crate::charset::CharSet;
use crate::types::RegexNodeId;
use std::iter::Peekable;
use std::str::Chars;

/// Parse error with position information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at position {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Regex pattern parser.
pub struct Parser<'a> {
    chars: Peekable<Chars<'a>>,
    position: usize,
    builder: RegexBuilder<CharSet>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
            position: 0,
            builder: RegexBuilder::new(),
        }
    }

    /// Parse the pattern and return the root node ID and builder.
    pub fn parse(mut self) -> Result<(RegexNodeId, RegexBuilder<CharSet>), ParseError> {
        let root = self.parse_alternation()?;
        if self.peek().is_some() {
            return Err(self.error("Unexpected character"));
        }
        Ok((root, self.builder))
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            position: self.position,
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next();
        if c.is_some() {
            self.position += 1;
        }
        c
    }

    fn expect(&mut self, expected: char) -> Result<(), ParseError> {
        match self.advance() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(self.error(&format!("Expected '{}', found '{}'", expected, c))),
            None => Err(self.error(&format!("Expected '{}', found end of input", expected))),
        }
    }

    /// Skip flag characters in a group like (?m), (?i-u), etc.
    /// Stops at ':' (flag group with content) or ')' (flag-only group)
    fn skip_flags(&mut self) -> Result<(), ParseError> {
        while let Some(c) = self.peek() {
            match c {
                'i' | 'm' | 's' | 'u' | 'U' | 'x' | '-' => {
                    self.advance();
                }
                ':' => {
                    self.advance();
                    break;
                }
                ')' => {
                    // Don't consume - let caller handle it
                    break;
                }
                _ => {
                    return Err(self.error(&format!("Unknown flag character '{}'", c)));
                }
            }
        }
        Ok(())
    }

    /// Parse alternation: expr ('|' expr)*
    fn parse_alternation(&mut self) -> Result<RegexNodeId, ParseError> {
        let mut nodes = vec![self.parse_intersection()?];

        while self.peek() == Some('|') {
            self.advance();
            nodes.push(self.parse_intersection()?);
        }

        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(self.builder.mk_or(nodes))
        }
    }

    /// Parse intersection: expr ('&' expr)*
    fn parse_intersection(&mut self) -> Result<RegexNodeId, ParseError> {
        let mut nodes = vec![self.parse_concat()?];

        while self.peek() == Some('&') {
            self.advance();
            nodes.push(self.parse_concat()?);
        }

        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(self.builder.mk_and(nodes))
        }
    }

    /// Parse concatenation: term+
    fn parse_concat(&mut self) -> Result<RegexNodeId, ParseError> {
        let mut nodes = Vec::new();

        while let Some(c) = self.peek() {
            if c == '|' || c == '&' || c == ')' {
                break;
            }
            nodes.push(self.parse_quantified()?);
        }

        if nodes.is_empty() {
            Ok(self.builder.mk_eps())
        } else if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(self.builder.mk_concat_many(nodes))
        }
    }

    /// Parse quantified expression: atom ('*' | '+' | '?' | '{n,m}')? '?'?
    /// Note: Lazy quantifiers (*?, +?, etc.) are parsed but treated as greedy
    /// since our engine uses leftmost-longest semantics.
    fn parse_quantified(&mut self) -> Result<RegexNodeId, ParseError> {
        let node = self.parse_atom()?;

        let result = match self.peek() {
            Some('*') => {
                self.advance();
                self.builder.mk_loop(node, 0, u32::MAX)
            }
            Some('+') => {
                self.advance();
                self.builder.mk_loop(node, 1, u32::MAX)
            }
            Some('?') => {
                self.advance();
                self.builder.mk_loop(node, 0, 1)
            }
            Some('{') => self.parse_repeat_range(node)?,
            _ => return Ok(node),
        };

        // Consume optional '?' for lazy quantifiers (treated same as greedy)
        if self.peek() == Some('?') {
            self.advance();
        }

        Ok(result)
    }

    /// Parse {n,m} repeat syntax
    fn parse_repeat_range(&mut self, node: RegexNodeId) -> Result<RegexNodeId, ParseError> {
        self.expect('{')?;
        let low = self.parse_number()?;

        let high = if self.peek() == Some(',') {
            self.advance();
            if self.peek() == Some('}') {
                u32::MAX
            } else {
                self.parse_number()?
            }
        } else {
            low
        };

        self.expect('}')?;
        Ok(self.builder.mk_loop(node, low, high))
    }

    fn parse_number(&mut self) -> Result<u32, ParseError> {
        let mut num = 0u32;
        let mut found = false;

        while let Some(c) = self.peek() {
            if let Some(digit) = c.to_digit(10) {
                found = true;
                num = num.saturating_mul(10).saturating_add(digit);
                self.advance();
            } else {
                break;
            }
        }

        if !found {
            return Err(self.error("Expected a number"));
        }
        Ok(num)
    }

    /// Parse atom: literal | '.' | '_' | '(' expr ')' | '~' '(' expr ')' | '[' class ']' | escape
    fn parse_atom(&mut self) -> Result<RegexNodeId, ParseError> {
        match self.peek() {
            None => Err(self.error("Unexpected end of input")),
            Some('(') => {
                self.advance();
                // Check for lookahead/lookbehind
                if self.peek() == Some('?') {
                    self.advance();
                    match self.peek() {
                        Some(':') => {
                            // Non-capturing group (?:...) - just parse and return inner
                            self.advance();
                            let inner = self.parse_alternation()?;
                            self.expect(')')?;
                            Ok(inner)
                        }
                        Some('=') => {
                            self.advance();
                            let inner = self.parse_alternation()?;
                            self.expect(')')?;
                            Ok(self.builder.mk_lookahead(inner))
                        }
                        Some('!') => {
                            self.advance();
                            let inner = self.parse_alternation()?;
                            self.expect(')')?;
                            let not_inner = self.builder.mk_not(inner);
                            Ok(self.builder.mk_lookahead(not_inner))
                        }
                        Some('<') => {
                            self.advance();
                            match self.peek() {
                                Some('=') => {
                                    self.advance();
                                    let inner = self.parse_alternation()?;
                                    self.expect(')')?;
                                    Ok(self.builder.mk_lookbehind(inner))
                                }
                                Some('!') => {
                                    self.advance();
                                    let inner = self.parse_alternation()?;
                                    self.expect(')')?;
                                    let not_inner = self.builder.mk_not(inner);
                                    Ok(self.builder.mk_lookbehind(not_inner))
                                }
                                _ => Err(self.error("Expected '=' or '!' after '(?<'")),
                            }
                        }
                        _ => {
                            // Skip unknown flags like (?m), (?i), (?-u), etc.
                            // Just consume until we hit ':' or ')'
                            self.skip_flags()?;
                            if self.peek() == Some(')') {
                                // Flag-only group like (?m) - treat as epsilon
                                self.advance();
                                Ok(self.builder.mk_eps())
                            } else {
                                // Flag group with content like (?m:...) or (?i-u:...)
                                let inner = self.parse_alternation()?;
                                self.expect(')')?;
                                Ok(inner)
                            }
                        }
                    }
                } else {
                    let inner = self.parse_alternation()?;
                    self.expect(')')?;
                    Ok(inner)
                }
            }
            Some('~') => {
                self.advance();
                self.expect('(')?;
                let inner = self.parse_alternation()?;
                self.expect(')')?;
                Ok(self.builder.mk_not(inner))
            }
            Some('.') => {
                self.advance();
                // '.' matches any character except newline
                let mut set = CharSet::full();
                set.ascii &= !(1u128 << ('\n' as u32));
                Ok(self.builder.mk_singleton(set))
            }
            Some('_') => {
                self.advance();
                // '_' matches any character (true wildcard)
                Ok(self.builder.mk_singleton(CharSet::full()))
            }
            Some('[') => self.parse_char_class(),
            Some('^') => {
                self.advance();
                Ok(self.builder.mk_begin())
            }
            Some('$') => {
                self.advance();
                Ok(self.builder.mk_end())
            }
            Some('\\') => self.parse_escape(),
            Some(c) if is_metachar(c) => {
                Err(self.error(&format!("Unexpected metacharacter '{}'", c)))
            }
            Some(c) => {
                self.advance();
                Ok(self.builder.mk_singleton(CharSet::singleton(c)))
            }
        }
    }

    /// Create a CharSet for word characters (\w)
    fn word_charset() -> CharSet {
        let mut set = CharSet::range('a', 'z');
        set = set.union(&CharSet::range('A', 'Z'));
        set = set.union(&CharSet::range('0', '9'));
        set.insert('_');
        set
    }

    /// Create a CharSet for whitespace characters (\s)
    fn whitespace_charset() -> CharSet {
        let mut set = CharSet::singleton(' ');
        set.insert('\t');
        set.insert('\n');
        set.insert('\r');
        set.insert('\x0C'); // form feed
        set
    }

    fn parse_escape(&mut self) -> Result<RegexNodeId, ParseError> {
        self.expect('\\')?;
        match self.advance() {
            None => Err(self.error("Unexpected end after escape")),
            Some('n') => Ok(self.builder.mk_singleton(CharSet::singleton('\n'))),
            Some('r') => Ok(self.builder.mk_singleton(CharSet::singleton('\r'))),
            Some('t') => Ok(self.builder.mk_singleton(CharSet::singleton('\t'))),
            Some('d') => Ok(self.builder.mk_singleton(CharSet::range('0', '9'))),
            Some('w') => Ok(self.builder.mk_singleton(Self::word_charset())),
            Some('s') => Ok(self.builder.mk_singleton(Self::whitespace_charset())),
            Some('D') => {
                let set = CharSet::range('0', '9').complement();
                Ok(self.builder.mk_singleton(set))
            }
            Some('W') => Ok(self.builder.mk_singleton(Self::word_charset().complement())),
            Some('S') => Ok(self
                .builder
                .mk_singleton(Self::whitespace_charset().complement())),
            Some('A') => Ok(self.builder.mk_begin()),
            Some('z') => Ok(self.builder.mk_end()),
            Some('b') => {
                // Word boundary: (?<=\w)(?!\w)|(?<!\w)(?=\w)
                // Matches at position where word/non-word transition occurs
                let word = self.builder.mk_singleton(Self::word_charset());
                let non_word = self.builder.mk_singleton(Self::word_charset().complement());

                // (?<=\w)(?!\w) - after word char, before non-word
                let lookbehind_word = self.builder.mk_lookbehind(word);
                let lookahead_non_word = self.builder.mk_lookahead(non_word);
                let word_to_nonword = self.builder.mk_concat(lookbehind_word, lookahead_non_word);

                // (?<!\w)(?=\w) - after non-word char (or start), before word char
                // (?<!\w) = not((?<=\w))
                let word2 = self.builder.mk_singleton(Self::word_charset());
                let preceded_by_word = self.builder.mk_lookbehind(word2);
                let not_preceded_by_word = self.builder.mk_not(preceded_by_word);
                let word3 = self.builder.mk_singleton(Self::word_charset());
                let lookahead_word = self.builder.mk_lookahead(word3);
                let nonword_to_word = self.builder.mk_concat(not_preceded_by_word, lookahead_word);

                Ok(self.builder.mk_or(vec![word_to_nonword, nonword_to_word]))
            }
            Some('B') => {
                // Non-word boundary: opposite of \b
                // (?<=\w)(?=\w)|(?<!\w)(?!\w)
                let word = self.builder.mk_singleton(Self::word_charset());
                let word2 = self.builder.mk_singleton(Self::word_charset());
                let word3 = self.builder.mk_singleton(Self::word_charset());
                let word4 = self.builder.mk_singleton(Self::word_charset());

                // (?<=\w)(?=\w) - in the middle of a word
                let lookbehind_word = self.builder.mk_lookbehind(word);
                let lookahead_word = self.builder.mk_lookahead(word2);
                let in_word = self.builder.mk_concat(lookbehind_word, lookahead_word);

                // (?<!\w)(?!\w) - outside of any word
                let preceded_by_word = self.builder.mk_lookbehind(word3);
                let not_preceded_by_word = self.builder.mk_not(preceded_by_word);
                let followed_by_word = self.builder.mk_lookahead(word4);
                let not_followed_by_word = self.builder.mk_not(followed_by_word);
                let outside_word = self
                    .builder
                    .mk_concat(not_preceded_by_word, not_followed_by_word);

                Ok(self.builder.mk_or(vec![in_word, outside_word]))
            }
            Some(c) => Ok(self.builder.mk_singleton(CharSet::singleton(c))),
        }
    }

    fn parse_char_class(&mut self) -> Result<RegexNodeId, ParseError> {
        self.expect('[')?;
        let negated = if self.peek() == Some('^') {
            self.advance();
            true
        } else {
            false
        };

        let mut set = CharSet::empty();

        while self.peek() != Some(']') && self.peek().is_some() {
            // Check for POSIX character class [:name:]
            if self.peek() == Some('[') {
                let mut peek_chars = self.chars.clone();
                peek_chars.next(); // skip the '['
                if peek_chars.next() == Some(':') {
                    // This looks like a POSIX class
                    if let Some(posix_set) = self.try_parse_posix_class()? {
                        set = set.union(&posix_set);
                        continue;
                    }
                }
            }

            let c = self.advance().unwrap();

            if self.peek() == Some('-') && {
                let mut peek2 = self.chars.clone();
                peek2.next();
                peek2.peek() != Some(&']')
            } {
                self.advance(); // consume '-'
                let end = self
                    .advance()
                    .ok_or_else(|| self.error("Expected end of range"))?;
                set.insert_range(c, end);
            } else if c == '\\' {
                let escaped = self
                    .advance()
                    .ok_or_else(|| self.error("Unexpected end after escape"))?;
                match escaped {
                    'n' => set.insert('\n'),
                    'r' => set.insert('\r'),
                    't' => set.insert('\t'),
                    'd' => set = set.union(&CharSet::range('0', '9')),
                    'w' => {
                        set = set.union(&CharSet::range('a', 'z'));
                        set = set.union(&CharSet::range('A', 'Z'));
                        set = set.union(&CharSet::range('0', '9'));
                        set.insert('_');
                    }
                    's' => {
                        set.insert(' ');
                        set.insert('\t');
                        set.insert('\n');
                        set.insert('\r');
                        set.insert('\x0C');
                    }
                    _ => set.insert(escaped),
                }
            } else {
                set.insert(c);
            }
        }

        self.expect(']')?;

        if negated {
            set = set.complement();
        }

        Ok(self.builder.mk_singleton(set))
    }

    /// Try to parse a POSIX character class like [:alpha:]
    /// Returns None if it's not a valid POSIX class (so caller can treat '[' as literal)
    fn try_parse_posix_class(&mut self) -> Result<Option<CharSet>, ParseError> {
        // We've peeked and seen '[' followed by ':'
        self.advance(); // consume '['
        self.advance(); // consume ':'

        // Check for negation
        let negated = if self.peek() == Some('^') {
            self.advance();
            true
        } else {
            false
        };

        // Read the class name
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == ':' {
                break;
            }
            if !c.is_ascii_alphabetic() {
                return Err(self.error("Invalid POSIX class name"));
            }
            name.push(c);
            self.advance();
        }

        self.expect(':')?;
        self.expect(']')?;

        let mut set = match name.as_str() {
            "alpha" => {
                let mut s = CharSet::range('a', 'z');
                s = s.union(&CharSet::range('A', 'Z'));
                s
            }
            "digit" => CharSet::range('0', '9'),
            "alnum" => {
                let mut s = CharSet::range('a', 'z');
                s = s.union(&CharSet::range('A', 'Z'));
                s = s.union(&CharSet::range('0', '9'));
                s
            }
            "space" => {
                let mut s = CharSet::singleton(' ');
                s.insert('\t');
                s.insert('\n');
                s.insert('\r');
                s.insert('\x0B'); // vertical tab
                s.insert('\x0C'); // form feed
                s
            }
            "upper" => CharSet::range('A', 'Z'),
            "lower" => CharSet::range('a', 'z'),
            "word" => {
                let mut s = CharSet::range('a', 'z');
                s = s.union(&CharSet::range('A', 'Z'));
                s = s.union(&CharSet::range('0', '9'));
                s.insert('_');
                s
            }
            "xdigit" => {
                let mut s = CharSet::range('0', '9');
                s = s.union(&CharSet::range('a', 'f'));
                s = s.union(&CharSet::range('A', 'F'));
                s
            }
            "punct" => {
                let mut s = CharSet::empty();
                for c in "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".chars() {
                    s.insert(c);
                }
                s
            }
            "blank" => {
                let mut s = CharSet::singleton(' ');
                s.insert('\t');
                s
            }
            "cntrl" => {
                let mut s = CharSet::range('\x00', '\x1F');
                s.insert('\x7F');
                s
            }
            "graph" => {
                // Visible characters (not space or control)
                CharSet::range('!', '~')
            }
            "print" => {
                // Printable characters (graph + space)
                CharSet::range(' ', '~')
            }
            "ascii" => CharSet::range('\x00', '\x7F'),
            _ => return Err(self.error(&format!("Unknown POSIX class '{}'", name))),
        };

        if negated {
            set = set.complement();
        }

        Ok(Some(set))
    }
}

fn is_metachar(c: char) -> bool {
    matches!(
        c,
        '*' | '+' | '?' | '{' | '}' | '|' | '&' | '(' | ')' | '[' | ']'
    )
}
