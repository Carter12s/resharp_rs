//! Brzozowski derivative algorithm.
//!
//! This module implements the core derivative computation that transforms
//! a regex into the "remainder" after matching a character.

use crate::builder::RegexBuilder;
use crate::charset::CharSet;
use crate::types::{node_id, LocationKind, RegexNode, RegexNodeId};

/// Check if a node is nullable (matches empty string) at a given location.
///
/// This is optimized to use cached NodeFlags when possible, falling back
/// to recursive computation only when the node depends on anchors.
#[inline]
pub fn is_nullable(
    builder: &RegexBuilder<CharSet>,
    loc: LocationKind,
    node_id: RegexNodeId,
) -> bool {
    // Fast path: check cached flags first
    let flags = builder.flags(node_id);

    // If always nullable, return true immediately
    if flags.is_always_nullable() {
        return true;
    }

    // If cannot be nullable at all, return false immediately
    if !flags.can_be_nullable() {
        return false;
    }

    // The node depends on location (anchors) - need full computation
    is_nullable_slow(builder, loc, node_id)
}

/// Slow path for nullable computation when anchors are involved.
fn is_nullable_slow(
    builder: &RegexBuilder<CharSet>,
    loc: LocationKind,
    node_id: RegexNodeId,
) -> bool {
    // Handle well-known node IDs
    match node_id {
        node_id::BOT => return false,
        node_id::EPS => return true,
        node_id::TOP => return false,
        node_id::TOP_STAR => return true,
        node_id::TOP_PLUS => return false,
        node_id::END_ANCHOR => return loc.is_end(),
        node_id::BEGIN_ANCHOR => return loc.is_begin(),
        _ => {}
    }

    match builder.node(node_id) {
        RegexNode::Singleton(_) => false,
        RegexNode::Or(nodes) => nodes.iter().any(|&n| is_nullable(builder, loc, n)),
        RegexNode::And(nodes) => nodes.iter().all(|&n| is_nullable(builder, loc, n)),
        RegexNode::Loop { node, low, .. } => *low == 0 || is_nullable(builder, loc, *node),
        RegexNode::Not(inner) => !is_nullable(builder, loc, *inner),
        RegexNode::Concat { head, tail } => {
            is_nullable(builder, loc, *head) && is_nullable(builder, loc, *tail)
        }
        RegexNode::LookAround { node, .. } => is_nullable(builder, loc, *node),
        RegexNode::End => loc.is_end(),
        RegexNode::Begin => loc.is_begin(),
    }
}

/// Compute the derivative of a regex with respect to a character set (minterm).
pub fn derivative(
    builder: &mut RegexBuilder<CharSet>,
    loc: LocationKind,
    minterm: &CharSet,
    node_id: RegexNodeId,
) -> RegexNodeId {
    match builder.node(node_id).clone() {
        RegexNode::Singleton(set) => {
            // If minterm is contained in the set, derivative is epsilon
            if !set.intersection(minterm).is_empty() {
                node_id::EPS
            } else {
                node_id::BOT
            }
        }

        RegexNode::Loop { node, low, high } => {
            let decr = |x: u32| if x == u32::MAX || x == 0 { x } else { x - 1 };
            let r_decr = builder.mk_loop(node, decr(low), decr(high));
            let dr = derivative(builder, loc, minterm, node);
            builder.mk_concat(dr, r_decr)
        }

        RegexNode::Or(nodes) => {
            let mut derivs = Vec::new();
            for n in nodes {
                let d = derivative(builder, loc, minterm, n);
                if d != node_id::BOT {
                    derivs.push(d);
                }
            }
            match derivs.len() {
                0 => node_id::BOT,
                1 => derivs[0],
                _ => builder.mk_or(derivs),
            }
        }

        RegexNode::And(nodes) => {
            let mut derivs = Vec::new();
            for n in nodes {
                let d = derivative(builder, loc, minterm, n);
                if d == node_id::BOT {
                    return node_id::BOT;
                }
                if d != node_id::TOP_STAR {
                    derivs.push(d);
                }
            }
            match derivs.len() {
                0 => node_id::TOP_STAR,
                1 => derivs[0],
                _ => builder.mk_and(derivs),
            }
        }

        RegexNode::Not(inner) => {
            let d = derivative(builder, loc, minterm, inner);
            builder.mk_not(d)
        }

        RegexNode::Concat { head, tail } => {
            let dhead = derivative(builder, loc, minterm, head);
            let rs = builder.mk_concat(dhead, tail);

            if is_nullable(builder, loc, head) {
                let dtail = derivative(builder, loc, minterm, tail);
                if dtail == node_id::BOT {
                    rs
                } else if rs == node_id::BOT {
                    dtail
                } else {
                    builder.mk_or2(rs, dtail)
                }
            } else {
                rs
            }
        }

        RegexNode::LookAround {
            node,
            look_back,
            relative_to,
            pending_nullables,
        } => {
            let dr = derivative(builder, loc, minterm, node);

            if look_back {
                // Lookbehind: skip leading _* if present
                let inner = match builder.node(node).clone() {
                    RegexNode::Concat { head, tail } if head == node_id::TOP_STAR => tail,
                    _ => node,
                };
                let d = derivative(builder, loc, minterm, inner);
                builder.mk_lookbehind(d)
            } else {
                // Lookahead
                if pending_nullables.is_empty() && is_nullable(builder, loc, dr) {
                    let new_pending = builder.zero_refset().clone();
                    builder.intern_lookahead_with_pending(dr, relative_to + 1, new_pending)
                } else {
                    builder.intern_lookahead_with_pending(dr, relative_to + 1, pending_nullables)
                }
            }
        }

        RegexNode::Begin | RegexNode::End => node_id::BOT,
    }
}
