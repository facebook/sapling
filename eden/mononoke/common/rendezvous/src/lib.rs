/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod multi_rendez_vous;
mod rendez_vous;
mod rendez_vous_stats;
mod tunables;

#[cfg(test)]
mod test;

pub use crate::tunables::{TunablesMultiRendezVousController, TunablesRendezVousController};
pub use multi_rendez_vous::{MultiRendezVous, MultiRendezVousController};
pub use rendez_vous::{RendezVous, RendezVousController};
pub use rendez_vous_stats::RendezVousStats;

#[derive(Copy, Clone, Debug)]
pub struct RendezVousOptions {
    pub free_connections: usize,
}

impl RendezVousOptions {
    pub fn for_test() -> Self {
        Self {
            free_connections: 0,
        }
    }
}
