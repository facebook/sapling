/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{FileContentData, Node, NodeData, NodeType, WrappedPath};
use crate::progress::{
    progress_stream, report_state, ProgressReporter, ProgressStateCountByType, ProgressStateMutex,
};
use crate::sampling::{PathTrackingRoute, SampleTrigger, SamplingWalkVisitor, WalkSampleMapping};
use crate::scrub::ScrubStats;
use crate::setup::{
    parse_node_types, setup_common, CORPUS, DEFAULT_INCLUDE_NODE_TYPES,
    EXCLUDE_SAMPLE_NODE_TYPE_ARG, INCLUDE_SAMPLE_NODE_TYPE_ARG, OUTPUT_DIR_ARG,
    PROGRESS_INTERVAL_ARG, PROGRESS_SAMPLE_DURATION_S, PROGRESS_SAMPLE_RATE,
    PROGRESS_SAMPLE_RATE_ARG, SAMPLE_OFFSET_ARG, SAMPLE_RATE_ARG,
};
use crate::tail::{walk_exact_tail, RepoWalkRun};

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::{CoreContext, SamplingKey};
use fbinit::FacebookInit;
use filetime::{self, FileTime};
use futures::{
    future::{self, FutureExt, TryFutureExt},
    stream::{Stream, TryStreamExt},
};
use mononoke_types::{datetime::DateTime, BlobstoreBytes};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use samplingblob::SamplingHandler;
use slog::Logger;
use std::{collections::HashMap, io::Write, path::PathBuf, sync::Arc, time::Duration};
use tokio::fs::{self as tkfs};

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
/// https://url.spec.whatwg.org/#path-percent-encode-set plus comma
const PATH: &AsciiSet = &FRAGMENT.add(b'#').add(b'?').add(b'{').add(b'}').add(b',');

// A path not found in repo paths.
// The constant includes a comma so that it is not a valid percent encoding
// so that we can distiguish in-repo paths fron the hierarchy used for the blobs.
const DUMP_DIR: &str = ".mononoke,";

// A subdir used for temp files before they are moved to final location
const INFLIGHT_DIR: &str = "Inflight";

// Force load of leaf data like file contents that graph traversal did not need
// Output is the samples
fn corpus_stream<InStream, SS>(
    scheduled_max: usize,
    output_dir: Option<String>,
    s: InStream,
    sampler: Arc<CorpusSamplingHandler<CorpusSample>>,
) -> impl Stream<Item = Result<(Node, Option<()>, Option<ScrubStats>), Error>>
where
    InStream: Stream<
            Item = Result<
                (
                    (Node, Option<WrappedPath>),
                    (Option<DateTime>, Option<NodeData>),
                    Option<SS>,
                ),
                Error,
            >,
        >
        + 'static
        + Send,
{
    s.map_ok(move |(walk_key, (mtime, nd), _progress_stats)| match nd {
        Some(NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream))) => {
            cloned!(sampler);
            file_bytes_stream
                // Force file chunks to be loaded
                .try_fold(0, |acc, file_bytes| future::ok(acc + file_bytes.size()))
                // We take the size from the sample rather than the stream as it
                // includes thrift wrapper overhead so more closely matches store
                .map_ok(move |_num_bytes| {
                    let sample = sampler.complete_step(&walk_key);
                    let size = ScrubStats::from(sample.as_ref());
                    (walk_key, sample, mtime, Some(size))
                })
                .left_future()
        }
        _ => {
            let sample = sampler.complete_step(&walk_key);
            let size = ScrubStats::from(sample.as_ref());
            future::ready(Ok((walk_key, sample, mtime, Some(size)))).right_future()
        }
    })
    .try_buffer_unordered(scheduled_max)
    // Dump the data to disk
    .map_ok(move |((n, path), sample, mtime, stats)| match sample {
        Some(sample) => move_node_files(output_dir.clone(), n.clone(), path, mtime, sample).map_ok(move |()| (n, Some(()), stats)).left_future(),
        None => future::ok((n, Some(()), stats)).right_future(),
    })
    .try_buffer_unordered(scheduled_max)
}

// Disk directory layout is of the form NodeType/root/<repo_path>/.mononoke,/aa/bb/cc/blob_key
// where the repo_path and blob_key are both percent_encoded and
// aa/bb/cc etc is a subset of the hash used to prevent any one directory becoming too large.
// For types without any in-repo path (e.g. `BonsaiChangeset`) the repo_path component is omitted.
fn disk_node_dir(
    base_for_type: &PathBuf,
    path: Option<&WrappedPath>,
    hash_subset: &[u8],
    dump_extension: bool,
) -> PathBuf {
    let mut o = base_for_type.clone();
    match path {
        Some(WrappedPath::NonRoot(path)) => {
            let path = PathBuf::from(percent_encode(&path.as_ref().to_vec(), PATH).to_string());
            if dump_extension {
                match path.extension() {
                    Some(ext) => {
                        o.push("byext");
                        o.push(ext);
                    }
                    None => {
                        o.push("noext");
                    }
                }
            }
            o.push("root");
            o.push(path);
        }
        // This is content directly for the root, e.g. a root manifest
        Some(WrappedPath::Root) => o.push("root"),
        // Not path associated in any way, e.g. a BonsaiChangeset
        None => (),
    };

    // Separate the dumped data from the repo dir structure
    o.push(DUMP_DIR);

    // 16777216 directories per path should be enough
    for d in 0..3 {
        o.push(hex::encode(&hash_subset[d..(d + 1)]));
    }
    o
}

fn dump_with_extension(node_type: NodeType) -> bool {
    match node_type {
        NodeType::Root => false,
        // Bonsai
        NodeType::Bookmark => false,
        NodeType::BonsaiChangeset => false,
        NodeType::BonsaiHgMapping => false,
        NodeType::BonsaiPhaseMapping => false,
        NodeType::PublishedBookmarks => false,
        NodeType::BonsaiFsnodeMapping => false,
        // Hg
        NodeType::HgBonsaiMapping => false,
        NodeType::HgChangeset => false,
        NodeType::HgManifest => false,
        NodeType::HgFileEnvelope => true,
        NodeType::HgFileNode => true,
        // Content
        NodeType::FileContent => true,
        NodeType::FileContentMetadata => true,
        NodeType::AliasContentMapping => true,
        // Derived Data
        NodeType::Fsnode => false,
    }
}

async fn move_node_files(
    output_dir: Option<String>,
    node: Node,
    repo_path: Option<WrappedPath>,
    mtime: Option<DateTime>,
    sample: CorpusSample,
) -> Result<(), Error> {
    let hash_subset = node
        .sampling_fingerprint()
        .unwrap_or_default()
        .to_le_bytes();

    let output_dir = match output_dir {
        Some(output_dir) => output_dir,
        None => return Ok(()),
    };

    let mut base = PathBuf::from(output_dir);
    base.push(node.get_type().to_string());

    let inflight_dir = match sample.inflight_dir {
        Some(inflight_dir) => inflight_dir,
        None => return Ok(()),
    };

    let dump_extension = dump_with_extension(node.get_type());

    for (k, _) in sample.data {
        let mut dest_path = disk_node_dir(&base, repo_path.as_ref(), &hash_subset, dump_extension);
        tkfs::create_dir_all(&dest_path).await?;

        let key_file = percent_encode(k.as_bytes(), PATH).to_string();
        dest_path.push(&key_file);
        let mut source_path = inflight_dir.clone();
        source_path.push(&key_file);

        if let Some(mtime) = mtime {
            filetime::set_file_mtime(
                source_path.clone(),
                FileTime::from_unix_time(mtime.timestamp_secs(), 0),
            )?;
        }

        tkfs::rename(source_path, dest_path).await?;
    }
    tkfs::remove_dir(inflight_dir).await?;
    Ok(())
}

#[derive(Debug)]
pub struct CorpusSamplingHandler<T> {
    inner: WalkSampleMapping<(Node, Option<WrappedPath>), T>,
    output_dir: Option<String>,
}

impl<T> SampleTrigger<(Node, Option<WrappedPath>)> for CorpusSamplingHandler<T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: (Node, Option<WrappedPath>)) {
        self.inner.map_keys(sample_key, walk_key);
    }
}

// This exists so we can track output_dir
impl<T> CorpusSamplingHandler<T> {
    pub fn new(output_dir: Option<String>) -> Self {
        Self {
            inner: WalkSampleMapping::new(),
            output_dir,
        }
    }

    pub fn complete_step(&self, walk_key: &(Node, Option<WrappedPath>)) -> Option<T> {
        self.inner.complete_step(walk_key)
    }
}

#[derive(Debug)]
pub struct CorpusSample {
    pub inflight_dir: Option<PathBuf>,
    pub data: HashMap<String, u64>,
}

impl Default for CorpusSample {
    fn default() -> Self {
        Self {
            inflight_dir: None,
            data: HashMap::with_capacity(1),
        }
    }
}

impl From<Option<&CorpusSample>> for ScrubStats {
    fn from(sample: Option<&CorpusSample>) -> Self {
        sample
            .map(|sample| ScrubStats {
                blobstore_keys: sample.data.values().len() as u64,
                blobstore_bytes: sample.data.values().by_ref().sum(),
            })
            .unwrap_or_default()
    }
}

impl SamplingHandler for CorpusSamplingHandler<CorpusSample> {
    fn sample_get(
        &self,
        ctx: CoreContext,
        key: String,
        value: Option<&BlobstoreBytes>,
    ) -> Result<(), Error> {
        let output_dir = match &self.output_dir {
            Some(d) => d,
            None => return Ok(()),
        };

        let sampling_key = match ctx.sampling_key() {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut inflight_path = PathBuf::from(output_dir);
        inflight_path.push(INFLIGHT_DIR);
        inflight_path.push(sampling_key.inner().to_string());
        if let Some(mut guard) = self.inner.inflight().get_mut(sampling_key) {
            std::fs::create_dir_all(&inflight_path)?;
            guard.inflight_dir = Some(inflight_path.clone());
            inflight_path.push(percent_encode(key.as_bytes(), PATH).to_string());
            let mut f = std::fs::File::create(inflight_path)?;
            if let Some(value) = value {
                f.write_all(value.as_bytes())?;
            }
            guard.data.insert(key, value.map_or(0, |v| v.len()) as u64);
        }

        Ok(())
    }
}

// Subcommand entry point for dumping a corpus of blobs to disk
pub async fn corpus<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), Error> {
    let output_dir = sub_m.value_of(OUTPUT_DIR_ARG).map(|s| s.to_string());
    let corpus_sampler = Arc::new(CorpusSamplingHandler::<CorpusSample>::new(
        output_dir.clone(),
    ));

    let (datasources, walk_params) = setup_common(
        CORPUS,
        fb,
        &logger,
        Some(corpus_sampler.clone()),
        matches,
        sub_m,
    )
    .await?;

    let repo_name = args::get_repo_name(fb, &matches)?;
    let sample_rate = args::get_u64_opt(&sub_m, SAMPLE_RATE_ARG).unwrap_or(100);
    let sample_offset = args::get_u64_opt(&sub_m, SAMPLE_OFFSET_ARG).unwrap_or(0);
    let progress_interval_secs = args::get_u64_opt(&sub_m, PROGRESS_INTERVAL_ARG);
    let progress_sample_rate = args::get_u64_opt(&sub_m, PROGRESS_SAMPLE_RATE_ARG);

    if let Some(output_dir) = &output_dir {
        if !std::path::Path::new(output_dir).exists() {
            std::fs::create_dir(output_dir).map_err(Error::from)?
        }
    }

    cloned!(
        walk_params.include_node_types,
        walk_params.include_edge_types
    );
    let mut sampling_node_types = parse_node_types(
        sub_m,
        INCLUDE_SAMPLE_NODE_TYPE_ARG,
        EXCLUDE_SAMPLE_NODE_TYPE_ARG,
        DEFAULT_INCLUDE_NODE_TYPES,
    )?;
    sampling_node_types.retain(|i| include_node_types.contains(i));

    let sizing_progress_state =
        ProgressStateMutex::new(ProgressStateCountByType::<ScrubStats, ScrubStats>::new(
            fb,
            logger.clone(),
            CORPUS,
            repo_name,
            sampling_node_types.clone(),
            progress_sample_rate.unwrap_or(PROGRESS_SAMPLE_RATE),
            Duration::from_secs(progress_interval_secs.unwrap_or(PROGRESS_SAMPLE_DURATION_S)),
        ));

    let make_sink = {
        cloned!(
            walk_params.progress_state,
            walk_params.quiet,
            walk_params.scheduled_max,
            corpus_sampler
        );
        move |run: RepoWalkRun| {
            cloned!(run.ctx);
            async move |walk_output| {
                cloned!(ctx, sizing_progress_state);
                let walk_progress = progress_stream(quiet, &progress_state.clone(), walk_output);

                let corpus =
                    corpus_stream(scheduled_max, output_dir, walk_progress, corpus_sampler);
                let report_sizing = progress_stream(quiet, &sizing_progress_state.clone(), corpus);
                report_state(ctx, sizing_progress_state, report_sizing)
                    .map({
                        cloned!(progress_state);
                        move |d| {
                            progress_state.report_progress();
                            d
                        }
                    })
                    .await
            }
        }
    };

    let walk_state = Arc::new(SamplingWalkVisitor::new(
        include_node_types,
        include_edge_types,
        sampling_node_types,
        corpus_sampler,
        sample_rate,
        sample_offset,
    ));
    walk_exact_tail::<_, _, _, _, _, PathTrackingRoute>(
        fb,
        logger,
        datasources,
        walk_params,
        walk_state,
        make_sink,
        true,
    )
    .await
}
