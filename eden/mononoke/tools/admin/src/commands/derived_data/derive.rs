/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::builder::PossibleValuesParser;
use clap::Args;
use context::CoreContext;
use context::SessionClass;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::Rederivation;
use futures_stats::TimedTryFutureExt;
use mononoke_api::ChangesetId;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::DerivableType;
use repo_derived_data::RepoDerivedDataRef;
use slog::trace;
use strum::IntoEnumIterator;

use super::Repo;

#[derive(Args)]
pub(super) struct DeriveArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    /// Type of derived data
    #[clap(long, short = 'T', required = true,  value_parser = PossibleValuesParser::new(DerivableType::iter().map(|t| DerivableType::name(&t))), group="types to derive")]
    derived_data_types: Vec<String>,
    /// Whether all enabled derived data types should be derived
    #[clap(long, required = true, group = "types to derive")]
    all_types: bool,
    /// Whether the changesets need to be rederived or not
    #[clap(long)]
    pub(crate) rederive: bool,
    /// Whether to derive from the predecessor of this derived data type
    #[clap(long)]
    from_predecessor: bool,
    /// Batch size to use for derivation
    #[clap(long)]
    batch_size: Option<u64>,
}

pub(super) async fn derive(ctx: &mut CoreContext, repo: &Repo, args: DeriveArgs) -> Result<()> {
    let resolved_csids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let csids = resolved_csids.as_slice();

    let manager = repo.repo_derived_data().manager();

    let derived_data_types = if args.all_types {
        // Derive all the types enabled in the config
        let derived_data_config = repo.repo_derived_data().active_config();
        derived_data_config.types.clone()
    } else {
        // Only derive the types specified by the user
        args.derived_data_types
            .into_iter()
            .map(|ty| DerivableType::from_name(&ty))
            .collect::<Result<HashSet<_>>>()?
    };

    let rederivation = if args.rederive {
        trace!(ctx.logger(), "about to rederive {} commits", csids.len());
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
        Arc::new(Mutex::new(csids.iter().copied().collect::<HashSet<_>>()))
    } else {
        trace!(ctx.logger(), "about to derive {} commits", csids.len());
        Default::default()
    };

    if args.from_predecessor {
        for derived_data_type in derived_data_types {
            for csid in csids {
                derive_from_predecessor(
                    ctx,
                    manager,
                    derived_data_type,
                    *csid,
                    rederivation.clone(),
                )
                .await?;
            }
        }
    } else {
        let (stats, ()) = manager
            .derive_bulk(
                ctx,
                csids,
                Some(rederivation),
                &derived_data_types.into_iter().collect::<Vec<_>>(),
                args.batch_size,
            )
            .try_timed()
            .await?;
        trace!(
            ctx.logger(),
            "finished derivation in {}ms",
            stats.completion_time.as_millis(),
        );
    }

    Ok(())
}

async fn derive_from_predecessor(
    ctx: &CoreContext,
    manager: &DerivedDataManager,
    derived_data_type: DerivableType,
    csid: ChangesetId,
    rederivation: Arc<dyn Rederivation>,
) -> Result<()> {
    trace!(ctx.logger(), "deriving {} from predecessors", csid);
    let (stats, res) = BulkDerivation::derive_from_predecessor(
        manager,
        ctx,
        csid,
        Some(rederivation),
        derived_data_type,
    )
    .try_timed()
    .await?;
    trace!(
        ctx.logger(),
        "derived {} for {} in {}ms, {:?}",
        derived_data_type.name(),
        csid,
        stats.completion_time.as_millis(),
        res,
    );
    Ok(())
}
