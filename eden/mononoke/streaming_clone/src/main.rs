/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Context, Error};
use blake2::{Blake2b, Digest};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes};
use borrowed::borrowed;
use clap::{Arg, SubCommand};
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{future, stream, StreamExt, TryStreamExt};
use mercurial_revlog::revlog::{Entry, RevIdx, Revlog};
use slog::{info, Logger};
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use std::borrow::Borrow;
use std::convert::TryInto;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use streaming_clone::SqlStreamingChunksFetcher;
use tokio::io::AsyncReadExt;

pub const CREATE_SUB_CMD: &str = "create";
pub const DEFAULT_MAX_DATA_CHUNK_SIZE: u32 = 950 * 1024;
pub const DOT_HG_PATH_ARG: &str = "dot-hg-path";
pub const MAX_DATA_CHUNK_SIZE: &str = "max-data-chunk-size";
pub const STREAMING_CLONE: &str = "streaming-clone";

pub async fn streaming_clone<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<(), Error> {
    args::init_cachelib(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, &logger, &matches).await?;

    let streaming_chunks_fetcher = create_streaming_chunks_fetcher(fb, &logger, matches).await?;
    match matches.subcommand() {
        (CREATE_SUB_CMD, Some(sub_m)) => {
            let max_data_chunk_size: u32 =
                args::get_and_parse(sub_m, MAX_DATA_CHUNK_SIZE, DEFAULT_MAX_DATA_CHUNK_SIZE);
            // This command works only if there are no streaming chunks at all for a give repo.
            // So exit quickly if database is not empty
            let count = streaming_chunks_fetcher
                .count_chunks(&ctx, repo.get_repoid())
                .await?;
            if count > 0 {
                return Err(anyhow!(
                    "cannot create new streaming clone chunks because they already exists"
                ));
            }

            let p = sub_m
                .value_of(DOT_HG_PATH_ARG)
                .ok_or_else(|| anyhow!("{} is not set", DOT_HG_PATH_ARG))?;
            let mut idx = PathBuf::from(p);
            idx.push("store");
            idx.push("00changelog.i");
            let data = idx.with_extension("d");

            // Iterate through all revlog entries and split them in chunks.
            // Data for each chunk won't be larger than max_data_chunk_size
            let revlog = Revlog::from_idx_with_data(idx.clone(), None as Option<String>)?;
            let chunks = split_into_chunks(&revlog, max_data_chunk_size)?;

            info!(ctx.logger(), "about to upload {} entries", chunks.len());
            let chunks = upload_chunks_blobstore(&ctx, &repo, &chunks, &idx, &data).await?;

            info!(ctx.logger(), "inserting into streaming clone database");
            insert_entries_into_db(&ctx, &repo, &streaming_chunks_fetcher, chunks).await?;

            Ok(())
        }
        _ => Err(anyhow!("unknown subcommand")),
    }
}

fn split_into_chunks(revlog: &Revlog, max_data_chunk_size: u32) -> Result<Vec<Chunk>, Error> {
    let index_entry_size: u32 = revlog.index_entry_size().try_into().unwrap();

    let mut chunks = vec![];
    let mut iter = (&revlog).into_iter();

    let mut current_chunk = match iter.next() {
        Some((idx, entry)) => {
            let idx_start = u64::from(idx.as_u32() * index_entry_size);
            let data_start = entry.offset;
            let mut chunk = Chunk::new(idx_start, data_start);
            chunk.add_entry(idx, index_entry_size, &entry)?;
            chunk
        }
        None => {
            return Ok(vec![]);
        }
    };

    for (idx, entry) in iter {
        if !can_add_entry(&current_chunk, &entry, max_data_chunk_size) {
            let next_chunk = current_chunk.next_chunk();
            chunks.push(current_chunk);
            current_chunk = next_chunk;
        }

        current_chunk.add_entry(idx, index_entry_size, &entry)?;
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    Ok(chunks)
}

async fn upload_chunks_blobstore<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    chunks: &'a [Chunk],
    idx: &'a Path,
    data: &'a Path,
) -> Result<Vec<(usize, &'a Chunk, BlobstoreKeys)>, Error> {
    let chunks = stream::iter(chunks.iter().enumerate().map(|(chunk_id, chunk)| {
        borrowed!(ctx, repo, idx, data);
        async move {
            let keys = upload_chunk(
                &ctx,
                &repo,
                chunk,
                chunk_id.try_into().unwrap(),
                &idx,
                &data,
            )
            .await?;
            Result::<_, Error>::Ok((chunk_id, chunk, keys))
        }
    }))
    .buffered(10)
    .try_collect::<Vec<_>>()
    .await?;

    Ok(chunks)
}

async fn insert_entries_into_db(
    ctx: &CoreContext,
    repo: &BlobRepo,
    streaming_chunks_fetcher: &SqlStreamingChunksFetcher,
    entries: Vec<(usize, &'_ Chunk, BlobstoreKeys)>,
) -> Result<(), Error> {
    for insert_chunk in entries.chunks(10) {
        let mut rows = vec![];
        for (chunk_id, chunk, keys) in insert_chunk {
            rows.push((
                (*chunk_id).try_into().unwrap(),
                keys.idx.as_str(),
                chunk.idx_len,
                keys.data.as_str(),
                chunk.data_len,
            ))
        }

        streaming_chunks_fetcher
            .insert_chunks(&ctx, repo.get_repoid(), rows)
            .await?;
    }

    Ok(())
}

async fn create_streaming_chunks_fetcher<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<SqlStreamingChunksFetcher, Error> {
    let config_store = args::init_config_store(fb, logger, matches)?;
    let (_, config) = args::get_config(config_store, &matches)?;
    let storage_config = config.storage_config;
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    SqlStreamingChunksFetcher::with_metadata_database_config(
        fb,
        &storage_config.metadata,
        &mysql_options,
        readonly_storage.0,
    )
    .await
    .context("Failed to open SqlStreamingChunksFetcher")
}

struct BlobstoreKeys {
    idx: String,
    data: String,
}

async fn upload_chunk(
    ctx: &CoreContext,
    repo: &BlobRepo,
    chunk: &Chunk,
    chunk_id: u32,
    idx_path: &Path,
    data_path: &Path,
) -> Result<BlobstoreKeys, Error> {
    let f1 = upload_data(
        ctx,
        repo,
        chunk_id,
        idx_path,
        chunk.idx_start,
        chunk.idx_len,
        "idx",
    );

    let f2 = upload_data(
        ctx,
        repo,
        chunk_id,
        data_path,
        chunk.data_start,
        chunk.data_len,
        "data",
    );

    let (idx, data) = future::try_join(f1, f2).await?;
    Ok(BlobstoreKeys { idx, data })
}

async fn upload_data(
    ctx: &CoreContext,
    repo: &BlobRepo,
    chunk_id: u32,
    path: impl Borrow<Path>,
    start: u64,
    len: u32,
    suffix: &str,
) -> Result<String, Error> {
    let path: &Path = path.borrow();

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(SeekFrom::Start(start)).await?;

    let mut data = vec![];
    file.take(len as u64).read_to_end(&mut data).await?;

    let key = generate_key(chunk_id, &data, suffix);

    repo.blobstore()
        .put(ctx, key.clone(), BlobstoreBytes::from_bytes(data))
        .await?;

    Ok(key)
}

fn generate_key(chunk_id: u32, data: &[u8], suffix: &str) -> String {
    let hash = Blake2b::digest(data);

    format!("streaming_clone-chunk{:06}-{:x}-{}", chunk_id, hash, suffix,)
}

fn can_add_entry(chunk: &Chunk, entry: &Entry, max_data_size: u32) -> bool {
    chunk.data_len.saturating_add(entry.compressed_len) <= max_data_size
}

struct Chunk {
    idx_start: u64,
    idx_len: u32,
    data_start: u64,
    data_len: u32,
}

impl Chunk {
    fn new(idx_start: u64, data_start: u64) -> Self {
        Self {
            idx_start,
            idx_len: 0,
            data_start,
            data_len: 0,
        }
    }

    fn next_chunk(&self) -> Chunk {
        Self {
            idx_start: self.idx_start + u64::from(self.idx_len),
            idx_len: 0,
            data_start: self.data_start + u64::from(self.data_len),
            data_len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.idx_len == 0
    }

    fn add_entry(
        &mut self,
        idx: RevIdx,
        index_entry_size: u32,
        entry: &Entry,
    ) -> Result<(), Error> {
        self.idx_len += index_entry_size;

        let expected_offset = self.data_start + u64::from(self.data_len);
        if expected_offset != entry.offset {
            return Err(anyhow!(
                "failed to add entry {}: expected offset {}, actual offset {}",
                idx.as_u32(),
                expected_offset,
                entry.offset
            ));
        }
        self.data_len += entry.compressed_len;

        Ok(())
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new("Tool to manage streaming clone chunks")
        .with_advanced_args_hidden()
        .build()
        .subcommand(
            SubCommand::with_name(CREATE_SUB_CMD)
                .about("create new streaming clone")
                .arg(
                    Arg::with_name(DOT_HG_PATH_ARG)
                        .long(DOT_HG_PATH_ARG)
                        .takes_value(true)
                        .required(true)
                        .help("path to .hg folder with changelog"),
                )
                .arg(
                    Arg::with_name(MAX_DATA_CHUNK_SIZE)
                        .long(MAX_DATA_CHUNK_SIZE)
                        .takes_value(true)
                        .required(false)
                        .help("max size of the data entry that we'll write to the blobstore"),
                ),
        )
        .get_matches();

    args::init_cachelib(fb, &matches);
    let logger = args::init_logging(fb, &matches)?;

    let mut runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(streaming_clone(fb, logger, &matches))
}
