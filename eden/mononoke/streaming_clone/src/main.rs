/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blake2::Blake2b;
use blake2::Digest;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use borrowed::borrowed;
use clap::App;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_revlog::revlog::Entry;
use mercurial_revlog::revlog::RevIdx;
use mercurial_revlog::revlog::Revlog;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::o;
use slog::Logger;
use std::borrow::Borrow;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use streaming_clone::StreamingClone;
use streaming_clone::StreamingCloneRef;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

pub const CREATE_SUB_CMD: &str = "create";
pub const DEFAULT_MAX_DATA_CHUNK_SIZE: u32 = 950 * 1024;
pub const DOT_HG_PATH_ARG: &str = "dot-hg-path";
pub const MAX_DATA_CHUNK_SIZE: &str = "max-data-chunk-size";
pub const SKIP_LAST_CHUNK_ARG: &str = "skip-last-chunk";
pub const STREAMING_CLONE: &str = "streaming-clone";
pub const TAG_ARG: &str = "tag";
pub const NO_UPLOAD_IF_LESS_THAN_CHUNKS_ARG: &str = "no-upload-if-less-than-chunks";
pub const UPDATE_SUB_CMD: &str = "update";

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    streaming_clone: StreamingClone,
}

pub async fn streaming_clone<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<(), Error> {
    let mut scuba = matches.scuba_sample_builder();
    let repo: Repo = args::open_repo(fb, &logger, matches).await?;
    scuba.add("reponame", repo.repo_identity().name());

    let res = match matches.subcommand() {
        (CREATE_SUB_CMD, Some(sub_m)) => {
            let tag: Option<&str> = sub_m.value_of(TAG_ARG);
            scuba.add_opt("tag", tag);
            let ctx = build_context(fb, &logger, &repo, &tag);
            // This command works only if there are no streaming chunks at all for a give repo.
            // So exit quickly if database is not empty
            let count = repo.streaming_clone().count_chunks(&ctx, tag).await?;
            if count > 0 {
                return Err(anyhow!(
                    "cannot create new streaming clone chunks because they already exists"
                ));
            }

            update_streaming_changelog(&ctx, &repo, sub_m, tag).await
        }
        (UPDATE_SUB_CMD, Some(sub_m)) => {
            let tag: Option<&str> = sub_m.value_of(TAG_ARG);
            scuba.add_opt("tag", tag);
            let ctx = build_context(fb, &logger, &repo, &tag);
            update_streaming_changelog(&ctx, &repo, sub_m, tag).await
        }
        _ => Err(anyhow!("unknown subcommand")),
    };

    match res {
        Ok(chunks_num) => {
            scuba.add("success", 1);
            scuba.add("chunks_inserted", format!("{}", chunks_num));
        }
        Err(ref err) => {
            scuba.add("success", 0);
            scuba.add("error", format!("{:#}", err));
        }
    };

    scuba.log();
    res?;
    Ok(())
}

fn build_context(
    fb: FacebookInit,
    logger: &Logger,
    repo: &Repo,
    tag: &Option<&str>,
) -> CoreContext {
    let logger = if let Some(tag) = tag {
        logger.new(o!("repo" => repo.repo_identity().name().to_string(), "tag" => tag.to_string()))
    } else {
        logger.new(o!("repo" => repo.repo_identity().name().to_string()))
    };

    CoreContext::new_with_logger(fb, logger)
}

// Returns how many chunks were inserted
async fn update_streaming_changelog(
    ctx: &CoreContext,
    repo: &Repo,
    sub_m: &ArgMatches<'_>,
    tag: Option<&str>,
) -> Result<usize, Error> {
    let max_data_chunk_size: u32 =
        args::get_and_parse(sub_m, MAX_DATA_CHUNK_SIZE, DEFAULT_MAX_DATA_CHUNK_SIZE);
    let (idx, data) = get_revlog_paths(sub_m)?;

    let revlog = Revlog::from_idx_with_data(idx.clone(), None as Option<String>)?;
    let rev_idx_to_skip =
        find_latest_rev_id_in_streaming_changelog(ctx, repo, &revlog, tag).await?;

    let skip_last_chunk = sub_m.is_present(SKIP_LAST_CHUNK_ARG);
    let chunks = split_into_chunks(
        &revlog,
        Some(rev_idx_to_skip),
        max_data_chunk_size,
        skip_last_chunk,
    )?;

    let no_upload_if_less_than_chunks: Option<usize> =
        args::get_and_parse_opt(sub_m, NO_UPLOAD_IF_LESS_THAN_CHUNKS_ARG);

    if let Some(at_least_chunks) = no_upload_if_less_than_chunks {
        if chunks.len() < at_least_chunks {
            info!(
                ctx.logger(),
                "has too few chunks to upload - {}. Exiting",
                chunks.len()
            );
            return Ok(0);
        }
    }

    info!(ctx.logger(), "about to upload {} entries", chunks.len());
    let chunks = upload_chunks_blobstore(ctx, repo, &chunks, &idx, &data).await?;

    info!(ctx.logger(), "inserting into streaming clone database");
    let start = repo.streaming_clone().select_max_chunk_num(ctx).await?;
    info!(ctx.logger(), "current max chunk num is {:?}", start);
    let start = start.map_or(0, |start| start + 1);
    let chunks: Vec<_> = chunks
        .into_iter()
        .enumerate()
        .map(|(chunk_id, (chunk, keys))| {
            let chunk_id: u32 = chunk_id.try_into().unwrap();
            (chunk_id, (chunk, keys))
        })
        .map(|(chunk_id, (chunk, keys))| (start + chunk_id, chunk, keys))
        .collect();
    let chunks_num = chunks.len();
    insert_entries_into_db(ctx, repo, chunks, tag).await?;

    Ok(chunks_num)
}

fn get_revlog_paths(sub_m: &ArgMatches<'_>) -> Result<(PathBuf, PathBuf), Error> {
    let p = sub_m
        .value_of(DOT_HG_PATH_ARG)
        .ok_or_else(|| anyhow!("{} is not set", DOT_HG_PATH_ARG))?;
    let mut idx = PathBuf::from(p);
    idx.push("store");
    idx.push("00changelog.i");
    let data = idx.with_extension("d");

    Ok((idx, data))
}

async fn find_latest_rev_id_in_streaming_changelog(
    ctx: &CoreContext,
    repo: &Repo,
    revlog: &Revlog,
    tag: Option<&str>,
) -> Result<usize, Error> {
    let index_entry_size = revlog.index_entry_size();
    let (cur_idx_size, cur_data_size) = repo
        .streaming_clone()
        .select_index_and_data_sizes(ctx, tag)
        .await?
        .unwrap_or((0, 0));
    info!(
        ctx.logger(),
        "current sizes in database: index: {}, data: {}", cur_idx_size, cur_data_size
    );
    let cur_idx_size: usize = cur_idx_size.try_into().unwrap();
    let rev_idx_to_skip = cur_idx_size / index_entry_size;

    Ok(rev_idx_to_skip)
}

fn split_into_chunks(
    revlog: &Revlog,
    skip: Option<usize>,
    max_data_chunk_size: u32,
    skip_last_chunk: bool,
) -> Result<Vec<Chunk>, Error> {
    let index_entry_size: u32 = revlog.index_entry_size().try_into().unwrap();

    let mut chunks = vec![];
    let mut iter: Box<dyn Iterator<Item = (RevIdx, Entry)>> = Box::new(revlog.into_iter());
    if let Some(skip) = skip {
        iter = Box::new(iter.skip(skip));
    }

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

    if skip_last_chunk {
        chunks.pop();
    }

    Ok(chunks)
}

async fn upload_chunks_blobstore<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    chunks: &'a [Chunk],
    idx: &'a Path,
    data: &'a Path,
) -> Result<Vec<(&'a Chunk, BlobstoreKeys)>, Error> {
    let chunks = stream::iter(chunks.iter().enumerate().map(|(chunk_id, chunk)| {
        borrowed!(ctx, repo, idx, data);
        async move {
            let keys =
                upload_chunk(ctx, repo, chunk, chunk_id.try_into().unwrap(), idx, data).await?;
            Result::<_, Error>::Ok((chunk, keys))
        }
    }))
    .buffered(10)
    .inspect({
        let mut i = 0;
        move |_| {
            i += 1;
            if i % 100 == 0 {
                info!(ctx.logger(), "uploaded {}", i);
            }
        }
    })
    .try_collect::<Vec<_>>()
    .await?;

    Ok(chunks)
}

async fn insert_entries_into_db(
    ctx: &CoreContext,
    repo: &Repo,
    entries: Vec<(u32, &'_ Chunk, BlobstoreKeys)>,
    tag: Option<&str>,
) -> Result<(), Error> {
    for insert_chunk in entries.chunks(10) {
        let mut rows = vec![];
        for (chunk_id, chunk, keys) in insert_chunk {
            rows.push((
                *chunk_id,
                keys.idx.as_str(),
                chunk.idx_len,
                keys.data.as_str(),
                chunk.data_len,
            ))
        }

        repo.streaming_clone().insert_chunks(ctx, tag, rows).await?;
    }

    Ok(())
}

struct BlobstoreKeys {
    idx: String,
    data: String,
}

async fn upload_chunk(
    ctx: &CoreContext,
    repo: &Repo,
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
    repo: &Repo,
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

    repo.repo_blobstore()
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

fn add_common_args<'a, 'b>(sub_cmd: App<'a, 'b>) -> App<'a, 'b> {
    sub_cmd
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
        )
        .arg(
            Arg::with_name(TAG_ARG)
                .long(TAG_ARG)
                .takes_value(true)
                .required(false)
                .help("which tag to use when preparing the changelog"),
        )
        .arg(
            Arg::with_name(SKIP_LAST_CHUNK_ARG)
                .long(SKIP_LAST_CHUNK_ARG)
                .takes_value(false)
                .required(false)
                .help("skip uploading last chunk. "),
        )
        .arg(
            Arg::with_name(NO_UPLOAD_IF_LESS_THAN_CHUNKS_ARG)
                .long(NO_UPLOAD_IF_LESS_THAN_CHUNKS_ARG)
                .takes_value(true)
                .required(false)
                .conflicts_with(SKIP_LAST_CHUNK_ARG)
                .help("Do not do anything if we have less than that number of chunks to upload"),
        )
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new("Tool to manage streaming clone chunks")
        .with_advanced_args_hidden()
        .with_scuba_logging_args()
        .build()
        .subcommand(add_common_args(
            SubCommand::with_name(CREATE_SUB_CMD).about("create new streaming clone"),
        ))
        .subcommand(add_common_args(
            SubCommand::with_name(UPDATE_SUB_CMD).about("update existing streaming changelog"),
        ))
        .get_matches(fb)?;

    let logger = matches.logger();
    let runtime = matches.runtime();
    runtime.block_on(streaming_clone(fb, logger.clone(), &matches))
}
