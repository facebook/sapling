/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use mononoke_types::ChangesetId;
use sorted_vector_map::SortedVectorMap;
use std::{collections::HashMap, str::FromStr};

pub const EXTRA_PREFIX: &str = "source-cs-id-";

pub fn encode_latest_synced_state_extras(
    latest_synced_state: &HashMap<String, ChangesetId>,
) -> SortedVectorMap<String, Vec<u8>> {
    latest_synced_state
        .iter()
        .map(|(name, cs_id)| {
            (
                format!("{}{}", EXTRA_PREFIX, name),
                Vec::from(cs_id.to_hex().as_bytes()),
            )
        })
        .collect()
}

pub fn decode_latest_synced_state_extras<'a>(
    extra: impl Iterator<Item = (&'a str, &'a [u8])>,
) -> Result<HashMap<String, ChangesetId>, Error> {
    extra
        .into_iter()
        .filter_map(|(name, value)| {
            name.strip_prefix(EXTRA_PREFIX)
                .map(|repo_name| (repo_name.to_string(), value))
        })
        .map(|(repo_name, value)| {
            let cs_id = ChangesetId::from_str(&String::from_utf8(value.to_vec())?)?;
            anyhow::Ok((repo_name.to_string(), cs_id))
        })
        .collect::<Result<HashMap<_, _>, _>>()
        .context("failed to parsed latest synced state extras")
}
