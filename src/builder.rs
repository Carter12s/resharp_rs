//! Regex node builder with structural sharing.
//!
//! The builder maintains a collection of regex nodes and ensures that
//! structurally identical nodes are shared (hash-consing).

use crate::types::{node_id, NodeFlags, RefSet, RegexNode, RegexNodeId, RegexNodeInfo};
use rustc_hash::FxHashMap;

/// Builder for regex nodes with structural sharing.
pub struct RegexBuilder<S: Clone + Eq + std::hash::Hash + Default> {
    /// All nodes stored by ID
    nodes: Vec<RegexNode<S>>,
    /// Node info (flags)
    infos: Vec<RegexNodeInfo>,
    /// Map from node to ID for deduplication (FxHashMap for performance)
    node_map: FxHashMap<RegexNode<S>, RegexNodeId>,
    /// Empty RefSet singleton
    empty_refset: RefSet,
    /// Zero RefSet singleton (position 0)
    zero_refset: RefSet,
}

impl<S: Clone + Eq + std::hash::Hash + Default> Default for RegexBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone + Eq + std::hash::Hash + Default> RegexBuilder<S> {
    pub fn new() -> Self {
        let mut builder = Self {
            nodes: Vec::new(),
            infos: Vec::new(),
            node_map: FxHashMap::default(),
            empty_refset: RefSet::empty(),
            zero_refset: RefSet::singleton(0),
        };

        // Initialize well-known nodes
        builder.init_builtin_nodes();
        builder
    }

    fn init_builtin_nodes(&mut self) {
        // BOT (empty language) - create a placeholder that matches nothing
        // We'll use Or([]) since empty alternation is bottom
        let bot = RegexNode::Or(vec![]);
        self.add_node_with_flags(bot, NodeFlags::NONE);

        // EPS (epsilon) - matches empty string
        // We'll use Loop(BOT, 0, 0) which matches exactly the empty string
        let eps = RegexNode::Loop {
            node: node_id::BOT,
            low: 0,
            high: 0,
        };
        self.add_node_with_flags(
            eps,
            NodeFlags::CAN_BE_NULLABLE | NodeFlags::IS_ALWAYS_NULLABLE,
        );

        // TOP (any single char) - placeholder, will be set by solver
        let top = RegexNode::And(vec![]); // placeholder
        self.add_node_with_flags(top, NodeFlags::NONE);

        // TOP_STAR (any string) - _*
        let top_star = RegexNode::Loop {
            node: node_id::TOP,
            low: 0,
            high: u32::MAX,
        };
        self.add_node_with_flags(
            top_star,
            NodeFlags::CAN_BE_NULLABLE | NodeFlags::IS_ALWAYS_NULLABLE,
        );

        // TOP_PLUS (any non-empty string) - _+
        let top_plus = RegexNode::Loop {
            node: node_id::TOP,
            low: 1,
            high: u32::MAX,
        };
        self.add_node_with_flags(top_plus, NodeFlags::NONE);

        // END_ANCHOR
        let end = RegexNode::End;
        self.add_node_with_flags(
            end,
            NodeFlags::CAN_BE_NULLABLE | NodeFlags::DEPENDS_ON_ANCHOR,
        );

        // BEGIN_ANCHOR
        let begin = RegexNode::Begin;
        self.add_node_with_flags(
            begin,
            NodeFlags::CAN_BE_NULLABLE | NodeFlags::DEPENDS_ON_ANCHOR,
        );
    }

    fn add_node_with_flags(&mut self, node: RegexNode<S>, flags: NodeFlags) -> RegexNodeId {
        let id = self.nodes.len() as RegexNodeId;
        self.nodes.push(node.clone());
        self.infos.push(RegexNodeInfo { flags });
        // Don't add builtin nodes to the map - they're special-cased
        id
    }

    /// Get a node by ID
    #[inline]
    pub fn node(&self, id: RegexNodeId) -> &RegexNode<S> {
        &self.nodes[id as usize]
    }

    /// Get node flags by ID (fast path for nullable checks)
    #[inline]
    pub fn flags(&self, id: RegexNodeId) -> NodeFlags {
        self.infos[id as usize].flags
    }

    /// Intern a node (returns existing ID if already present)
    fn intern(&mut self, node: RegexNode<S>) -> RegexNodeId {
        if let Some(&id) = self.node_map.get(&node) {
            return id;
        }

        // Compute flags for the new node
        let flags = self.compute_flags(&node);

        let id = self.nodes.len() as RegexNodeId;
        self.nodes.push(node.clone());
        self.infos.push(RegexNodeInfo { flags });
        self.node_map.insert(node, id);
        id
    }

    /// Compute NodeFlags for a node based on its structure.
    fn compute_flags(&self, node: &RegexNode<S>) -> NodeFlags {
        match node {
            RegexNode::Singleton(_) => NodeFlags::NONE,

            RegexNode::Concat { head, tail } => {
                let hf = self.flags(*head);
                let tf = self.flags(*tail);

                let mut flags = NodeFlags::NONE;

                // Both must be nullable for concat to be nullable
                if hf.is_always_nullable() && tf.is_always_nullable() {
                    flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                } else if hf.can_be_nullable() && tf.can_be_nullable() {
                    flags = flags | NodeFlags::CAN_BE_NULLABLE;
                }

                // Propagate anchor dependency
                if hf.depends_on_anchor() || tf.depends_on_anchor() {
                    flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                }

                flags
            }

            RegexNode::Loop {
                node: inner, low, ..
            } => {
                if *low == 0 {
                    // Can always match empty string
                    NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE
                } else {
                    // Inherits from inner node
                    let inner_flags = self.flags(*inner);
                    let mut flags = NodeFlags::NONE;
                    if inner_flags.is_always_nullable() {
                        flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                    } else if inner_flags.can_be_nullable() {
                        flags = flags | NodeFlags::CAN_BE_NULLABLE;
                    }
                    if inner_flags.depends_on_anchor() {
                        flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                    }
                    flags
                }
            }

            RegexNode::Or(nodes) => {
                let mut any_always = false;
                let mut any_can = false;
                let mut any_anchor = false;

                for &n in nodes {
                    let f = self.flags(n);
                    if f.is_always_nullable() {
                        any_always = true;
                    }
                    if f.can_be_nullable() {
                        any_can = true;
                    }
                    if f.depends_on_anchor() {
                        any_anchor = true;
                    }
                }

                let mut flags = NodeFlags::NONE;
                if any_always {
                    flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                } else if any_can {
                    flags = flags | NodeFlags::CAN_BE_NULLABLE;
                }
                if any_anchor {
                    flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                }
                flags
            }

            RegexNode::And(nodes) => {
                let mut all_always = true;
                let mut all_can = true;
                let mut any_anchor = false;

                for &n in nodes {
                    let f = self.flags(n);
                    if !f.is_always_nullable() {
                        all_always = false;
                    }
                    if !f.can_be_nullable() {
                        all_can = false;
                    }
                    if f.depends_on_anchor() {
                        any_anchor = true;
                    }
                }

                let mut flags = NodeFlags::NONE;
                if all_always {
                    flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                } else if all_can {
                    flags = flags | NodeFlags::CAN_BE_NULLABLE;
                }
                if any_anchor {
                    flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                }
                flags
            }

            RegexNode::Not(inner) => {
                let inner_flags = self.flags(*inner);
                let mut flags = NodeFlags::NONE;

                // ~(always_nullable) = never nullable
                // ~(never_nullable) = always nullable
                // ~(can_be_nullable) = can be nullable (depends)
                if !inner_flags.can_be_nullable() {
                    // Inner never matches empty, so complement always does
                    flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                } else if !inner_flags.is_always_nullable() {
                    // Inner might or might not match empty
                    flags = flags | NodeFlags::CAN_BE_NULLABLE;
                }
                // If inner has no anchor dependency and is always nullable,
                // then Not is never nullable (no CAN_BE_NULLABLE)

                if inner_flags.depends_on_anchor() {
                    flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                }
                flags
            }

            RegexNode::LookAround { node: inner, .. } => {
                // Lookaround nullability depends on inner node
                let inner_flags = self.flags(*inner);
                let mut flags = NodeFlags::CONTAINS_LOOKAROUND;
                if inner_flags.is_always_nullable() {
                    flags = flags | NodeFlags::IS_ALWAYS_NULLABLE | NodeFlags::CAN_BE_NULLABLE;
                } else if inner_flags.can_be_nullable() {
                    flags = flags | NodeFlags::CAN_BE_NULLABLE;
                }
                if inner_flags.depends_on_anchor() {
                    flags = flags | NodeFlags::DEPENDS_ON_ANCHOR;
                }
                flags
            }

            RegexNode::Begin | RegexNode::End => {
                NodeFlags::CAN_BE_NULLABLE | NodeFlags::DEPENDS_ON_ANCHOR
            }
        }
    }

    /// Create a singleton (character set) node
    pub fn mk_singleton(&mut self, set: S) -> RegexNodeId {
        self.intern(RegexNode::Singleton(set))
    }

    /// Create epsilon node
    pub fn mk_eps(&self) -> RegexNodeId {
        node_id::EPS
    }

    /// Create begin anchor
    pub fn mk_begin(&self) -> RegexNodeId {
        node_id::BEGIN_ANCHOR
    }

    /// Create end anchor
    pub fn mk_end(&self) -> RegexNodeId {
        node_id::END_ANCHOR
    }

    /// Create concatenation of two nodes
    pub fn mk_concat(&mut self, head: RegexNodeId, tail: RegexNodeId) -> RegexNodeId {
        // Simplifications
        if head == node_id::BOT || tail == node_id::BOT {
            return node_id::BOT;
        }
        if head == node_id::EPS {
            return tail;
        }
        if tail == node_id::EPS {
            return head;
        }

        self.intern(RegexNode::Concat { head, tail })
    }

    /// Create concatenation of multiple nodes (right-associative)
    pub fn mk_concat_many(&mut self, nodes: Vec<RegexNodeId>) -> RegexNodeId {
        nodes
            .into_iter()
            .rev()
            .fold(node_id::EPS, |acc, n| self.mk_concat(n, acc))
    }

    /// Create a loop (repetition) node
    pub fn mk_loop(&mut self, node: RegexNodeId, low: u32, high: u32) -> RegexNodeId {
        // Simplifications
        if low == 0 && high == 0 {
            return node_id::EPS;
        }
        if low == 1 && high == 1 {
            return node;
        }
        if node == node_id::BOT && low > 0 {
            return node_id::BOT;
        }
        if node == node_id::EPS {
            return node_id::EPS;
        }
        // _* is TOP_STAR
        if node == node_id::TOP && low == 0 && high == u32::MAX {
            return node_id::TOP_STAR;
        }

        self.intern(RegexNode::Loop { node, low, high })
    }

    /// Create alternation (union) of multiple nodes
    pub fn mk_or(&mut self, mut nodes: Vec<RegexNodeId>) -> RegexNodeId {
        // Flatten nested Ors and remove duplicates
        let mut flat = Vec::new();
        for id in nodes.drain(..) {
            if id == node_id::BOT {
                continue; // BOT is identity for Or
            }
            if id == node_id::TOP_STAR {
                return node_id::TOP_STAR; // TOP_STAR absorbs everything
            }
            match self.node(id) {
                RegexNode::Or(inner) => flat.extend(inner.iter().copied()),
                _ => flat.push(id),
            }
        }

        flat.sort();
        flat.dedup();

        match flat.len() {
            0 => node_id::BOT,
            1 => flat[0],
            _ => self.intern(RegexNode::Or(flat)),
        }
    }

    /// Create alternation of two nodes
    pub fn mk_or2(&mut self, a: RegexNodeId, b: RegexNodeId) -> RegexNodeId {
        self.mk_or(vec![a, b])
    }

    /// Create intersection of multiple nodes
    pub fn mk_and(&mut self, mut nodes: Vec<RegexNodeId>) -> RegexNodeId {
        // Flatten nested Ands and remove duplicates
        let mut flat = Vec::new();
        for id in nodes.drain(..) {
            if id == node_id::TOP_STAR {
                continue; // TOP_STAR is identity for And
            }
            if id == node_id::BOT {
                return node_id::BOT; // BOT absorbs everything
            }
            match self.node(id) {
                RegexNode::And(inner) => flat.extend(inner.iter().copied()),
                _ => flat.push(id),
            }
        }

        flat.sort();
        flat.dedup();

        match flat.len() {
            0 => node_id::TOP_STAR,
            1 => flat[0],
            _ => self.intern(RegexNode::And(flat)),
        }
    }

    /// Create complement (negation) of a node
    pub fn mk_not(&mut self, node: RegexNodeId) -> RegexNodeId {
        // Double negation elimination
        if let RegexNode::Not(inner) = self.node(node) {
            return *inner;
        }
        // ~BOT = TOP_STAR
        if node == node_id::BOT {
            return node_id::TOP_STAR;
        }
        // ~TOP_STAR = BOT
        if node == node_id::TOP_STAR {
            return node_id::BOT;
        }

        self.intern(RegexNode::Not(node))
    }

    /// Create a lookahead assertion
    pub fn mk_lookahead(&mut self, node: RegexNodeId) -> RegexNodeId {
        // (?=_*) is always true (matches epsilon)
        if node == node_id::TOP_STAR {
            return node_id::EPS;
        }
        // (?=BOT) never matches
        if node == node_id::BOT {
            return node_id::BOT;
        }

        self.intern(RegexNode::LookAround {
            node,
            look_back: false,
            relative_to: 0,
            pending_nullables: self.empty_refset.clone(),
        })
    }

    /// Create a lookbehind assertion
    pub fn mk_lookbehind(&mut self, node: RegexNodeId) -> RegexNodeId {
        if node == node_id::TOP_STAR {
            return node_id::EPS;
        }
        if node == node_id::BOT {
            return node_id::BOT;
        }

        self.intern(RegexNode::LookAround {
            node,
            look_back: true,
            relative_to: 0,
            pending_nullables: self.empty_refset.clone(),
        })
    }

    /// Get the empty refset
    /// Get the zero refset
    pub fn zero_refset(&self) -> &RefSet {
        &self.zero_refset
    }

    /// Create a lookahead with specific pending nullables
    pub fn intern_lookahead_with_pending(
        &mut self,
        node: RegexNodeId,
        relative_to: RegexNodeId,
        pending: RefSet,
    ) -> RegexNodeId {
        self.intern(RegexNode::LookAround {
            node,
            look_back: false,
            relative_to,
            pending_nullables: pending,
        })
    }
}
