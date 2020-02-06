/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use std::cmp::{self, Ordering};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::iter::Iterator;

use ::manifest::Entry;
use mononoke_types::{hash::GitSha1, MPathElement};

use crate::errors::ErrorKind;
use crate::mode;
use crate::thrift;
use crate::{BlobHandle, ObjectKind};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct TreeHandle {
    oid: GitSha1,
}

impl TreeHandle {
    pub fn filemode(&self) -> i32 {
        mode::GIT_FILEMODE_TREE
    }

    pub fn oid(&self) -> &GitSha1 {
        &self.oid
    }

    pub fn blobstore_key(&self) -> String {
        format!("git.tree.{}", self.oid)
    }
}

impl TryFrom<thrift::TreeHandle> for TreeHandle {
    type Error = Error;

    fn try_from(t: thrift::TreeHandle) -> Result<Self, Error> {
        let size = t.size.try_into()?;
        let oid = GitSha1::from_bytes(&t.oid.0, ObjectKind::Tree.as_str(), size)?;
        Ok(Self { oid })
    }
}

impl Into<thrift::TreeHandle> for TreeHandle {
    fn into(self) -> thrift::TreeHandle {
        let size = self.oid.size();

        thrift::TreeHandle {
            oid: self.oid.into_thrift(),
            size: size.try_into().expect("Tree size must fit in a i64"),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum TreeMember {
    Blob(BlobHandle),
    Tree(TreeHandle),
}

impl Into<Entry<TreeHandle, BlobHandle>> for TreeMember {
    fn into(self) -> Entry<TreeHandle, BlobHandle> {
        match self {
            Self::Blob(handle) => Entry::Leaf(handle),
            Self::Tree(handle) => Entry::Tree(handle),
        }
    }
}

impl From<Entry<TreeHandle, BlobHandle>> for TreeMember {
    fn from(entry: Entry<TreeHandle, BlobHandle>) -> Self {
        match entry {
            Entry::Leaf(handle) => Self::Blob(handle),
            Entry::Tree(handle) => Self::Tree(handle),
        }
    }
}

impl TreeMember {
    pub fn filemode(&self) -> i32 {
        match self {
            Self::Blob(ref blob) => blob.filemode(),
            Self::Tree(ref tree) => tree.filemode(),
        }
    }

    pub fn oid(&self) -> &GitSha1 {
        match self {
            Self::Blob(ref blob) => blob.oid(),
            Self::Tree(ref tree) => tree.oid(),
        }
    }

    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Blob(..) => ObjectKind::Blob,
            Self::Tree(..) => ObjectKind::Tree,
        }
    }
}

impl TryFrom<thrift::TreeMember> for TreeMember {
    type Error = Error;

    fn try_from(t: thrift::TreeMember) -> Result<Self, Error> {
        match t {
            thrift::TreeMember::Blob(blob) => Ok(Self::Blob(blob.try_into()?)),
            thrift::TreeMember::Tree(tree) => Ok(Self::Tree(tree.try_into()?)),
            thrift::TreeMember::UnknownField(..) => Err(ErrorKind::InvalidThrift.into()),
        }
    }
}

impl Into<thrift::TreeMember> for TreeMember {
    fn into(self) -> thrift::TreeMember {
        match self {
            Self::Blob(blob) => thrift::TreeMember::Blob(blob.into()),
            Self::Tree(tree) => thrift::TreeMember::Tree(tree.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tree {
    handle: TreeHandle,
    members: HashMap<MPathElement, TreeMember>,
}

impl Tree {
    pub fn handle(&self) -> &TreeHandle {
        &self.handle
    }
}

impl TryFrom<thrift::Tree> for Tree {
    type Error = Error;

    fn try_from(t: thrift::Tree) -> Result<Self, Error> {
        let handle = t.handle.try_into()?;

        let members = t
            .members
            .into_iter()
            .map(|(path, member)| {
                let path = MPathElement::from_thrift(path)?;
                let member = member.try_into()?;
                Ok((path, member))
            })
            .collect::<Result<HashMap<_, _>, Error>>()?;

        Ok(Self { handle, members })
    }
}

impl Into<thrift::Tree> for Tree {
    fn into(self) -> thrift::Tree {
        let Tree { handle, members } = self;

        let members = members
            .into_iter()
            .map(|(path, member)| (path.into_thrift(), member.into()))
            .collect();

        thrift::Tree {
            handle: handle.into(),
            members,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TreeBuilder {
    members: HashMap<MPathElement, TreeMember>,
}

impl TreeBuilder {
    // TODO: Can we verify members here (git_path_isvalid)
    pub fn new(members: HashMap<MPathElement, TreeMember>) -> Self {
        Self { members }
    }
}

impl Into<Tree> for TreeBuilder {
    fn into(self) -> Tree {
        let mut object_buff = Vec::new();
        self.write_serialized_object(&mut object_buff)
            .expect("Writes to Vec cannot fail");

        let oid = ObjectKind::Tree.create_oid(&object_buff);

        Tree {
            handle: TreeHandle { oid },
            members: self.members,
        }
    }
}

pub trait Treeish {
    fn members(&self) -> &HashMap<MPathElement, TreeMember>;

    fn write_serialized_object(&self, writer: &mut impl Write) -> Result<(), io::Error> {
        for (path, member) in iter_members_git_path_order(self.members()) {
            write!(writer, "{:o} ", member.filemode())?;
            writer.write_all(path.as_ref())?;
            writer.write_all(&[0])?;
            writer.write_all(member.oid().as_ref())?;
        }

        Ok(())
    }

    fn write_humanized_representation(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (path, member) in iter_members_git_path_order(self.members()) {
            write!(
                f,
                "{:06o} {} {}\t{}\n",
                member.filemode(),
                member.kind().as_str(),
                member.oid(),
                path
            )?;
        }

        Ok(())
    }
}

impl Treeish for Tree {
    fn members(&self) -> &HashMap<MPathElement, TreeMember> {
        &self.members
    }
}

impl Treeish for TreeBuilder {
    fn members(&self) -> &HashMap<MPathElement, TreeMember> {
        &self.members
    }
}

impl Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.write_humanized_representation(f)
    }
}

impl Display for TreeBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.write_humanized_representation(f)
    }
}

fn iter_members_git_path_order(
    members: &HashMap<MPathElement, TreeMember>,
) -> impl Iterator<Item = (&MPathElement, &TreeMember)> {
    let mut members: Vec<_> = members.iter().collect();
    members.sort_by(|(p1, e1), (p2, e2)| git_path_cmp(p1, e1, p2, e2));
    members.into_iter()
}

// TODO: Expose git_path_cmp from libgit2 and use it here
// https://github.com/libgit2/libgit2/blob/fb439c975a2de33f5b0c317f3fdea49dc94b27dc/src/path.c#L850
fn git_path_cmp(
    p1: &MPathElement,
    e1: &TreeMember,
    p2: &MPathElement,
    e2: &TreeMember,
) -> Ordering {
    const NULL: u8 = 0;
    const SLASH: u8 = '/' as u8;

    let p1 = p1.as_ref();
    let p2 = p2.as_ref();
    let len = cmp::min(p1.len(), p2.len());

    let ordering = p1[..len].cmp(&p2[..len]);
    if ordering != Ordering::Equal {
        return ordering;
    }

    let c1 = p1
        .get(len)
        .unwrap_or(if e1.kind().is_tree() { &SLASH } else { &NULL });

    let c2 = p2
        .get(len)
        .unwrap_or(if e2.kind().is_tree() { &SLASH } else { &NULL });

    c1.cmp(c2)
}
