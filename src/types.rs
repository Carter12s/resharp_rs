//! Core types for the regex engine.

/// Number of ASCII characters (0-127).
pub const ASCII_RANGE: usize = 128;

/// Number of possible byte values (0-255).
pub const BYTE_RANGE: usize = 256;

/// Unique identifier for regex nodes in the builder's storage.
pub type RegexNodeId = u32;

/// Well-known node IDs for special regex constructs.
pub mod node_id {
    use super::RegexNodeId;

    /// Bottom/empty language - matches nothing
    pub const BOT: RegexNodeId = 0;
    /// Epsilon - matches empty string
    pub const EPS: RegexNodeId = 1;
    /// Top - matches any single character
    pub const TOP: RegexNodeId = 2;
    /// True star (_*) - matches any string
    pub const TOP_STAR: RegexNodeId = 3;
    /// True plus (_+) - matches any non-empty string
    pub const TOP_PLUS: RegexNodeId = 4;
    /// End anchor (\z)
    pub const END_ANCHOR: RegexNodeId = 5;
    /// Begin anchor (\A)
    pub const BEGIN_ANCHOR: RegexNodeId = 6;
}

/// Location kind for derivative computation - influences nullability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LocationKind {
    /// At the beginning of the string (before first character)
    Begin = 0,
    /// In the middle of the string (not at beginning or end)
    Center = 1,
    /// At the end of the string (after last character)
    End = 2,
    /// Both at beginning and end (empty string only)
    BeginEnd = 3,
}

impl LocationKind {
    /// Check if this location is at the beginning
    #[inline]
    pub fn is_begin(self) -> bool {
        matches!(self, LocationKind::Begin | LocationKind::BeginEnd)
    }

    /// Check if this location is at the end
    #[inline]
    pub fn is_end(self) -> bool {
        matches!(self, LocationKind::End | LocationKind::BeginEnd)
    }
}

/// Flags for regex nodes - used to short-circuit expensive computations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeFlags(u8);

impl NodeFlags {
    pub const NONE: Self = Self(0);
    pub const CAN_BE_NULLABLE: Self = Self(0x01);
    pub const IS_ALWAYS_NULLABLE: Self = Self(0x02);
    pub const CONTAINS_LOOKAROUND: Self = Self(0x04);
    pub const DEPENDS_ON_ANCHOR: Self = Self(0x08);
    pub const HAS_SUFFIX_LOOKAHEAD: Self = Self(0x10);
    pub const HAS_PREFIX_LOOKBEHIND: Self = Self(0x20);

    #[inline]
    pub fn has(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    #[inline]
    pub fn is_always_nullable(self) -> bool {
        self.has(Self::IS_ALWAYS_NULLABLE)
    }

    #[inline]
    pub fn can_be_nullable(self) -> bool {
        self.has(Self::CAN_BE_NULLABLE)
    }

    #[inline]
    pub fn contains_lookaround(self) -> bool {
        self.has(Self::CONTAINS_LOOKAROUND)
    }

    #[inline]
    pub fn depends_on_anchor(self) -> bool {
        self.has(Self::DEPENDS_ON_ANCHOR)
    }
}

impl std::ops::BitOr for NodeFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for NodeFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

/// Flags for DFA states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateFlags(u8);

impl StateFlags {
    pub const NONE: Self = Self(0);
    pub const INITIAL: Self = Self(0x01);
    pub const HAS_TAG: Self = Self(0x02);
    pub const IS_ANCHOR_NULLABLE: Self = Self(0x04);
    pub const CAN_SKIP: Self = Self(0x08);
    pub const IS_BEGIN_NULLABLE: Self = Self(0x10);
    pub const IS_END_NULLABLE: Self = Self(0x20);
    pub const IS_ALWAYS_NULLABLE: Self = Self(0x40);
    pub const IS_PENDING_NULLABLE: Self = Self(0x80);

    #[inline]
    pub fn is_always_nullable(self) -> bool {
        (self.0 & Self::IS_ALWAYS_NULLABLE.0) == Self::IS_ALWAYS_NULLABLE.0
    }

    #[inline]
    pub fn can_be_nullable(self) -> bool {
        (self.0 & (Self::IS_ALWAYS_NULLABLE.0 | Self::IS_ANCHOR_NULLABLE.0)) != 0
    }

    #[inline]
    pub fn is_initial(self) -> bool {
        (self.0 & Self::INITIAL.0) == Self::INITIAL.0
    }

    #[inline]
    pub fn can_skip(self) -> bool {
        (self.0 & Self::CAN_SKIP.0) == Self::CAN_SKIP.0
    }
}

impl std::ops::BitOr for StateFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// A regex AST node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RegexNode<S: Clone + Eq + std::hash::Hash> {
    /// Matches a single character from the set
    Singleton(S),
    /// Concatenation: head followed by tail
    Concat {
        head: RegexNodeId,
        tail: RegexNodeId,
    },
    /// Repetition: node{low, high}
    Loop {
        node: RegexNodeId,
        low: u32,
        high: u32,
    },
    /// Alternation: any of the nodes
    Or(Vec<RegexNodeId>),
    /// Intersection: all nodes must match
    And(Vec<RegexNodeId>),
    /// Complement: matches what inner doesn't match
    Not(RegexNodeId),
    /// Lookaround assertion
    LookAround {
        node: RegexNodeId,
        look_back: bool,
        relative_to: RegexNodeId,
        pending_nullables: RefSet,
    },
    /// Start of string anchor
    Begin,
    /// End of string anchor
    End,
}

/// Information about a regex node for optimization purposes.
#[derive(Debug, Clone, Default)]
pub struct RegexNodeInfo {
    pub flags: NodeFlags,
}

/// A set of relative positions for pending lookaround nullability.
/// Stored as ranges of (start, end) pairs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct RefSet {
    inner: Vec<(u16, u16)>,
}

impl RefSet {
    pub fn empty() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn singleton(pos: u16) -> Self {
        Self {
            inner: vec![(pos, pos)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// A match result from the regex engine.
///
/// Contains the start and end byte offsets of the match within the input string.
/// Use [`as_str()`](Self::as_str) to extract the matched text.
///
/// # Example
///
/// ```
/// use resharp_rs::Regex;
///
/// let mut re = Regex::new(r"\d+").unwrap();
/// let m = re.find("abc123def").unwrap();
/// assert_eq!(m.start, 3);
/// assert_eq!(m.end, 6);
/// assert_eq!(m.as_str("abc123def"), "123");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

impl Match {
    /// Create a new match with the given byte offsets.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Returns the length of the match in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if this is an empty (zero-length) match.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Extract the matched text from the input string.
    ///
    /// # Panics
    ///
    /// Panics if the offsets are out of bounds for the input string.
    pub fn as_str<'a>(&self, input: &'a str) -> &'a str {
        &input[self.start..self.end]
    }
}
