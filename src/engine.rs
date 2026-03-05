//! DFA-based regex matching engine.
//!
//! This module implements the compiled regex and matching logic.

use crate::algorithm::{derivative, is_nullable};
use crate::builder::RegexBuilder;
use crate::charset::CharSet;
use crate::parser::Parser;
use crate::types::{node_id, LocationKind, Match, RegexNode, RegexNodeId, ASCII_RANGE, BYTE_RANGE};
use crate::{Error, Result};
use memchr::memmem;
use rustc_hash::FxHashMap;

/// Maximum number of DFA states before we bail out.
const DEFAULT_STATE_LIMIT: usize = 10_000;

/// Default limit for pre-computed DFA states (keep it reasonable for memory)
const DEFAULT_PRECOMPUTE_LIMIT: usize = 1024;

/// Sentinel value for uncomputed transitions
const UNCOMPUTED: RegexNodeId = u32::MAX;

/// Nullable cache values (using u8 for memory efficiency)
mod nullable_status {
    /// State is nullable (matches empty string)
    pub const NULLABLE: u8 = 1;
    /// State is not nullable
    pub const NOT_NULLABLE: u8 = 2;
}

/// Literal prefix optimization data
#[derive(Debug, Clone)]
enum PrefixAccelerator {
    /// No prefix optimization available
    None,
    /// Single byte prefix (use memchr)
    SingleByte(u8),
    /// Multi-byte literal prefix (use memmem)
    Literal(Vec<u8>),
    /// Small set of starting bytes (use memchr2/3 or startset)
    StartBytes(Vec<u8>),
}

/// Pre-computed DFA for high-throughput matching.
/// Uses a flat transition table indexed by (state_index * BYTE_RANGE + byte).
#[derive(Debug, Clone)]
struct PrecomputedDfa {
    /// Flat transition table: transitions[state_idx * BYTE_RANGE + byte] = next_state_idx
    transitions: Vec<u32>,
    /// Nullable bitmap: bit i is set if state i is nullable (for Center location)
    nullable: Vec<bool>,
}

/// A compiled regular expression using Brzozowski derivatives.
///
/// This regex engine implements POSIX leftmost-longest semantics via the
/// Brzozowski derivative algorithm. It automatically pre-computes the DFA
/// at construction time for optimal matching performance.
///
/// # Features
///
/// - **Automatic DFA pre-computation**: The DFA is built at construction time
///   for optimal matching throughput.
/// - **Prefix acceleration**: Literal prefixes are extracted and searched
///   using SIMD-accelerated algorithms (memchr/memmem).
/// - **POSIX semantics**: Always finds the leftmost-longest match.
///
/// # Example
///
/// ```
/// use resharp_rs::Regex;
///
/// let mut re = Regex::new("[a-zA-Z]+").unwrap();
/// assert!(re.is_match("hello world"));
///
/// let m = re.find("hello world").unwrap();
/// assert_eq!(m.start, 0);
/// assert_eq!(m.end, 5);
/// ```
pub struct Regex {
    builder: RegexBuilder<CharSet>,
    root: RegexNodeId,
    /// Cached DFA transitions: state -> byte -> next_state (BYTE_RANGE entries per state)
    transitions: FxHashMap<RegexNodeId, Box<[RegexNodeId; BYTE_RANGE]>>,
    /// Cached nullable status for states at Center location.
    /// Uses nullable_status constants: NULLABLE or NOT_NULLABLE.
    /// Uncached entries are not present in the map.
    nullable_cache: FxHashMap<RegexNodeId, u8>,
    /// Startset: bytes that can begin a match (ASCII only, ASCII_RANGE bits)
    startset: u128,
    /// Whether the pattern is nullable at start (can match empty string)
    nullable_at_start: bool,
    state_limit: usize,
    /// Literal prefix for fast searching
    prefix_accel: PrefixAccelerator,
    /// State after consuming the literal prefix (for skipping ahead)
    prefix_end_state: Option<RegexNodeId>,
    /// Pre-computed DFA for high-throughput mode (None if not pre-computed)
    precomputed_dfa: Option<PrecomputedDfa>,
}

impl Regex {
    /// Compile a regex pattern into a `Regex` object.
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is invalid or contains unsupported syntax.
    ///
    /// # Example
    ///
    /// ```
    /// use resharp_rs::Regex;
    ///
    /// let re = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
    /// ```
    pub fn new(pattern: &str) -> Result<Self> {
        let parser = Parser::new(pattern);
        let (root, mut builder) = parser.parse().map_err(Error::Parse)?;

        // Compute startset for the root node
        let startset = compute_startset(&builder, root);
        let nullable_at_start = is_nullable(&builder, LocationKind::Begin, root);

        // Extract literal prefix for fast searching
        let (prefix_accel, prefix_end_state) = compute_prefix_acceleration(&mut builder, root);

        let mut regex = Self {
            builder,
            root,
            transitions: FxHashMap::default(),
            nullable_cache: FxHashMap::default(),
            startset,
            nullable_at_start,
            state_limit: DEFAULT_STATE_LIMIT,
            prefix_accel,
            prefix_end_state,
            precomputed_dfa: None,
        };

        // Automatically pre-compute DFA for high-throughput matching
        regex.precompute_dfa_internal(DEFAULT_PRECOMPUTE_LIMIT);

        Ok(regex)
    }

    /// Set the maximum number of DFA states allowed during matching.
    ///
    /// If this limit is exceeded, the match will fail. This prevents
    /// pathological patterns from consuming excessive memory.
    pub fn with_state_limit(mut self, limit: usize) -> Self {
        self.state_limit = limit;
        self
    }

    /// Internal method to pre-compute the DFA with a state limit.
    fn precompute_dfa_internal(&mut self, max_states: usize) -> bool {
        // Explore reachable states via BFS
        let mut state_to_idx: FxHashMap<RegexNodeId, u32> = FxHashMap::default();
        let mut idx_to_state: Vec<RegexNodeId> = Vec::new();
        let mut worklist: Vec<RegexNodeId> = Vec::new();

        // Start with the root state
        state_to_idx.insert(self.root, 0);
        idx_to_state.push(self.root);
        worklist.push(self.root);

        // Also track BOT state (dead state)
        let bot_idx = 1u32;
        state_to_idx.insert(node_id::BOT, bot_idx);
        idx_to_state.push(node_id::BOT);

        // BFS to discover all reachable states
        while let Some(state) = worklist.pop() {
            if state == node_id::BOT {
                continue; // Dead state has no outgoing transitions
            }

            // Compute all BYTE_RANGE transitions for this state
            for byte in 0u8..=255 {
                let next = self.transition_fast(state, byte);
                if let std::collections::hash_map::Entry::Vacant(e) = state_to_idx.entry(next) {
                    if idx_to_state.len() >= max_states {
                        // State limit exceeded, abort
                        return false;
                    }
                    let new_idx = idx_to_state.len() as u32;
                    e.insert(new_idx);
                    idx_to_state.push(next);
                    worklist.push(next);
                }
            }
        }

        let num_states = idx_to_state.len() as u32;

        // Build flat transition table
        let mut transitions = vec![bot_idx; (num_states as usize) * BYTE_RANGE];
        for (state_idx, &state) in idx_to_state.iter().enumerate() {
            if state == node_id::BOT {
                // Dead state stays dead for all bytes
                continue;
            }
            let base = state_idx * BYTE_RANGE;
            for byte in 0u8..=255 {
                let next = self.transition_fast(state, byte);
                let next_idx = *state_to_idx.get(&next).unwrap();
                transitions[base + byte as usize] = next_idx;
            }
        }

        // Build nullable bitmap
        let mut nullable = vec![false; num_states as usize];
        for (state_idx, &state) in idx_to_state.iter().enumerate() {
            nullable[state_idx] = self.is_nullable_cached(state, false);
        }

        self.precomputed_dfa = Some(PrecomputedDfa {
            transitions,
            nullable,
        });

        true
    }

    /// Check if the pattern matches anywhere in the input.
    ///
    /// # Example
    ///
    /// ```
    /// use resharp_rs::Regex;
    ///
    /// let mut re = Regex::new(r"\d+").unwrap();
    /// assert!(re.is_match("abc123xyz"));
    /// assert!(!re.is_match("no digits here"));
    /// ```
    pub fn is_match(&mut self, input: &str) -> bool {
        self.find(input).is_some()
    }

    /// Find the first match in the input (leftmost-longest).
    ///
    /// Returns the leftmost-longest match as per POSIX semantics.
    /// The returned [`Match`] contains the start and end byte offsets.
    ///
    /// # Example
    ///
    /// ```
    /// use resharp_rs::Regex;
    ///
    /// let mut re = Regex::new(r"[a-z]+").unwrap();
    /// let m = re.find("123abc456").unwrap();
    /// assert_eq!(m.start, 3);
    /// assert_eq!(m.end, 6);
    /// ```
    pub fn find(&mut self, input: &str) -> Option<Match> {
        let bytes = input.as_bytes();
        let n = bytes.len();

        // Special case: empty string - check with BeginEnd location
        if n == 0 {
            if is_nullable(&self.builder, LocationKind::BeginEnd, self.root) {
                return Some(Match::new(0, 0));
            }
            return None;
        }

        // If pattern is nullable at start, check for empty match or longer match
        if self.nullable_at_start {
            if let Some(end) = self.match_from_bytes(bytes, 0) {
                return Some(Match::new(0, end));
            }
        }

        // Use prefix accelerator for fast searching when available
        // Clone to avoid borrow issues with mutable self
        let accel = self.prefix_accel.clone();
        match accel {
            PrefixAccelerator::SingleByte(b) => {
                return self.find_with_single_byte_prefix(bytes, b);
            }
            PrefixAccelerator::Literal(ref prefix) if prefix.len() >= 2 => {
                let prefix = prefix.clone();
                return self.find_with_literal_prefix(bytes, &prefix);
            }
            PrefixAccelerator::StartBytes(ref start_bytes) => {
                let start_bytes = start_bytes.clone();
                return self.find_with_startset(bytes, &start_bytes);
            }
            _ => {}
        }

        // Fallback: use startset to skip positions that can't match
        let mut start = 0;
        while start < n {
            let b = bytes[start];
            // Skip if this byte can't start a match (ASCII only optimization)
            if b < 128 && (self.startset & (1u128 << b)) == 0 {
                start += 1;
                continue;
            }

            if let Some(end) = self.match_from_bytes(bytes, start) {
                return Some(Match::new(start, end));
            }
            // Advance to next UTF-8 character boundary
            start += utf8_char_width(b);
        }

        // Check for empty match at end if pattern is nullable at both start and end
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            return Some(Match::new(n, n));
        }

        None
    }

    /// Find using single-byte prefix with memchr
    fn find_with_single_byte_prefix(&mut self, bytes: &[u8], needle: u8) -> Option<Match> {
        let n = bytes.len();
        let mut start = 0;

        while let Some(pos) = memchr::memchr(needle, &bytes[start..]) {
            let abs_pos = start + pos;
            // Use prefix-aware matching to skip re-computing the first byte transition
            if let Some(end) = self.match_from_bytes_with_prefix(bytes, abs_pos, 1) {
                return Some(Match::new(abs_pos, end));
            }
            start = abs_pos + 1;
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            return Some(Match::new(n, n));
        }

        None
    }

    /// Find using multi-byte literal prefix with memmem
    fn find_with_literal_prefix(&mut self, bytes: &[u8], prefix: &[u8]) -> Option<Match> {
        let n = bytes.len();
        let finder = memmem::Finder::new(prefix);
        let prefix_len = prefix.len();
        let mut start = 0;

        while let Some(pos) = finder.find(&bytes[start..]) {
            let abs_pos = start + pos;
            // Match from the prefix position, but we can skip prefix validation
            if let Some(end) = self.match_from_bytes_with_prefix(bytes, abs_pos, prefix_len) {
                return Some(Match::new(abs_pos, end));
            }
            start = abs_pos + 1;
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            return Some(Match::new(n, n));
        }

        None
    }

    /// Find using startset with memchr2/memchr3
    fn find_with_startset(&mut self, bytes: &[u8], start_bytes: &[u8]) -> Option<Match> {
        let n = bytes.len();
        let mut start = 0;

        while start < n {
            // Find next position that could start a match
            let pos = match start_bytes.len() {
                1 => memchr::memchr(start_bytes[0], &bytes[start..]),
                2 => memchr::memchr2(start_bytes[0], start_bytes[1], &bytes[start..]),
                3 => memchr::memchr3(
                    start_bytes[0],
                    start_bytes[1],
                    start_bytes[2],
                    &bytes[start..],
                ),
                _ => None,
            };

            match pos {
                Some(rel_pos) => {
                    let abs_pos = start + rel_pos;
                    if let Some(end) = self.match_from_bytes(bytes, abs_pos) {
                        return Some(Match::new(abs_pos, end));
                    }
                    start = abs_pos + utf8_char_width(bytes[abs_pos]);
                }
                None => break,
            }
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            return Some(Match::new(n, n));
        }

        None
    }

    /// Match from a position where we know the literal prefix already matched
    fn match_from_bytes_with_prefix(
        &mut self,
        bytes: &[u8],
        start: usize,
        prefix_len: usize,
    ) -> Option<usize> {
        let n = bytes.len();
        let prefix_end = start + prefix_len;

        // If we have a pre-computed state after the prefix, start from there
        let state = if let Some(prefix_state) = self.prefix_end_state {
            prefix_state
        } else {
            // Fall back to normal matching
            return self.match_from_bytes(bytes, start);
        };

        let mut current_state = state;
        let mut last_match: Option<usize> = None;

        // Check if state after prefix is nullable
        if self.is_nullable_cached(current_state, prefix_end == n) {
            last_match = Some(prefix_end);
        }

        // Continue matching from after the prefix using fast path
        #[allow(clippy::needless_range_loop)] // We need `i` for position checks
        for i in prefix_end..n {
            let b = bytes[i];

            // Use fast transition (Center location, cached)
            current_state = self.transition_fast(current_state, b);

            if current_state == node_id::BOT {
                break;
            }

            if self.is_nullable_cached(current_state, i + 1 == n) {
                last_match = Some(i + 1);
            }
        }

        last_match
    }

    /// Find all non-overlapping matches.
    ///
    /// Returns a vector of all matches, where each match is the leftmost-longest
    /// starting from the position after the previous match ended.
    ///
    /// # Example
    ///
    /// ```
    /// use resharp_rs::Regex;
    ///
    /// let mut re = Regex::new(r"\d+").unwrap();
    /// let matches = re.find_all("a1b22c333");
    /// assert_eq!(matches.len(), 3);
    /// assert_eq!(&"a1b22c333"[matches[0].start..matches[0].end], "1");
    /// assert_eq!(&"a1b22c333"[matches[1].start..matches[1].end], "22");
    /// assert_eq!(&"a1b22c333"[matches[2].start..matches[2].end], "333");
    /// ```
    pub fn find_all(&mut self, input: &str) -> Vec<Match> {
        let mut matches = Vec::new();
        let bytes = input.as_bytes();
        let n = bytes.len();

        // Special case: empty string - check with BeginEnd location
        if n == 0 {
            if is_nullable(&self.builder, LocationKind::BeginEnd, self.root) {
                matches.push(Match::new(0, 0));
            }
            return matches;
        }

        // Use prefix accelerator for fast searching when available
        let accel = self.prefix_accel.clone();
        match accel {
            PrefixAccelerator::SingleByte(needle) => {
                return self.find_all_with_single_byte_prefix(bytes, needle);
            }
            PrefixAccelerator::Literal(ref prefix) if prefix.len() >= 2 => {
                let prefix = prefix.clone();
                return self.find_all_with_literal_prefix(bytes, &prefix);
            }
            PrefixAccelerator::StartBytes(ref start_bytes) => {
                let start_bytes = start_bytes.clone();
                return self.find_all_with_startset(bytes, &start_bytes);
            }
            _ => {}
        }

        // Fallback: linear scan with startset filtering
        let mut pos = 0;

        while pos < n {
            let b = bytes[pos];
            // Skip if this byte can't start a match (ASCII only optimization)
            if b < 128 && (self.startset & (1u128 << b)) == 0 && !self.nullable_at_start {
                pos += 1;
                continue;
            }

            if let Some(end) = self.match_from_bytes(bytes, pos) {
                matches.push(Match::new(pos, end));
                // Move past the match (at least one byte to avoid infinite loop)
                if end > pos {
                    pos = end;
                } else {
                    pos += utf8_char_width(b);
                }
            } else {
                // Advance to next UTF-8 character boundary
                pos += utf8_char_width(b);
            }
        }

        // Check for empty match at end if pattern is nullable at End
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            matches.push(Match::new(n, n));
        }

        matches
    }

    /// Find all matches using single-byte prefix with memchr
    fn find_all_with_single_byte_prefix(&mut self, bytes: &[u8], needle: u8) -> Vec<Match> {
        let n = bytes.len();
        let mut matches = Vec::new();
        let mut start = 0;

        while let Some(pos) = memchr::memchr(needle, &bytes[start..]) {
            let abs_pos = start + pos;
            if let Some(end) = self.match_from_bytes_with_prefix(bytes, abs_pos, 1) {
                matches.push(Match::new(abs_pos, end));
                // Skip past the match
                if end > abs_pos {
                    start = end;
                } else {
                    start = abs_pos + 1;
                }
            } else {
                start = abs_pos + 1;
            }
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            matches.push(Match::new(n, n));
        }

        matches
    }

    /// Find all matches using multi-byte literal prefix with memmem
    fn find_all_with_literal_prefix(&mut self, bytes: &[u8], prefix: &[u8]) -> Vec<Match> {
        let n = bytes.len();
        let mut matches = Vec::new();
        let finder = memmem::Finder::new(prefix);
        let prefix_len = prefix.len();
        let mut start = 0;

        while let Some(pos) = finder.find(&bytes[start..]) {
            let abs_pos = start + pos;
            if let Some(end) = self.match_from_bytes_with_prefix(bytes, abs_pos, prefix_len) {
                matches.push(Match::new(abs_pos, end));
                // Skip past the match
                if end > abs_pos {
                    start = end;
                } else {
                    start = abs_pos + 1;
                }
            } else {
                start = abs_pos + 1;
            }
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            matches.push(Match::new(n, n));
        }

        matches
    }

    /// Find all matches using startset with memchr2/memchr3
    fn find_all_with_startset(&mut self, bytes: &[u8], start_bytes: &[u8]) -> Vec<Match> {
        let n = bytes.len();
        let mut matches = Vec::new();
        let mut start = 0;

        while start < n {
            // Find next position that could start a match
            let pos = match start_bytes.len() {
                1 => memchr::memchr(start_bytes[0], &bytes[start..]),
                2 => memchr::memchr2(start_bytes[0], start_bytes[1], &bytes[start..]),
                3 => memchr::memchr3(
                    start_bytes[0],
                    start_bytes[1],
                    start_bytes[2],
                    &bytes[start..],
                ),
                _ => None,
            };

            match pos {
                Some(rel_pos) => {
                    let abs_pos = start + rel_pos;
                    if let Some(end) = self.match_from_bytes(bytes, abs_pos) {
                        matches.push(Match::new(abs_pos, end));
                        if end > abs_pos {
                            start = end;
                        } else {
                            start = abs_pos + utf8_char_width(bytes[abs_pos]);
                        }
                    } else {
                        start = abs_pos + utf8_char_width(bytes[abs_pos]);
                    }
                }
                None => break,
            }
        }

        // Check for empty match at end if pattern is nullable
        if is_nullable(&self.builder, LocationKind::End, self.root) {
            matches.push(Match::new(n, n));
        }

        matches
    }

    /// Match from a given byte position, return end byte position of longest match.
    fn match_from_bytes(&mut self, bytes: &[u8], start: usize) -> Option<usize> {
        let n = bytes.len();

        // For non-boundary positions, use pre-computed DFA if available
        // (This is the common case in find_all and internal searching)
        if start > 0 && start < n && self.precomputed_dfa.is_some() {
            return self.match_from_bytes_precomputed(bytes, start);
        }

        let mut state = self.root;
        let mut last_match: Option<usize> = None;

        // Determine location for the start position
        let start_loc = if start == 0 && n == 0 {
            LocationKind::BeginEnd
        } else if start == 0 {
            LocationKind::Begin
        } else if start == n {
            LocationKind::End
        } else {
            LocationKind::Center
        };

        // Check if initial state is nullable (for empty matches)
        if is_nullable(&self.builder, start_loc, state) {
            last_match = Some(start);
        }

        // Process bytes
        #[allow(clippy::needless_range_loop)] // We need `i` for position and end checks
        for i in start..n {
            let b = bytes[i];

            // Determine location: Begin only for position 0, otherwise Center
            // (End is handled via nullable check, not transition)
            let loc = if i == 0 {
                LocationKind::Begin
            } else {
                LocationKind::Center
            };

            // Get transition - use fast path for Center (common case)
            state = if loc == LocationKind::Center {
                self.transition_fast(state, b)
            } else {
                self.transition(state, b, loc)
            };

            if state == node_id::BOT {
                break;
            }

            // Check for match - use cached nullable for Center, full check for End
            let is_end = i + 1 == n;
            if self.is_nullable_cached(state, is_end) {
                last_match = Some(i + 1);
            }
        }

        last_match
    }

    /// Fast matching using pre-computed DFA (Center location only).
    /// Returns the end position of the longest match, or None if no match.
    /// This is optimized for high throughput - the DFA must be pre-computed.
    #[inline]
    fn match_from_bytes_precomputed(&self, bytes: &[u8], start: usize) -> Option<usize> {
        let dfa = self.precomputed_dfa.as_ref()?;

        let n = bytes.len();
        let mut last_match: Option<usize> = None;

        // Get initial state index (root is always index 0)
        let mut state_idx = 0u32;

        // Check if initial state is nullable
        if dfa.nullable[state_idx as usize] {
            last_match = Some(start);
        }

        // Dead state index (BOT is always index 1)
        let dead_state = 1u32;

        // Process bytes using flat table lookups
        #[allow(clippy::needless_range_loop)] // We need `i` for position tracking
        for i in start..n {
            let b = bytes[i];
            let base = (state_idx as usize) * BYTE_RANGE;
            state_idx = dfa.transitions[base + b as usize];

            if state_idx == dead_state {
                break;
            }

            if dfa.nullable[state_idx as usize] {
                last_match = Some(i + 1);
            }
        }

        last_match
    }

    /// Get or compute the transition for a byte.
    #[inline]
    fn transition(&mut self, state: RegexNodeId, byte: u8, loc: LocationKind) -> RegexNodeId {
        // Note: We don't cache Begin/End transitions since they depend on position
        // Only Center transitions are cached
        if loc == LocationKind::Center {
            return self.transition_fast(state, byte);
        }

        // Compute derivative for this byte (Begin/End locations)
        let minterm = CharSet::ascii_singleton(byte);
        derivative(&mut self.builder, loc, minterm, state)
    }

    /// Fast transition for Center location (the common case).
    /// Computes transitions lazily with single-byte granularity for simplicity and performance.
    #[inline]
    fn transition_fast(&mut self, state: RegexNodeId, byte: u8) -> RegexNodeId {
        // Use entry API to avoid double lookup
        let trans = self
            .transitions
            .entry(state)
            .or_insert_with(|| Box::new([UNCOMPUTED; BYTE_RANGE]));

        let cached = trans[byte as usize];
        if cached != UNCOMPUTED {
            return cached;
        }

        // Compute and cache using pre-computed singleton
        let minterm = CharSet::ascii_singleton(byte);
        let next = derivative(&mut self.builder, LocationKind::Center, minterm, state);
        trans[byte as usize] = next;

        next
    }

    /// Check if a state is nullable, using cache for Center location.
    #[inline]
    fn is_nullable_cached(&mut self, state: RegexNodeId, is_end: bool) -> bool {
        if is_end {
            return is_nullable(&self.builder, LocationKind::End, state);
        }

        // Use entry API to avoid double lookup
        *self.nullable_cache.entry(state).or_insert_with(|| {
            if is_nullable(&self.builder, LocationKind::Center, state) {
                nullable_status::NULLABLE
            } else {
                nullable_status::NOT_NULLABLE
            }
        }) == nullable_status::NULLABLE
    }
}

/// Get the width of a UTF-8 character from its first byte.
#[inline]
fn utf8_char_width(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

/// Compute the startset (set of bytes that can start a match) for a regex node.
/// Returns a 128-bit bitmap for ASCII characters.
fn compute_startset(builder: &RegexBuilder<CharSet>, node_id: RegexNodeId) -> u128 {
    compute_startset_inner(builder, node_id, &mut FxHashMap::default())
}

fn compute_startset_inner(
    builder: &RegexBuilder<CharSet>,
    node_id: RegexNodeId,
    cache: &mut FxHashMap<RegexNodeId, u128>,
) -> u128 {
    // Check cache first
    if let Some(&cached) = cache.get(&node_id) {
        return cached;
    }

    // Handle well-known nodes
    let result = match node_id {
        node_id::BOT => 0,           // Empty language - no startset
        node_id::EPS => 0,           // Epsilon - no startset (matches empty)
        node_id::TOP => !0u128,      // Any char - all ASCII
        node_id::TOP_STAR => !0u128, // Any string - all ASCII
        node_id::TOP_PLUS => !0u128, // Any non-empty string - all ASCII
        node_id::BEGIN_ANCHOR => 0,  // Anchor - no startset
        node_id::END_ANCHOR => 0,    // Anchor - no startset
        _ => {
            match builder.node(node_id).clone() {
                RegexNode::Singleton(set) => set.ascii,

                RegexNode::Loop { node, low, .. } => {
                    if low == 0 {
                        // Can match empty, so startset includes inner's startset
                        compute_startset_inner(builder, node, cache)
                    } else {
                        compute_startset_inner(builder, node, cache)
                    }
                }

                RegexNode::Or(nodes) => {
                    // Union of all children's startsets
                    let mut result = 0u128;
                    for n in nodes {
                        result |= compute_startset_inner(builder, n, cache);
                    }
                    result
                }

                RegexNode::And(nodes) => {
                    // Intersection of all children's startsets
                    if nodes.is_empty() {
                        !0u128 // Empty And = TOP
                    } else {
                        let mut result = !0u128;
                        for n in nodes {
                            result &= compute_startset_inner(builder, n, cache);
                        }
                        result
                    }
                }

                RegexNode::Not(inner) => {
                    // Complement of inner's startset
                    !compute_startset_inner(builder, inner, cache)
                }

                RegexNode::Concat { head, tail } => {
                    let head_startset = compute_startset_inner(builder, head, cache);
                    // If head can be nullable (at any location), tail's startset also contributes
                    // We check Begin since that's the relevant location for startset (matching from start)
                    if is_nullable(builder, LocationKind::Begin, head) {
                        head_startset | compute_startset_inner(builder, tail, cache)
                    } else {
                        head_startset
                    }
                }

                RegexNode::LookAround { node, .. } => compute_startset_inner(builder, node, cache),

                RegexNode::Begin | RegexNode::End => 0,
            }
        }
    };

    cache.insert(node_id, result);
    result
}

/// Compute prefix acceleration by extracting literal prefix and precomputing
/// the state after consuming it.
fn compute_prefix_acceleration(
    builder: &mut RegexBuilder<CharSet>,
    root: RegexNodeId,
) -> (PrefixAccelerator, Option<RegexNodeId>) {
    let mut prefix_bytes = Vec::new();
    let mut current = root;

    // Walk through concatenations collecting literal characters
    loop {
        match builder.node(current).clone() {
            RegexNode::Concat { head, tail } => {
                // Try to extract a literal from head
                if let Some(byte) = extract_singleton_byte(builder, head) {
                    prefix_bytes.push(byte);
                    current = tail;
                } else {
                    break;
                }
            }
            // Single character at the end (not concatenated)
            _ => {
                if prefix_bytes.is_empty() {
                    if let Some(byte) = extract_singleton_byte(builder, current) {
                        prefix_bytes.push(byte);
                        // current would be EPS after consuming the single char,
                        // but we don't need to track it since we break immediately
                    }
                }
                break;
            }
        }
    }

    // Compute the state after consuming the prefix by computing derivatives
    let prefix_end_state = if !prefix_bytes.is_empty() {
        let mut state = root;
        for &byte in &prefix_bytes {
            let minterm = CharSet::ascii_singleton(byte);
            // Use Center location since we're in the middle of matching
            state = derivative(builder, LocationKind::Center, minterm, state);
            if state == node_id::BOT {
                // Prefix can't match - shouldn't happen for a valid pattern
                return (PrefixAccelerator::None, None);
            }
        }
        Some(state)
    } else {
        None
    };

    // Check if pattern is nullable - can't use prefix acceleration for nullable patterns
    // since they can match empty strings at any position
    let is_nullable_start = is_nullable(builder, LocationKind::Begin, root);

    // Determine the accelerator type
    match prefix_bytes.len() {
        0 => {
            // No literal prefix - try to extract startset bytes for memchr acceleration
            // But only if the pattern isn't nullable (can't match empty at any position)
            if !is_nullable_start {
                let startset = compute_startset(builder, root);
                let count = startset.count_ones() as usize;
                if count > 0 && count <= 3 {
                    // Small enough to use memchr2/memchr3
                    let mut start_bytes = Vec::with_capacity(count);
                    for i in 0..(ASCII_RANGE as u8) {
                        if (startset & (1u128 << i)) != 0 {
                            start_bytes.push(i);
                        }
                    }
                    return (PrefixAccelerator::StartBytes(start_bytes), None);
                }
            }
            (PrefixAccelerator::None, None)
        }
        1 => (
            PrefixAccelerator::SingleByte(prefix_bytes[0]),
            prefix_end_state,
        ),
        _ => (PrefixAccelerator::Literal(prefix_bytes), prefix_end_state),
    }
}

/// Try to extract a single ASCII byte from a singleton node.
fn extract_singleton_byte(builder: &RegexBuilder<CharSet>, node: RegexNodeId) -> Option<u8> {
    match builder.node(node) {
        RegexNode::Singleton(charset) => {
            // Check if this is a single ASCII character
            let ascii = charset.ascii;
            if ascii.count_ones() == 1 {
                // Find which bit is set
                for i in 0..(ASCII_RANGE as u8) {
                    if (ascii & (1u128 << i)) != 0 {
                        return Some(i);
                    }
                }
            }
            None
        }
        _ => None,
    }
}
