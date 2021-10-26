//! Tests about discontinuous segments
//!
//! Previously, segments in a group are continuous. In other words, all segments
//! in the master group can be represented using a single span `0..=x`.  With
//! discontinuous segments, a group might be represented as a few spans.
//!
//! The discontinuous spans are designed to better support multiple long-lived
//! long branches. For example:
//!
//! ```plain,ignore
//! 1---2---3--...--999---1000     branch1
//!      \
//!       5000--...--5999---6000   branch2
//! ```
//!
//! Note: discontinuous segments is not designed to support massive amount of
//! branches. It introduces O(branch) factor in complexity in many places.
