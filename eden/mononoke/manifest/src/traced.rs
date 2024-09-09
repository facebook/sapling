/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hash;
use std::hash::Hasher;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use mononoke_types::MPathElement;

use crate::types::Entry;
use crate::types::Manifest;

/// Traced allows you to trace a given parent through manifest derivation. For example, if you
/// assign ID 1 to a tree, then perform manifest derivation, then further entries you presented to
/// you that came from this parent will have the same ID.
#[derive(Debug)]
pub struct Traced<I, E>(Option<I>, E);

impl<I, E: Hash> Hash for Traced<I, E> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.1.hash(state);
    }
}

impl<I, E: PartialEq> PartialEq for Traced<I, E> {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

impl<I, E: Eq> Eq for Traced<I, E> {}

impl<I: Copy, E: Copy> Copy for Traced<I, E> {}

impl<I: Clone, E: Clone> Clone for Traced<I, E> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<I, E> Traced<I, E> {
    pub fn generate(e: E) -> Self {
        Self(None, e)
    }

    pub fn assign(i: I, e: E) -> Self {
        Self(Some(i), e)
    }

    pub fn id(&self) -> Option<&I> {
        self.0.as_ref()
    }

    pub fn untraced(&self) -> &E {
        &self.1
    }

    pub fn into_untraced(self) -> E {
        self.1
    }
}

impl<I: Copy, E> Traced<I, E> {
    fn inherit_into_entry<TreeId, LeafId>(
        &self,
        e: Entry<TreeId, LeafId>,
    ) -> Entry<Traced<I, TreeId>, Traced<I, LeafId>> {
        match e {
            Entry::Tree(t) => Entry::Tree(Traced(self.0, t)),
            Entry::Leaf(l) => Entry::Leaf(Traced(self.0, l)),
        }
    }
}

impl<I, TreeId, LeafId> From<Entry<Traced<I, TreeId>, Traced<I, LeafId>>>
    for Entry<TreeId, LeafId>
{
    fn from(entry: Entry<Traced<I, TreeId>, Traced<I, LeafId>>) -> Self {
        match entry {
            Entry::Tree(Traced(_, t)) => Entry::Tree(t),
            Entry::Leaf(Traced(_, l)) => Entry::Leaf(l),
        }
    }
}

impl<I: Send + Sync + Copy + 'static, M: Manifest> Manifest for Traced<I, M> {
    type TreeId = Traced<I, <M as Manifest>::TreeId>;
    type LeafId = Traced<I, <M as Manifest>::LeafId>;

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(
            self.1
                .list()
                .map(|(path, entry)| (path, self.inherit_into_entry(entry)))
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.1.lookup(name).map(|e| self.inherit_into_entry(e))
    }
}

#[async_trait]
impl<I: Clone + 'static + Send + Sync, M: Loadable + Send + Sync> Loadable for Traced<I, M> {
    type Value = Traced<I, <M as Loadable>::Value>;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let id = self.0.clone();
        let v = self.1.load(ctx, blobstore).await?;
        Ok(Traced(id, v))
    }
}
