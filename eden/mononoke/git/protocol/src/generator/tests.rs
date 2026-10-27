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
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::DummyCgdmChangesetDivider;
use git_types::GDMV2Entry;
use git_types::GDMV2ObjectEntry;
use git_types::GitDeltaManifestEntryOps;
use git_types::upload_non_blob_git_object;
use gix_hash::ObjectId;
use gix_object::Kind;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreBytes;
use mononoke_types::MPath;
use mononoke_types_mocks::changesetid::ONES_CSID;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use rustc_hash::FxHashSet;
use sha1::Digest;
use sha1::Sha1;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::GDMEntryProvider;
use super::boundary_trees_and_blobs;
use super::packfile_stream_from_changesets;
use crate::types::ChainBreakingMode;
use crate::types::DeltaInclusion;
use crate::types::FetchContainer;
use crate::types::FetchFilter;
use crate::types::PackfileConcurrency;
use crate::types::PackfileItemInclusion;
use crate::types::ShallowCommits;
use crate::types::ShallowInfoResponse;

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
