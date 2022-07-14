/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utility for grouping commits with their primordials.

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

/// A group of commits, and their assigned primordial commit (if known).
#[derive(Debug)]
pub(crate) struct Group<T> {
    /// The assigned primordial commit.
    primordial: Option<T>,

    /// The commits in the group.
    members: HashSet<T>,
}

/// A utility struct to aid grouping of commits and assignment of primordials.
///
/// This struct collects groups of commits together that will be assigned to
/// the same primordial commit.
///
/// Groups can be merged once it is known that they will end up with the same
/// primordial assignment.
#[derive(Debug)]
pub(crate) struct Grouper<T> {
    /// The groups of commits.
    groups: Vec<Group<T>>,

    /// A map from member commit to the group they are currently part of
    /// (by index into the `groups` vector).
    member_groups: HashMap<T, usize>,
}

impl<T: Copy + Hash + Eq> Group<T> {
    fn new() -> Group<T> {
        Group {
            primordial: None,
            members: HashSet::new(),
        }
    }

    fn with_member(mut self, member: T) -> Group<T> {
        self.members.insert(member);
        self
    }

    fn with_primordial(mut self, primordial: T) -> Group<T> {
        self.primordial = Some(primordial);
        self
    }
}

impl<T: Copy + Hash + Eq> Grouper<T> {
    /// Create a new grouper to group sets of commits and assign primordials.
    pub(crate) fn new() -> Grouper<T> {
        Grouper {
            groups: Vec::new(),
            member_groups: HashMap::new(),
        }
    }

    /// Set the primordial commit for the group containing `member`.  If
    /// no such group exists then a group with that member will be created
    /// and its primordial assigned.
    pub(crate) fn set_primordial(&mut self, member: T, primordial: T) {
        match self.member_groups.get(&member) {
            Some(&group) => self.groups[group].primordial = Some(primordial),
            None => {
                self.member_groups.insert(member, self.groups.len());
                self.groups
                    .push(Group::new().with_member(member).with_primordial(primordial));
            }
        }
    }

    /// Merge the group for `other` into the group for `member`.  If
    /// either don't exist, then they are added to the other's group.
    /// If neither exist, a new group is created.
    pub(crate) fn merge(&mut self, member: T, other: T) {
        match (
            self.member_groups.get(&member),
            self.member_groups.get(&other),
        ) {
            (Some(&group1), Some(&group2)) => {
                if group1 != group2 {
                    if self.groups[group1].members.len() > self.groups[group2].members.len() {
                        self.move_group(group1, group2);
                    } else {
                        self.move_group(group2, group1);
                    }
                }
            }
            (Some(&group), None) => {
                self.groups[group].members.insert(other);
                self.member_groups.insert(other, group);
            }
            (None, Some(&group)) => {
                self.groups[group].members.insert(member);
                self.member_groups.insert(member, group);
            }
            (None, None) => {
                self.member_groups.insert(member, self.groups.len());
                self.member_groups.insert(other, self.groups.len());
                self.groups
                    .push(Group::new().with_member(member).with_member(other));
            }
        }
    }

    /// Move all members of `src` to `dest`.
    fn move_group(&mut self, dest: usize, src: usize) {
        let members = std::mem::take(&mut self.groups[src].members);
        for member in members {
            self.member_groups.insert(member, dest);
            self.groups[dest].members.insert(member);
        }
        if let Some(primordial) = self.groups[src].primordial.take() {
            self.groups[dest].primordial = Some(primordial);
        }
    }

    /// Iterate over all groups, returning their primordial (if known) and members.
    pub(crate) fn groups(self) -> impl Iterator<Item = (Option<T>, HashSet<T>)> {
        self.groups
            .into_iter()
            .map(|group| (group.primordial, group.members))
    }
}
