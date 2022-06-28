/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dashmap::DashMap;
use std::hash::Hash;
use std::sync::Arc;

use crate::RendezVous;
use crate::RendezVousController;
use crate::RendezVousStats;
use crate::TunablesMultiRendezVousController;

pub trait MultiRendezVousController: Send + Sync + 'static {
    type Controller: RendezVousController;

    fn new_controller(&self) -> Self::Controller;
}

/// A wrapper around RendezVous that can be keyed by a grouping key (G). This is useful when you
/// want multiple RendezVous instances for a set of groups but you don't know the groups ahead of
/// time (e.g. the groups might be repository ids).
pub struct MultiRendezVous<
    G,
    K,
    V,
    C: MultiRendezVousController = TunablesMultiRendezVousController,
> {
    inner: Arc<DashMap<G, RendezVous<K, V, <C as MultiRendezVousController>::Controller>>>,
    multi_controller: C,
    stats: Arc<RendezVousStats>,
}

impl<G, K, V, C> Clone for MultiRendezVous<G, K, V, C>
where
    G: Hash + Eq,
    C: MultiRendezVousController + Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            multi_controller: self.multi_controller.clone(),
            stats: self.stats.clone(),
        }
    }
}

impl<G, K, V, C> MultiRendezVous<G, K, V, C>
where
    G: Hash + Eq,
    C: MultiRendezVousController,
{
    pub fn new(multi_controller: C, stats: RendezVousStats) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            multi_controller,
            stats: Arc::new(stats),
        }
    }

    pub fn get(&self, group: G) -> RendezVous<K, V, <C as MultiRendezVousController>::Controller> {
        use dashmap::mapref::entry::Entry;

        let ret = match self.inner.entry(group) {
            Entry::Occupied(e) => e.get().clone(),
            Entry::Vacant(e) => e
                .insert(RendezVous::new(
                    self.multi_controller.new_controller(),
                    self.stats.clone(),
                ))
                .clone(),
        };

        ret
    }
}
