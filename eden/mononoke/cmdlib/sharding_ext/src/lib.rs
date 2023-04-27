/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;

const ENCODED_SLASH: &str = "_SLASH_";
const ENCODED_PLUS: &str = "_PLUS_";
const X_REPO_SEPARATOR: &str = "_TO_";
const CHUNK_SEPARATOR: &str = "_CHUNK_";
const CHUNK_SIZE_SEPARATOR: &str = "_SIZE_";
const CHUNK_PART_SEPARATOR: &str = "_OF_";

/// Struct representing the parsed structure of a Shard assigned by SM
#[derive(Clone, Default, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoShard {
    // The repo corresponding to this shard. If this is a x-repo shard,
    // then repo_name = source_repo_name
    pub repo_name: String,
    // The target repo name for this x-repo shard. It remains None for
    // single repo shards
    pub target_repo_name: Option<String>,
    // The total number of chunks as represented by the chunked shard.
    // i.e. The 16 in ShardId = fbsource_CHUNK_1_OF_16. Remains None in non-chunked
    // shards
    pub total_chunks: Option<usize>,
    // The id of the chunk corresponding to this specific shard.
    // i.e. The 1 in ShardId = fbsource_CHUNK_1_OF_16. Remains None in non-chunked
    // shards
    pub chunk_id: Option<usize>,
    // The size of each chunk as encoded in the chunked shard.
    // i.e. the 1000 in ShardId = fbsource_CHUNK_1_OF_16_SIZE_1000
    pub chunk_size: Option<usize>,
}

impl RepoShard {
    fn with_repo_name(repo_name: &str) -> Self {
        Self {
            repo_name: repo_name.to_string(),
            ..Default::default()
        }
    }

    fn with_source_and_target(repo_name: &str, target_repo_name: &str) -> Self {
        Self {
            repo_name: repo_name.to_string(),
            target_repo_name: Some(target_repo_name.to_string()),
            ..Default::default()
        }
    }

    fn with_chunks(repo_name: &str, target_repo_name: &str, chunks: &str) -> Result<Self> {
        let mut repo_shard = Self::with_source_and_target(repo_name, target_repo_name);
        let mut chunk_size_split = split_chunk_size(chunks).into_iter();
        if let Some(chunk_parts) = chunk_size_split.next() {
            let mut parts = split_chunk_parts(chunk_parts).into_iter();
            if let (Some(chunk_id), Some(total_chunks)) = (parts.next(), parts.next()) {
                let chunk_id = chunk_id.parse::<usize>().with_context(|| {
                    format_err!(
                        "Failure in creating RepoShard. Invalid chunk_id {}",
                        chunk_id
                    )
                })?;
                let total_chunks = total_chunks.parse::<usize>().with_context(|| {
                    format_err!(
                        "Failure in creating RepoShard. Invalid total_chunks {}",
                        total_chunks
                    )
                })?;
                repo_shard.chunk_id = Some(chunk_id);
                repo_shard.total_chunks = Some(total_chunks);
            } else {
                anyhow::bail!(
                    "Failure in creating RepoShard. Invalid chunk parts format {:?}",
                    parts
                )
            }
        } else {
            anyhow::bail!(
                "Failure in creating RepoShard. Invalid chunk format {:?}",
                chunks
            )
        }
        if let Some(chunk_size) = chunk_size_split.next() {
            let chunk_size = chunk_size.parse::<usize>().with_context(|| {
                format_err!(
                    "Failure in creating RepoShard. Invalid chunk_size {}",
                    chunk_size
                )
            })?;
            repo_shard.chunk_size = Some(chunk_size);
        }
        Ok(repo_shard)
    }

    /// Create the RepoShard based on the full string representation of the ShardID
    pub fn from_shard_id(shard_id: &str) -> Result<Self> {
        let decoded = decode_repo_name(shard_id);
        let mut split = split_repo_names(&decoded).into_iter();

        let repo_shard = match (split.next(), split.next()) {
            (Some(repo_name), None) => RepoShard::with_repo_name(repo_name),
            (Some(repo_name), Some(target_repo_name_and_chunks)) => {
                let mut split = split_chunk(target_repo_name_and_chunks).into_iter();
                match (split.next(), split.next()) {
                    (Some(target_repo_name), None) => {
                        RepoShard::with_source_and_target(repo_name, target_repo_name)
                    }
                    (Some(target_repo_name), Some(chunk_parts)) => {
                        RepoShard::with_chunks(repo_name, target_repo_name, chunk_parts)?
                    }
                    _ => anyhow::bail!(
                        "Failure in creating RepoShard. Invalid shard id {}",
                        shard_id
                    ),
                }
            }
            _ => anyhow::bail!(
                "Failure in creating RepoShard. Invalid shard id {}",
                shard_id
            ),
        };
        Ok(repo_shard)
    }
}

fn split_chunk(input: &str) -> Vec<&str> {
    input.split(CHUNK_SEPARATOR).collect()
}

fn split_chunk_size(chunks: &str) -> Vec<&str> {
    chunks.split(CHUNK_SIZE_SEPARATOR).collect()
}

fn split_chunk_parts(chunk_parts: &str) -> Vec<&str> {
    chunk_parts.split(CHUNK_PART_SEPARATOR).collect()
}

/// Function responsible for decoding an SM-encoded repo-name.
pub fn decode_repo_name(encoded_repo_name: &str) -> String {
    encoded_repo_name
        .replace(ENCODED_SLASH, "/")
        .replace(ENCODED_PLUS, "+")
}

/// Function responsible for SM-compatible encoding of repo-na
pub fn encode_repo_name(repo_name: &str) -> String {
    repo_name
        .replace('/', ENCODED_SLASH)
        .replace('+', ENCODED_PLUS)
}

/// Function responsible for splitting source and target repo name
/// from combined repo-name string.
pub fn split_repo_names(combined_repo_names: &str) -> Vec<&str> {
    combined_repo_names.split(X_REPO_SEPARATOR).collect()
}
