/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore::Storable;
use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::CGDMDividedChangesets;
use git_types::DeltaObjectKind;
use git_types::DummyCgdmChangesetDivider;
use git_types::GDMV2Entry;
use git_types::GDMV2ObjectEntry;
use git_types::GitDeltaManifestEntryOps;
use git_types::GitDeltaManifestV2;
use git_types::GitTreeId;
use git_types::PackfileItem;
use git_types::RootGitDeltaManifestV2Id;
use git_types::thrift;
use git_types::upload_non_blob_git_object;
use gix_hash::Kind as GitHashKind;
use gix_hash::ObjectId;
use gix_object::Kind;
use gix_pack::data::File as PackFile;
use gix_pack::data::entry::Header as PackEntryHeader;
use gix_pack::data::output::Entry as PackEntry;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreValue;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::ThriftConvert;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use packfile::pack::DeltaForm;
use packfile::pack::PackfileWriter;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use rustc_hash::FxHashSet;
use sha1::Digest;
use sha1::Sha1;
use tempfile::NamedTempFile;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::GDMEntryProvider;
use super::boundary_client_tree as select_boundary_client_tree;
use super::boundary_trees_and_blobs;
use super::packfile_stream_from_changesets;
use super::tree_and_blob_packfile_stream;
use super::trees_and_blobs_count;
use crate::pack_processor::parse_pack;
use crate::types::ChainBreakingMode;
use crate::types::DeltaInclusion;
use crate::types::FetchContainer;
use crate::types::FetchFilter;
use crate::types::FetchRequest;
use crate::types::PackfileConcurrency;
use crate::types::PackfileItemInclusion;
use crate::types::RefsSource;
use crate::types::ShallowCommits;
use crate::types::ShallowInfoResponse;
use crate::types::ShallowVariant;

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const CANCELLATION_TIMEOUT: Duration = Duration::from_secs(1);
const RETAINED_BYTES: usize = 1024 * 1024;

type Entries = Vec<Box<dyn GitDeltaManifestEntryOps + Send>>;

#[facet::container]
struct TestRepo(RepoBlobstore, RepoDerivedData);

async fn fetch_container(fb: FacebookInit) -> Result<FetchContainer> {
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    Ok(FetchContainer {
        ctx: Arc::new(CoreContext::test_mock(fb)),
        blobstore: repo.repo_blobstore_arc(),
        derived_data: repo.repo_derived_data_arc(),
        git_delta_manifest_version: GitDeltaManifestVersion::V2,
        delta_inclusion: DeltaInclusion::Exclude,
        filter: Arc::new(None),
        concurrency: PackfileConcurrency::new(1, 1, 1, 1, None),
        packfile_item_inclusion: PackfileItemInclusion::FetchOnly,
        shallow_info: Arc::new(None),
        boundary_client_tree: None,
        chain_breaking_mode: ChainBreakingMode::None,
        cgdm_changeset_divider: Arc::new(DummyCgdmChangesetDivider),
    })
}

struct SignalOnDrop(mpsc::UnboundedSender<()>);

impl Drop for SignalOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[derive(Debug)]
struct Gate {
    release: Semaphore,
    started: mpsc::UnboundedSender<Weak<Vec<u8>>>,
    dropped: mpsc::UnboundedSender<()>,
    panic_after_release: bool,
}

impl Gate {
    fn new() -> (
        Arc<Self>,
        mpsc::UnboundedReceiver<Weak<Vec<u8>>>,
        mpsc::UnboundedReceiver<()>,
    ) {
        let (started, started_rx) = mpsc::unbounded_channel();
        let (dropped, dropped_rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                release: Semaphore::new(0),
                started,
                dropped,
                panic_after_release: false,
            }),
            started_rx,
            dropped_rx,
        )
    }

    async fn hold(&self) -> Result<()> {
        let _drop_signal = SignalOnDrop(self.dropped.clone());
        let retained = Arc::new(vec![1; RETAINED_BYTES]);
        self.started.send(Arc::downgrade(&retained))?;
        let _permit = self.release.acquire().await?;
        drop(retained);
        assert!(!self.panic_after_release, "injected pack-worker read panic");
        Ok(())
    }
}

struct ReleaseOnDrop<'a>(&'a Semaphore);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.add_permits(1);
    }
}

async fn assert_released_on_cancel(
    gate: Arc<Gate>,
    mut started: mpsc::UnboundedReceiver<Weak<Vec<u8>>>,
    mut dropped: mpsc::UnboundedReceiver<()>,
    cancel: impl FnOnce(),
) -> Result<()> {
    let _cleanup = ReleaseOnDrop(&gate.release);
    let retained = timeout(EVENT_TIMEOUT, started.recv())
        .await?
        .expect("the real producer must enter its gate before cancellation");
    assert!(retained.upgrade().is_some());
    cancel();
    let dropped_on_cancel = timeout(CANCELLATION_TIMEOUT, dropped.recv())
        .await
        .is_ok_and(|event| event.is_some());
    let retained_on_cancel = retained.upgrade().is_some();
    gate.release.add_permits(1);
    if !dropped_on_cancel {
        timeout(EVENT_TIMEOUT, dropped.recv())
            .await?
            .expect("cleanup releases the detached old-code operation");
    }
    assert!(
        retained.upgrade().is_none(),
        "cleanup must release the 1 MiB sentinel"
    );
    eprintln!(
        "pack cancellation evidence: dropped_before_release={dropped_on_cancel} retained_before_release={retained_on_cancel} sentinel_bytes={RETAINED_BYTES}"
    );
    assert!(
        dropped_on_cancel && !retained_on_cancel,
        "dropping the returned stream must release the pending producer before its gate is released"
    );
    Ok(())
}

enum Provider {
    Gated(Arc<Gate>),
    Empty {
        started: Arc<AtomicBool>,
        on_drop: SignalOnDrop,
    },
    Entries(Entries),
}

#[async_trait]
impl GDMEntryProvider for Provider {
    async fn gdm_entries(
        self,
        _ctx: Arc<CoreContext>,
        _blobstore: ArcRepoBlobstore,
        _derived_data: ArcRepoDerivedData,
        _version: GitDeltaManifestVersion,
    ) -> Result<Entries> {
        match self {
            Self::Gated(gate) => {
                gate.hold().await?;
                anyhow::bail!("gated test provider error")
            }
            Self::Empty { started, on_drop } => {
                started.store(true, Ordering::SeqCst);
                drop(on_drop);
                Ok(Vec::new())
            }
            Self::Entries(entries) => Ok(entries),
        }
    }
}

#[mononoke::fbinit_test]
async fn dropping_unpolled_pack_stream_cancels_gdm_provider(fb: FacebookInit) -> Result<()> {
    let fetch = fetch_container(fb).await?;
    let (gate, started, dropped) = Gate::new();
    let stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![Provider::Gated(gate.clone())],
        None,
    );
    assert_released_on_cancel(gate, started, dropped, || drop(stream)).await
}

#[mononoke::fbinit_test]
async fn closed_pack_receiver_does_not_start_later_changeset(fb: FacebookInit) -> Result<()> {
    let fetch = fetch_container(fb).await?;
    let (gate, mut started, mut first_dropped) = Gate::new();
    let _cleanup = ReleaseOnDrop(&gate.release);
    let later_started = Arc::new(AtomicBool::new(false));
    let (later_dropped, mut later_dropped_rx) = mpsc::unbounded_channel();
    let stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![
            Provider::Gated(gate.clone()),
            Provider::Empty {
                started: later_started.clone(),
                on_drop: SignalOnDrop(later_dropped),
            },
        ],
        None,
    );
    timeout(EVENT_TIMEOUT, started.recv())
        .await?
        .expect("first provider entered its gate");
    drop(stream);
    // Keep the first operation blocked until cancellation can actually run.
    let _ = timeout(CANCELLATION_TIMEOUT, first_dropped.recv()).await;
    gate.release.add_permits(1);
    timeout(EVENT_TIMEOUT, later_dropped_rx.recv())
        .await?
        .expect("later provider was released, whether or not it was polled");
    let started_after_close = later_started.load(Ordering::SeqCst);
    eprintln!("closed receiver evidence: later_changeset_started={started_after_close}");
    assert!(
        !started_after_close,
        "receiver closure must not allow another changeset to start"
    );
    Ok(())
}

#[derive(Debug)]
struct ObservedBlobstore {
    inner: Arc<dyn Blobstore>,
    reads: Arc<Mutex<Vec<String>>>,
    pack_read_gate: Option<Arc<Gate>>,
}

impl fmt::Display for ObservedBlobstore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ObservedBlobstore")
    }
}

#[async_trait]
impl Blobstore for ObservedBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.reads
            .lock()
            .expect("read log lock")
            .push(key.to_owned());
        if let Some(gate) = &self.pack_read_gate
            && key.contains("git_packfile_base_item.")
        {
            gate.hold().await?;
        }
        self.inner.get(ctx, key).await
    }

    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.inner.put_explicit(ctx, key, value, behaviour).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.inner.put_with_status(ctx, key, value).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }
}

fn observe_reads(
    fetch: &mut FetchContainer,
    pack_read_gate: Option<Arc<Gate>>,
) -> Arc<Mutex<Vec<String>>> {
    let reads = Arc::new(Mutex::new(Vec::new()));
    let read_log = reads.clone();
    fetch.blobstore = Arc::new(RepoBlobstore::new_with_wrapped_inner_blobstore(
        (*fetch.blobstore).clone(),
        |inner| {
            Arc::new(ObservedBlobstore {
                inner,
                reads: read_log,
                pack_read_gate,
            })
        },
    ));
    reads
}

#[mononoke::fbinit_test]
async fn dropping_pack_stream_cancels_inflight_object_read(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let (gate, started, dropped) = Gate::new();
    observe_reads(&mut fetch, Some(gate.clone()));
    let entry = GDMV2Entry {
        full_object: GDMV2ObjectEntry {
            oid: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            size: 1,
            kind: git_types::DeltaObjectKind::Tree,
            inlined_bytes: None,
        },
        deltas: Vec::new(),
    };
    let stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![Provider::Entries(vec![Box::new((MPath::ROOT, entry))])],
        None,
    );
    assert_released_on_cancel(gate, started, dropped, || drop(stream)).await
}

async fn store_object(fetch: &FetchContainer, kind: &str, body: Vec<u8>) -> Result<ObjectId> {
    let mut bytes = format!("{kind} {}\0", body.len()).into_bytes();
    bytes.extend(body);
    let oid = ObjectId::from_bytes_or_panic(&Sha1::digest(&bytes));
    upload_non_blob_git_object(&fetch.ctx, &fetch.blobstore, &oid, bytes).await?;
    Ok(oid)
}

async fn boundary_with_entry(
    fetch: &mut FetchContainer,
    mode: &str,
    entry_oid: ObjectId,
) -> Result<ObjectId> {
    let mut tree_bytes = format!("{mode} child\0").into_bytes();
    tree_bytes.extend_from_slice(entry_oid.as_bytes());
    let root = store_object(fetch, "tree", tree_bytes).await?;
    let commit_body = format!(
        "tree {root}\nauthor Test <test@example.com> 0 +0000\ncommitter Test <test@example.com> 0 +0000\n\nboundary fixture\n"
    );
    let commit = store_object(fetch, "commit", commit_body.into_bytes()).await?;
    let commits = ShallowCommits {
        commits: Vec::new(),
        boundary_commits: vec![(ONES_CSID, commit).into()],
    };
    fetch.shallow_info = Arc::new(Some(ShallowInfoResponse::new(
        commits.clone(),
        commits,
        Vec::new(),
    )));
    Ok(root)
}

fn filter(max_blob_size: u64, max_tree_depth: u64) -> Arc<Option<FetchFilter>> {
    Arc::new(Some(FetchFilter {
        max_blob_size,
        max_tree_depth,
        allowed_object_types: vec![Kind::Tree, Kind::Blob, Kind::Commit, Kind::Tag],
    }))
}

#[mononoke::fbinit_test]
async fn provider_error_bypasses_stalled_object_read(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let oid = ObjectId::empty_tree(gix_hash::Kind::Sha1);
    git_types::upload_packfile_base_item(&fetch.ctx, &fetch.blobstore, &oid, b"tree 0\0".to_vec())
        .await?;
    let (object_gate, mut object_started, mut object_dropped) = Gate::new();
    let (error_gate, mut error_started, _error_dropped) = Gate::new();
    let _object_cleanup = ReleaseOnDrop(&object_gate.release);
    let _error_cleanup = ReleaseOnDrop(&error_gate.release);
    observe_reads(&mut fetch, Some(object_gate.clone()));
    let entry = GDMV2Entry {
        full_object: GDMV2ObjectEntry {
            oid,
            size: 1,
            kind: git_types::DeltaObjectKind::Tree,
            inlined_bytes: None,
        },
        deltas: Vec::new(),
    };
    let mut stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![
            Provider::Entries(vec![Box::new((MPath::ROOT, entry))]),
            Provider::Gated(error_gate.clone()),
        ],
        None,
    );
    let retained = timeout(EVENT_TIMEOUT, object_started.recv())
        .await?
        .expect("first object read entered its gate");
    timeout(EVENT_TIMEOUT, error_started.recv())
        .await?
        .expect("later provider entered its gate");
    assert!(retained.upgrade().is_some());
    error_gate.release.add_permits(1);

    let prompt_error = match timeout(CANCELLATION_TIMEOUT, stream.next()).await {
        Ok(Some(Err(error))) => error.to_string() == "gated test provider error",
        _ => false,
    };
    let object_still_pending = retained.upgrade().is_some();
    drop(stream);
    timeout(EVENT_TIMEOUT, object_dropped.recv())
        .await?
        .expect("dropping the stream must cancel its blocked object read");
    assert!(retained.upgrade().is_none());
    eprintln!(
        "provider error liveness: error_before_object_release={prompt_error} object_pending_at_observation={object_still_pending}"
    );
    assert!(
        prompt_error && object_still_pending,
        "a later provider error must bypass the ordered queue while the earlier object read remains gated"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn pack_stream_preserves_provider_errors(fb: FacebookInit) -> Result<()> {
    let fetch = fetch_container(fb).await?;
    let (gate, _started, _dropped) = Gate::new();
    gate.release.add_permits(1);
    let mut stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![Provider::Gated(gate)],
        None,
    );
    let item = timeout(EVENT_TIMEOUT, stream.next())
        .await?
        .expect("provider error must reach the caller");
    let error = match item {
        Err(error) => error,
        Ok(_) => anyhow::bail!("provider failure must not become an item"),
    };
    assert_eq!(error.to_string(), "gated test provider error");
    assert!(timeout(EVENT_TIMEOUT, stream.next()).await?.is_none());
    Ok(())
}

#[mononoke::fbinit_test]
async fn pack_worker_exit_cancels_provider_while_stream_is_retained(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let oid = ObjectId::empty_tree(gix_hash::Kind::Sha1);
    git_types::upload_packfile_base_item(&fetch.ctx, &fetch.blobstore, &oid, b"tree 0\0".to_vec())
        .await?;
    let (mut object_gate, mut object_started, _object_dropped) = Gate::new();
    Arc::get_mut(&mut object_gate)
        .expect("new gate is uniquely owned")
        .panic_after_release = true;
    let (provider_gate, mut provider_started, mut provider_dropped) = Gate::new();
    let _object_cleanup = ReleaseOnDrop(&object_gate.release);
    let _provider_cleanup = ReleaseOnDrop(&provider_gate.release);
    observe_reads(&mut fetch, Some(object_gate.clone()));
    let entry = GDMV2Entry {
        full_object: GDMV2ObjectEntry {
            oid,
            size: 1,
            kind: git_types::DeltaObjectKind::Tree,
            inlined_bytes: None,
        },
        deltas: Vec::new(),
    };
    let mut stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![
            Provider::Entries(vec![Box::new((MPath::ROOT, entry))]),
            Provider::Gated(provider_gate.clone()),
        ],
        None,
    );
    timeout(EVENT_TIMEOUT, object_started.recv())
        .await?
        .expect("pack worker entered the controlled read");
    let retained = timeout(EVENT_TIMEOUT, provider_started.recv())
        .await?
        .expect("later provider entered its gate");
    assert!(
        retained.upgrade().is_some(),
        "provider must remain alive before pack worker exit"
    );
    object_gate.release.add_permits(1);

    // Polling EOF would drop the outer guards and mask a missing worker link.
    let cancelled = timeout(CANCELLATION_TIMEOUT, provider_dropped.recv())
        .await
        .is_ok_and(|event| event.is_some());
    let still_retained = retained.upgrade().is_some();
    provider_gate.release.add_permits(1);
    if !cancelled {
        timeout(EVENT_TIMEOUT, provider_dropped.recv())
            .await?
            .expect("release the provider before failing");
    }
    assert!(
        cancelled && !still_retained,
        "pack worker exit must cancel its provider without dropping or polling the output stream"
    );
    assert!(timeout(EVENT_TIMEOUT, stream.next()).await?.is_none());
    Ok(())
}

#[mononoke::fbinit_test]
async fn pack_stream_keeps_workers_alive_until_consumed(fb: FacebookInit) -> Result<()> {
    let fetch = fetch_container(fb).await?;
    let oid = ObjectId::empty_tree(gix_hash::Kind::Sha1);
    git_types::upload_packfile_base_item(&fetch.ctx, &fetch.blobstore, &oid, b"tree 0\0".to_vec())
        .await?;
    let entry = GDMV2Entry {
        full_object: GDMV2ObjectEntry {
            oid,
            size: 0,
            kind: git_types::DeltaObjectKind::Tree,
            inlined_bytes: None,
        },
        deltas: Vec::new(),
    };
    let stream = packfile_stream_from_changesets(
        fetch,
        Arc::new(FxHashSet::default()),
        vec![Provider::Entries(vec![Box::new((MPath::ROOT, entry))])],
        None,
    );
    let items = timeout(EVENT_TIMEOUT, stream.try_collect::<Vec<_>>()).await??;
    assert_eq!(
        items.len(),
        1,
        "live consumers must still receive the pack item"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_blob_none_does_not_read_blob_metadata(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let absent_blob = ObjectId::from_hex(b"1111111111111111111111111111111111111111")?;
    let root = boundary_with_entry(&mut fetch, "100644", absent_blob).await?;
    fetch.filter = filter(0, u64::MAX);
    let reads = observe_reads(&mut fetch, None);
    let result = boundary_trees_and_blobs(fetch).await;
    let reads = reads.lock().expect("read log lock").clone();
    eprintln!("blob:none planning reads: {reads:?}; result={result:?}");
    let entries = result.expect("excluded blob does not require stored size metadata");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.into_iter().next().expect("root tree").oid, root);
    assert_eq!(
        reads.len(),
        2,
        "one commit read and one tree read, no metadata/size reread"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_blob_size_filter_preserves_strict_limit(fb: FacebookInit) -> Result<()> {
    for size in [0, 3, 4, 5] {
        let mut fetch = fetch_container(fb).await?;
        let content = Bytes::from(vec![b'x'; size]);
        let mut raw = format!("blob {size}\0").into_bytes();
        raw.extend_from_slice(&content);
        let oid = ObjectId::from_bytes_or_panic(&Sha1::digest(&raw));
        filestore::store(
            &fetch.blobstore,
            FilestoreConfig::no_chunking_filestore(),
            &fetch.ctx,
            &StoreRequest::new(size as u64),
            stream::iter([Ok(content)]),
        )
        .await?;
        let root = boundary_with_entry(&mut fetch, "100644", oid).await?;
        fetch.filter = filter(4, u64::MAX);
        let entries = boundary_trees_and_blobs(fetch).await?;
        assert!(entries.iter().any(|entry| entry.oid == root));
        let blob = entries.iter().find(|entry| entry.oid == oid);
        assert_eq!(
            blob.map(|entry| entry.size),
            (size < 4).then_some(size as u64),
            "only blob sizes strictly below the limit are included"
        );
        assert_eq!(entries.len(), 1 + usize::from(size < 4));
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_depth_filter_does_not_load_excluded_subtree(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let absent_subtree = ObjectId::from_hex(b"2222222222222222222222222222222222222222")?;
    let root = boundary_with_entry(&mut fetch, "40000", absent_subtree).await?;
    fetch.filter = filter(u64::MAX, 1);
    let reads = observe_reads(&mut fetch, None);
    let result = boundary_trees_and_blobs(fetch).await;
    let reads = reads.lock().expect("read log lock").clone();
    eprintln!("tree-depth planning reads: {reads:?}; result={result:?}");
    let entries = result.expect("subtree at excluded depth must not be loaded");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.into_iter().next().expect("root tree").oid, root);
    assert_eq!(
        reads.len(),
        2,
        "only boundary commit and root tree are read"
    );
    Ok(())
}

async fn store_boundary_blob(fetch: &FetchContainer, content: &'static [u8]) -> Result<ObjectId> {
    let content = Bytes::from_static(content);
    let mut raw = format!("blob {}\0", content.len()).into_bytes();
    raw.extend_from_slice(&content);
    let oid = ObjectId::from_bytes_or_panic(&Sha1::digest(&raw));
    filestore::store(
        &fetch.blobstore,
        FilestoreConfig::no_chunking_filestore(),
        &fetch.ctx,
        &StoreRequest::new(content.len() as u64),
        stream::iter([Ok(content)]),
    )
    .await?;
    Ok(oid)
}

async fn store_boundary_tree(
    fetch: &FetchContainer,
    entries: &[(&str, &str, ObjectId)],
) -> Result<ObjectId> {
    let mut bytes = Vec::new();
    for (mode, name, oid) in entries {
        bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
        bytes.extend_from_slice(oid.as_bytes());
    }
    store_object(fetch, "tree", bytes).await
}

async fn store_boundary_commit(
    fetch: &FetchContainer,
    tree: ObjectId,
    message: &str,
) -> Result<ObjectId> {
    store_boundary_commit_with_parents(fetch, tree, &[], message).await
}

async fn store_boundary_commit_with_parents(
    fetch: &FetchContainer,
    tree: ObjectId,
    parents: &[ObjectId],
    message: &str,
) -> Result<ObjectId> {
    let parents: String = parents
        .iter()
        .map(|parent| format!("parent {parent}\n"))
        .collect();
    store_object(
        fetch,
        "commit",
        format!(
            "tree {tree}\n{parents}author Test <test@example.com> 0 +0000\ncommitter Test <test@example.com> 0 +0000\n\n{message}\n"
        )
        .into_bytes(),
    )
    .await
}

fn set_boundary_commits(fetch: &mut FetchContainer, boundaries: &[ObjectId], client: &[ObjectId]) {
    let commits = ShallowCommits {
        commits: Vec::new(),
        boundary_commits: boundaries
            .iter()
            .map(|oid| (ONES_CSID, *oid).into())
            .collect(),
    };
    fetch.shallow_info = Arc::new(Some(ShallowInfoResponse::new(
        commits.clone(),
        commits,
        client.iter().map(|oid| (ONES_CSID, *oid).into()).collect(),
    )));
}

fn boundary_request(fetch: &FetchContainer, haves: &[ObjectId]) -> FetchRequest {
    FetchRequest {
        heads: haves.to_vec(),
        bases: haves.to_vec(),
        include_out_of_pack_deltas: true,
        include_annotated_tags: false,
        offset_delta: true,
        delta_form: DeltaForm::RefAndOffset,
        shallow_variant: ShallowVariant::FromClientWithDepth(10),
        filter: fetch.filter.as_ref().clone(),
        shallow_info: fetch.shallow_info.as_ref().clone(),
        concurrency: fetch.concurrency,
        chain_breaking_mode: fetch.chain_breaking_mode,
        refs_source: RefsSource::DatabaseMaster,
    }
}

async fn boundary_client_tree(
    fetch: &FetchContainer,
    haves: &[ObjectId],
) -> Result<Option<GitTreeId>> {
    select_boundary_client_tree(fetch, &boundary_request(fetch, haves)).await
}

async fn assert_boundary_count_and_stream(
    mut fetch: FetchContainer,
    expected: &[ObjectId],
) -> Result<()> {
    let entries = boundary_trees_and_blobs(fetch.clone()).await?;
    assert_eq!(
        entries
            .into_iter()
            .map(|entry| entry.oid)
            .collect::<FxHashSet<_>>(),
        expected.iter().copied().collect::<FxHashSet<_>>(),
    );
    let changesets = CGDMDividedChangesets {
        groups: Vec::new(),
        individual_cs_ids: Vec::new(),
    };
    let (count, bases) =
        trees_and_blobs_count(fetch.clone(), changesets.clone(), Vec::new()).await?;
    assert_eq!(
        count,
        expected.len(),
        "pack header count must use the same boundary selection"
    );
    fetch.packfile_item_inclusion = PackfileItemInclusion::Generate;
    let stream =
        tree_and_blob_packfile_stream(fetch.clone(), changesets, Arc::new(bases), Vec::new(), None)
            .await?;
    let mut writer = PackfileWriter::new(
        Vec::new(),
        count.try_into()?,
        1,
        DeltaForm::RefAndOffset,
        None,
    );
    timeout(EVENT_TIMEOUT, writer.write(stream)).await??;
    writer.finish().await?;
    assert_eq!(writer.num_entries as usize, count);
    let pack = writer.into_write();
    assert_eq!(u32::from_be_bytes(pack[8..12].try_into()?) as usize, count);
    let (decoded, _guard) = parse_pack(&pack, &fetch.ctx, fetch.blobstore, 1, None).await?;
    assert_eq!(
        decoded.keys().copied().collect::<FxHashSet<_>>(),
        expected.iter().copied().collect::<FxHashSet<_>>()
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_pruning_same_tree_does_not_load_tree(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let absent_tree = ObjectId::from_hex(b"3333333333333333333333333333333333333333")?;
    let client = store_boundary_commit(&fetch, absent_tree, "client").await?;
    let boundary = store_boundary_commit(&fetch, absent_tree, "boundary").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    let reads = observe_reads(&mut fetch, None);
    assert_boundary_count_and_stream(fetch, &[]).await?;
    assert!(
        reads
            .lock()
            .expect("read log lock")
            .iter()
            .all(|key| !key.contains(&absent_tree.to_string())),
        "equal root IDs must prune the tree without loading it",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_pruning_keeps_historical_objects_absent_from_client(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let stable = store_boundary_blob(&fetch, b"stable").await?;
    let historical = store_boundary_blob(&fetch, b"historical").await?;
    let client_tree = store_boundary_tree(&fetch, &[("100644", "stable", stable)]).await?;
    let boundary_tree = store_boundary_tree(
        &fetch,
        &[
            ("100644", "historical", historical),
            ("100644", "stable", stable),
        ],
    )
    .await?;
    let client = store_boundary_commit(&fetch, client_tree, "client").await?;
    let boundary = store_boundary_commit(&fetch, boundary_tree, "boundary").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert_boundary_count_and_stream(fetch, &[boundary_tree, historical]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_cold_and_filtered_fetches_keep_legacy_selection(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let blob = store_boundary_blob(&fetch, b"stable").await?;
    let tree = store_boundary_tree(&fetch, &[("100644", "stable", blob)]).await?;
    let client = store_boundary_commit(&fetch, tree, "client").await?;
    let boundary = store_boundary_commit(&fetch, tree, "boundary").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[]).await?;
    assert!(fetch.boundary_client_tree.is_none());
    assert_boundary_count_and_stream(fetch.clone(), &[tree, blob]).await?;
    fetch.filter = filter(0, u64::MAX);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert!(fetch.boundary_client_tree.is_none());
    assert_boundary_count_and_stream(fetch, &[tree]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_requires_one_matching_shallow_have(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let client = ObjectId::from_hex(b"3333333333333333333333333333333333333333")?;
    let other = ObjectId::from_hex(b"4444444444444444444444444444444444444444")?;
    set_boundary_commits(&mut fetch, &[other], &[client]);
    let reads = observe_reads(&mut fetch, None);
    for haves in [vec![], vec![other], vec![client, other]] {
        assert!(boundary_client_tree(&fetch, &haves).await?.is_none());
    }
    set_boundary_commits(&mut fetch, &[], &[client]);
    assert!(boundary_client_tree(&fetch, &[client]).await?.is_none());
    set_boundary_commits(&mut fetch, &[other], &[]);
    assert!(boundary_client_tree(&fetch, &[client]).await?.is_none());
    assert!(reads.lock().expect("read log lock").is_empty());
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_pruning_multiple_boundaries_preserves_both_histories(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let stable = store_boundary_blob(&fetch, b"stable").await?;
    let left = store_boundary_blob(&fetch, b"left history").await?;
    let right = store_boundary_blob(&fetch, b"right history").await?;
    let client_tree = store_boundary_tree(&fetch, &[("100644", "stable", stable)]).await?;
    let left_tree = store_boundary_tree(
        &fetch,
        &[("100644", "left", left), ("100644", "stable", stable)],
    )
    .await?;
    let right_tree = store_boundary_tree(
        &fetch,
        &[("100644", "right", right), ("100644", "stable", stable)],
    )
    .await?;
    let client = store_boundary_commit(&fetch, client_tree, "client").await?;
    let left_commit = store_boundary_commit(&fetch, left_tree, "left boundary").await?;
    let right_commit = store_boundary_commit(&fetch, right_tree, "right boundary").await?;
    set_boundary_commits(&mut fetch, &[left_commit, right_commit], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert!(fetch.boundary_client_tree.is_none());
    assert_boundary_count_and_stream(fetch, &[left_tree, left, right_tree, right, stable]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_does_not_walk_client_only_subtrees(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let stable = store_boundary_blob(&fetch, b"stable").await?;
    let absent_tree = ObjectId::from_hex(b"3333333333333333333333333333333333333333")?;
    let client_tree = store_boundary_tree(
        &fetch,
        &[
            ("40000", "old-dir", absent_tree),
            ("100644", "stable", stable),
        ],
    )
    .await?;
    let boundary_tree = store_boundary_tree(&fetch, &[("100644", "stable", stable)]).await?;
    let client = store_boundary_commit(&fetch, client_tree, "client").await?;
    let boundary = store_boundary_commit(&fetch, boundary_tree, "boundary").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert_boundary_count_and_stream(fetch, &[boundary_tree]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_mode_change_does_not_resend_identical_blob(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let blob = store_boundary_blob(&fetch, b"stable").await?;
    let client_tree = store_boundary_tree(&fetch, &[("100644", "stable", blob)]).await?;
    let boundary_tree = store_boundary_tree(&fetch, &[("100755", "stable", blob)]).await?;
    let client = store_boundary_commit(&fetch, client_tree, "client").await?;
    let boundary = store_boundary_commit(&fetch, boundary_tree, "boundary").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert_boundary_count_and_stream(fetch, &[boundary_tree]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_gitlink_does_not_prove_blob_possession(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let blob =
        store_boundary_blob(&fetch, b"historical blob, not a fetched gitlink target").await?;
    let client_tree = store_boundary_tree(&fetch, &[("160000", "p", blob)]).await?;
    let old_tree = store_boundary_tree(&fetch, &[("100644", "p", blob)]).await?;
    let boundary = store_boundary_commit(&fetch, old_tree, "historical blob").await?;
    let client =
        store_boundary_commit_with_parents(&fetch, client_tree, &[boundary], "gitlink tip").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    fetch.boundary_client_tree = boundary_client_tree(&fetch, &[client]).await?;
    assert_boundary_count_and_stream(fetch, &[old_tree, blob]).await
}

#[mononoke::fbinit_test]
async fn boundary_pruning_other_request_shapes_do_not_read_client_tree(
    fb: FacebookInit,
) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let client = ObjectId::from_hex(b"3333333333333333333333333333333333333333")?;
    let other = ObjectId::from_hex(b"4444444444444444444444444444444444444444")?;
    set_boundary_commits(&mut fetch, &[other], &[client]);
    let reads = observe_reads(&mut fetch, None);
    for variant in [
        ShallowVariant::None,
        ShallowVariant::FromServerWithDepth(10),
        ShallowVariant::FromServerWithTime(123),
        ShallowVariant::FromServerExcludingRefs(vec!["main".to_owned()]),
        ShallowVariant::FromClientWithDepth(0),
    ] {
        let mut request = boundary_request(&fetch, &[client]);
        request.shallow_variant = variant;
        assert!(
            select_boundary_client_tree(&fetch, &request)
                .await?
                .is_none()
        );
    }
    for heads in [vec![], vec![other], vec![client, other]] {
        let mut request = boundary_request(&fetch, &[client]);
        request.heads = heads;
        assert!(
            select_boundary_client_tree(&fetch, &request)
                .await?
                .is_none()
        );
    }
    for (thin_pack, delta_form) in [
        (false, DeltaForm::RefAndOffset),
        (false, DeltaForm::OnlyOffset),
        (true, DeltaForm::OnlyOffset),
    ] {
        let mut request = boundary_request(&fetch, &[client]);
        request.include_out_of_pack_deltas = thin_pack;
        request.delta_form = delta_form;
        assert!(
            select_boundary_client_tree(&fetch, &request)
                .await?
                .is_none()
        );
    }
    set_boundary_commits(&mut fetch, &[other], &[client, other]);
    assert!(boundary_client_tree(&fetch, &[client]).await?.is_none());
    set_boundary_commits(&mut fetch, &[other, client], &[client]);
    assert!(boundary_client_tree(&fetch, &[client]).await?.is_none());
    assert!(reads.lock().expect("read log lock").is_empty());
    Ok(())
}

async fn store_test_delta_manifest(
    fetch: &FetchContainer,
    csid: ChangesetId,
    entries: Vec<(MPath, GDMV2Entry)>,
) -> Result<()> {
    let manifest = GitDeltaManifestV2::from_entries(&fetch.ctx, &fetch.blobstore, entries).await?;
    let id = manifest
        .into_blob()
        .store(&fetch.ctx, &fetch.blobstore)
        .await?;
    let root = RootGitDeltaManifestV2Id::try_from(BlobstoreBytes::from_bytes(id.into_bytes()))?;
    root.store_mapping(
        &fetch.ctx,
        &fetch.derived_data.manager().derivation_context(None),
        csid,
    )
    .await
}

fn literal_blob_delta(
    base: ObjectId,
    target: ObjectId,
    base_bytes: &[u8],
    target_bytes: &[u8],
) -> Result<GDMV2Entry> {
    assert!(base_bytes.len() < 128 && !target_bytes.is_empty() && target_bytes.len() < 128);
    let mut instructions = vec![
        base_bytes.len() as u8,
        target_bytes.len() as u8,
        target_bytes.len() as u8,
    ];
    instructions.extend_from_slice(target_bytes);
    let mut loose = format!("blob {}\0", instructions.len()).into_bytes();
    loose.extend_from_slice(&instructions);
    let encoded: PackEntry = PackfileItem::new_base(loose.into())?.try_into()?;
    let object = |oid, size| GDMV2ObjectEntry {
        oid,
        size,
        kind: DeltaObjectKind::Blob,
        inlined_bytes: None,
    };
    GDMV2Entry::from_thrift(thrift::GDMV2Entry {
        full_object: object(target, target_bytes.len() as u64).into_thrift(),
        deltas: vec![thrift::GDMV2DeltaEntry {
            parent: ONES_CSID.into_thrift(),
            base_object: object(base, base_bytes.len() as u64).into_thrift(),
            base_object_path: MPath::new("p")?.into_thrift(),
            instructions: thrift::GDMV2Instructions {
                uncompressed_size: instructions.len() as i64,
                compressed_size: encoded.compressed_data.len() as i64,
                instruction_bytes: thrift::GDMV2InstructionBytes::inlined(
                    encoded.compressed_data.into(),
                ),
                ..Default::default()
            },
            ..Default::default()
        }],
        ..Default::default()
    })
}

#[mononoke::fbinit_test]
async fn boundary_pruning_respects_writer_and_thin_pack_with_real_body_delta(
    fb: FacebookInit,
) -> Result<()> {
    const BASE: &[u8] = &[b'a'; 64];
    const TARGET: &[u8] = &[b'b'; 64];
    let mut fetch = fetch_container(fb).await?;
    fetch.delta_inclusion = DeltaInclusion::standard();
    fetch.packfile_item_inclusion = PackfileItemInclusion::Generate;
    let base = store_boundary_blob(&fetch, BASE).await?;
    let target = store_boundary_blob(&fetch, TARGET).await?;
    let boundary_tree = store_boundary_tree(&fetch, &[("100644", "p", base)]).await?;
    let intermediate_tree = store_boundary_tree(&fetch, &[("100644", "p", target)]).await?;
    // A(B) -> I(B') -> H(B): H is already shallow at the client, A is the new boundary.
    let boundary = store_boundary_commit(&fetch, boundary_tree, "A").await?;
    let intermediate =
        store_boundary_commit_with_parents(&fetch, intermediate_tree, &[boundary], "I").await?;
    let client =
        store_boundary_commit_with_parents(&fetch, boundary_tree, &[intermediate], "H").await?;
    set_boundary_commits(&mut fetch, &[boundary], &[client]);
    Arc::make_mut(&mut fetch.shallow_info)
        .as_mut()
        .expect("shallow response")
        .packfile_commits
        .commits = vec![(TWOS_CSID, intermediate).into()];
    let tree_entry = GDMV2Entry {
        full_object: GDMV2ObjectEntry {
            oid: intermediate_tree,
            size: ("100644 p\0".len() + target.as_bytes().len()) as u64,
            kind: DeltaObjectKind::Tree,
            inlined_bytes: None,
        },
        deltas: vec![],
    };
    store_test_delta_manifest(
        &fetch,
        TWOS_CSID,
        vec![
            (MPath::ROOT, tree_entry),
            (
                MPath::new("p")?,
                literal_blob_delta(base, target, BASE, TARGET)?,
            ),
        ],
    )
    .await?;
    let changesets = CGDMDividedChangesets {
        groups: vec![],
        individual_cs_ids: vec![TWOS_CSID],
    };
    for thin_pack in [false, true] {
        for delta_form in [DeltaForm::RefAndOffset, DeltaForm::OnlyOffset] {
            let mut request = boundary_request(&fetch, &[client]);
            request.include_out_of_pack_deltas = thin_pack;
            request.delta_form = delta_form;
            fetch.boundary_client_tree = select_boundary_client_tree(&fetch, &request).await?;
            let pruned = thin_pack && delta_form == DeltaForm::RefAndOffset;
            assert_eq!(fetch.boundary_client_tree.is_some(), pruned);
            let (count, bases) =
                trees_and_blobs_count(fetch.clone(), changesets.clone(), vec![]).await?;
            assert_eq!(count, if pruned { 2 } else { 4 });
            assert_eq!(bases.contains(&base), pruned);
            let stream = tree_and_blob_packfile_stream(
                fetch.clone(),
                changesets.clone(),
                Arc::new(bases),
                vec![],
                None,
            )
            .await?;
            let mut writer =
                PackfileWriter::new(Vec::new(), count.try_into()?, 1, delta_form, None);
            timeout(EVENT_TIMEOUT, writer.write(stream)).await??;
            writer.finish().await?;
            assert_eq!(writer.num_entries as usize, count);
            let pack = writer.into_write();
            assert_eq!(u32::from_be_bytes(pack[8..12].try_into()?) as usize, count);
            let temp = NamedTempFile::new()?;
            tokio::fs::write(temp.path(), &pack).await?;
            let file = PackFile::at(temp.path(), GitHashKind::Sha1)?;
            let entries = file.streaming_iter()?.collect::<Result<Vec<_>, _>>()?;
            assert_eq!(entries.len(), count);
            let (ref_deltas, offset_deltas) =
                entries
                    .iter()
                    .fold((0, 0), |(refs, offsets), entry| match entry.header {
                        PackEntryHeader::RefDelta { base_id } => {
                            assert_eq!(base_id, base);
                            (refs + 1, offsets)
                        }
                        PackEntryHeader::OfsDelta { .. } => (refs, offsets + 1),
                        _ => (refs, offsets),
                    });
            assert_eq!(
                (ref_deltas, offset_deltas),
                if delta_form == DeltaForm::OnlyOffset {
                    (0, 1)
                } else {
                    (1, 0)
                }
            );
            // The thin pack decodes with only the known client blob. Fallback packs
            // must decode against an entirely empty store, not the server's store.
            let receiver = fetch_container(fb).await?;
            if pruned {
                store_boundary_blob(&receiver, BASE).await?;
            }
            let (decoded, _guard) =
                parse_pack(&pack, &receiver.ctx, receiver.blobstore, 1, None).await?;
            assert_eq!(decoded.len(), if pruned { 3 } else { 4 });
            assert_eq!(
                decoded[&target].raw().as_ref(),
                [b"blob 64\0".as_slice(), TARGET].concat()
            );
            assert!(decoded.contains_key(&intermediate_tree));
            assert_eq!(decoded.contains_key(&boundary_tree), !pruned);
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn boundary_pruning_propagates_missing_client_commit(fb: FacebookInit) -> Result<()> {
    let mut fetch = fetch_container(fb).await?;
    let missing = ObjectId::from_hex(b"3333333333333333333333333333333333333333")?;
    set_boundary_commits(&mut fetch, &[missing], &[missing]);
    let error = boundary_client_tree(&fetch, &[missing])
        .await
        .expect_err("missing client commit must fail");
    assert!(
        error
            .to_string()
            .contains("Error fetching client shallow commit")
    );
    Ok(())
}
