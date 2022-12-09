/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

mod multi_rendez_vous;
mod rendez_vous;
mod rendez_vous_stats;
mod tunables;

#[cfg(test)]
mod test;

pub use multi_rendez_vous::MultiRendezVous;
pub use multi_rendez_vous::MultiRendezVousController;
pub use rendez_vous::RendezVous;
pub use rendez_vous::RendezVousController;
pub use rendez_vous_stats::RendezVousStats;

pub use crate::tunables::ConfigurableRendezVousController;
pub use crate::tunables::TunablesMultiRendezVousController;
pub use crate::tunables::TunablesRendezVousController;

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

/// Command line arguments for controlling rendez-vous
#[derive(Args, Debug)]
pub struct RendezVousArgs {
    /// How many concurrent connections to allow before batching kicks in
    #[clap(long, default_value = "5")]
    pub rendezvous_free_connections: usize,
}

impl From<RendezVousArgs> for RendezVousOptions {
    fn from(args: RendezVousArgs) -> Self {
        RendezVousOptions {
            free_connections: args.rendezvous_free_connections,
        }
    }
}

#[cfg(test)]
mod demo {
    use std::collections::HashMap;
    use std::sync::Arc;

    use anyhow::Error;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use maplit::hashset;

    use super::*;

    // NOTE: I'd make this a doctest, but we don't have support for running those at the moment
    #[fbinit::test]
    async fn demo(fb: FacebookInit) -> Result<(), Error> {
        // RendezVousOptions would typically be instantiated from CLI args via cmdlib.
        let opts = RendezVousOptions::for_test();

        let stats = Arc::new(RendezVousStats::new("stats_prefix".into()));

        // Callers sharing a RendezVous instance will be eligible to have their calls batched
        // together.
        let rdv = RendezVous::new(TunablesRendezVousController::new(opts), stats);

        let out = rdv
            .dispatch(fb, hashset! { 1u64 }, || {
                |keys| async move {
                    // Keys will include your own query (`1` in this example), and potentially
                    // other queries batched with yours via the RendezVous instance. Return a
                    // HashMap mapping keys to values as your output. You only need to return a
                    // value for keys that were found.
                    Ok(keys
                        .into_iter()
                        .map(|k| (k, k + 1))
                        .collect::<HashMap<_, _>>())
                }
            })
            .await?;

        // The output from dispatch will include only the keys your requested. If a key is missing,
        // you'll get `None` back as the value.
        assert_eq!(out, hashmap! { 1 => Some(2) });

        Ok(())
    }
}
