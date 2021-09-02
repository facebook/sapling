/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BonsaiDerivable;
use anyhow::Error;
use context::CoreContext;
use futures_stats::FutureStats;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::prelude::*;
use time_ext::DurationExt;

define_stats! {
    prefix = "mononoke.derived_data";
    derived_data_latency:
        dynamic_timeseries("{}.deriving.latency_ms", (derived_data_type: &'static str); Average),
}

pub fn init_derived_data_scuba<Derivable: BonsaiDerivable>(
    ctx: &CoreContext,
    reponame: &str,
    derived_data_scuba_table: &Option<String>,
    bcs_id: &ChangesetId,
) -> MononokeScubaSampleBuilder {
    match &derived_data_scuba_table {
        Some(scuba_table) => {
            let mut builder = MononokeScubaSampleBuilder::new(ctx.fb, scuba_table);
            builder.add_common_server_data();
            builder.add("derived_data", Derivable::NAME);
            builder.add("reponame", reponame);
            builder.add("changeset", format!("{}", bcs_id));
            builder
        }
        None => MononokeScubaSampleBuilder::with_discard(),
    }
}

pub fn log_derivation_start<Derivable>(
    ctx: &CoreContext,
    derived_data_scuba: &mut MononokeScubaSampleBuilder,
    bcs_id: &ChangesetId,
) where
    Derivable: BonsaiDerivable,
{
    let tag = "Generating derived data";
    ctx.scuba()
        .clone()
        .log_with_msg(tag, Some(format!("{} {}", Derivable::NAME, bcs_id)));
    // derived data name and bcs_id already logged as separate fields
    derived_data_scuba.log_with_msg(tag, None);
}

pub fn log_derivation_end<Derivable>(
    ctx: &CoreContext,
    derived_data_scuba: &mut MononokeScubaSampleBuilder,
    bcs_id: &ChangesetId,
    stats: &FutureStats,
    res: &Result<(), Error>,
) where
    Derivable: BonsaiDerivable,
{
    let tag = if res.is_ok() {
        "Generated derived data"
    } else {
        "Failed to generate derived data"
    };

    let msg = Some(format!("{} {}", Derivable::NAME, bcs_id));
    let mut scuba_sample = ctx.scuba().clone();
    scuba_sample.add_future_stats(&stats);
    if let Err(err) = res {
        scuba_sample.add("Derive error", format!("{:#}", err));
    };
    scuba_sample.log_with_msg(tag, msg.clone());

    ctx.perf_counters().insert_perf_counters(derived_data_scuba);

    let msg = match res {
        Ok(_) => None,
        Err(err) => Some(format!("{:#}", err)),
    };

    derived_data_scuba
        .add_future_stats(&stats)
        // derived data name and bcs_id already logged as separate fields
        .log_with_msg(tag, msg);

    STATS::derived_data_latency.add_value(
        stats.completion_time.as_millis_unchecked() as i64,
        (Derivable::NAME,),
    );
}

pub fn log_mapping_insertion<Derivable>(
    ctx: &CoreContext,
    derived_data_scuba: &mut MononokeScubaSampleBuilder,
    cs_id: ChangesetId,
    stats: &FutureStats,
    res: &Result<(), Error>,
    value: &Derivable,
) where
    Derivable: BonsaiDerivable,
{
    let tag = if res.is_ok() {
        "Inserted mapping"
    } else {
        "Failed to insert mapping"
    };

    ctx.perf_counters().insert_perf_counters(derived_data_scuba);

    let msg = match res {
        Ok(_) => None,
        Err(err) => Some(format!("{:#}", err)),
    };


    let mut value = format!("{:?}", value);
    // limit how much data we log to scuba
    let limit = 1000;
    if value.len() > limit {
        value = value.chars().take(limit).collect();
    }

    derived_data_scuba
        .add_future_stats(&stats)
        .add("changeset", format!("{}", cs_id))
        .add("mapping_value", value)
        // derived data name and bcs_id already logged as separate fields
        .log_with_msg(tag, msg);
}
