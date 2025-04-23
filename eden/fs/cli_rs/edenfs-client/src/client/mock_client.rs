/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::result::Result;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use fb303_core_clients::BaseService;
use fb303_core_clients::errors::*;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use mockall::mock;
pub use thrift_thriftclients::EdenService;
use thrift_thriftclients::thrift::errors::*;
use thrift_types::edenfs::*;
use thrift_types::edenfs_config::EdenConfigData;
use thrift_types::fb303_core::fb303_status;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::Connector;
use crate::client::EdenFsClientStatsHandler;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::EdenFsConnector;
use crate::client::connector::EdenFsThriftClient;
use crate::client::connector::StreamingEdenFsConnector;

pub struct MockThriftClient {
    thrift_service: Option<EdenFsThriftClient>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
}

impl MockThriftClient {
    pub fn set_thrift_service(&mut self, client: EdenFsThriftClient) {
        self.thrift_service = Some(client);
    }
}

#[async_trait]
impl Client for MockThriftClient {
    fn new(_fb: FacebookInit, _socket_file: PathBuf, _semaphore: Option<Semaphore>) -> Self {
        Self {
            thrift_service: None,
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
        }
    }

    fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        _conn_timeout: Option<Duration>,
        _recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let service = self.thrift_service.clone().unwrap();
        f(&service).await.map_err(|e| e.into())
    }

    async fn with_streaming_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        _conn_timeout: Option<Duration>,
        _recv_timeout: Option<Duration>,
        _f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        unimplemented!()
    }
}

pub fn make_boxed_future_result<T, E>(result: Result<T, E>) -> BoxFuture<'static, Result<T, E>>
where
    T: Send + 'static,
    E: Send + 'static,
{
    tokio::task::spawn(async move { result })
        .map(|r| match r {
            Ok(r) => r,
            Err(_) => panic!("Error joing tokio task."),
        })
        .boxed()
}

mock! {
    pub EdenFsService {}

    #[allow(non_snake_case)]
    impl BaseService for EdenFsService {
        fn getStatus(&self) -> BoxFuture<'static, Result<fb303_status, GetStatusError>>;

        fn getName(&self) -> BoxFuture<'static, Result<String, GetNameError>>;

        fn getVersion(&self) -> BoxFuture<'static, Result<String, GetVersionError>>;

        fn getStatusDetails(&self) -> BoxFuture<'static, Result<String, GetStatusDetailsError>>;

        fn getCounters(&self) -> BoxFuture<'static, Result<BTreeMap<String, i64>, GetCountersError>>;

        fn getRegexCounters(
            &self,
            regex: &str,
        ) -> BoxFuture<'static, Result<BTreeMap<String, i64>, GetRegexCountersError>>;

        fn getSelectedCounters(
            &self,
            keys: &[String],
        ) -> BoxFuture<'static, Result<BTreeMap<String, i64>, GetSelectedCountersError>>;

        fn getCounter(&self, key: &str) -> BoxFuture<'static, Result<i64, GetCounterError>>;

        fn getExportedValues(
            &self,
        ) -> BoxFuture<'static, Result<BTreeMap<String, String>, GetExportedValuesError>>;

        fn getSelectedExportedValues(
            &self,
            keys: &[String],
        ) -> BoxFuture<'static, Result<BTreeMap<String, String>, GetSelectedExportedValuesError>>;

        fn getRegexExportedValues(
            &self,
            regex: &str,
        ) -> BoxFuture<'static, Result<BTreeMap<String, String>, GetRegexExportedValuesError>>;

        fn getExportedValue(
            &self,
            key: &str,
        ) -> BoxFuture<'static, Result<String, GetExportedValueError>>;

        fn setOption(&self, key: &str, value: &str) -> BoxFuture<'static, Result<(), SetOptionError>>;

        fn getOption(&self, key: &str) -> BoxFuture<'static, Result<String, GetOptionError>>;

        fn getOptions(&self) -> BoxFuture<'static, Result<BTreeMap<String, String>, GetOptionsError>>;

        fn aliveSince(&self) -> BoxFuture<'static, Result<i64, AliveSinceError>>;
    }

    #[allow(non_snake_case)]
    impl EdenService for EdenFsService {
        fn listMounts(&self) -> BoxFuture<'static, Result<Vec<MountInfo>, ListMountsError>>;

        fn mount(&self, info: &MountArgument) -> BoxFuture<'static, Result<(), MountError>>;

        fn unmount(&self, mount_point: &PathString) -> BoxFuture<'static, Result<(), UnmountError>>;

        fn unmountV2(
            &self,
            unmount_argument: &UnmountArgument,
        ) -> BoxFuture<'static, Result<(), UnmountV2Error>>;

        fn checkOutRevision(
            &self,
            mount_point: &PathString,
            snapshot_hash: &ThriftRootId,
            checkout_mode: &CheckoutMode,
            params: &CheckOutRevisionParams,
        ) -> BoxFuture<'static, Result<Vec<CheckoutConflict>, CheckOutRevisionError>>;

        fn getCheckoutProgressInfo(
            &self,
            params: &CheckoutProgressInfoRequest,
        ) -> BoxFuture<'static, Result<CheckoutProgressInfoResponse, GetCheckoutProgressInfoError>>;

        fn resetParentCommits(
            &self,
            mount_point: &PathString,
            parents: &WorkingDirectoryParents,
            params: &ResetParentCommitsParams,
        ) -> BoxFuture<'static, Result<(), ResetParentCommitsError>>;

        fn getCurrentSnapshotInfo(
            &self,
            params: &GetCurrentSnapshotInfoRequest,
        ) -> BoxFuture<'static, Result<GetCurrentSnapshotInfoResponse, GetCurrentSnapshotInfoError>>;

        fn synchronizeWorkingCopy(
            &self,
            mount_point: &PathString,
            params: &SynchronizeWorkingCopyParams,
        ) -> BoxFuture<'static, Result<(), SynchronizeWorkingCopyError>>;

        fn getSHA1(
            &self,
            mount_point: &PathString,
            paths: &[PathString],
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<SHA1Result>, GetSHA1Error>>;

        fn getBlake3(
            &self,
            mount_point: &PathString,
            paths: &[PathString],
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<Blake3Result>, GetBlake3Error>>;

        fn getDigestHash(
            &self,
            mount_point: &PathString,
            paths: &[PathString],
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<DigestHashResult>, GetDigestHashError>>;

        fn addBindMount(
            &self,
            mount_point: &PathString,
            repo_path: &PathString,
            target_path: &PathString,
        ) -> BoxFuture<'static, Result<(), AddBindMountError>>;

        fn removeBindMount(
            &self,
            mount_point: &PathString,
            repo_path: &PathString,
        ) -> BoxFuture<'static, Result<(), RemoveBindMountError>>;

        fn getCurrentJournalPosition(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<JournalPosition, GetCurrentJournalPositionError>>;

        fn getFilesChangedSince(
            &self,
            mount_point: &PathString,
            from_position: &JournalPosition,
        ) -> BoxFuture<'static, Result<FileDelta, GetFilesChangedSinceError>>;

        fn setJournalMemoryLimit(
            &self,
            mount_point: &PathString,
            limit: i64,
        ) -> BoxFuture<'static, Result<(), SetJournalMemoryLimitError>>;

        fn getJournalMemoryLimit(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<i64, GetJournalMemoryLimitError>>;

        fn flushJournal(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<(), FlushJournalError>>;

        fn debugGetRawJournal(
            &self,
            params: &DebugGetRawJournalParams,
        ) -> BoxFuture<'static, Result<DebugGetRawJournalResponse, DebugGetRawJournalError>>;

        fn getEntryInformation(
            &self,
            mount_point: &PathString,
            paths: &[PathString],
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<EntryInformationOrError>, GetEntryInformationError>>;

        fn getFileInformation(
            &self,
            mount_point: &PathString,
            paths: &[PathString],
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<FileInformationOrError>, GetFileInformationError>>;

        fn getAttributesFromFilesV2(
            &self,
            params: &GetAttributesFromFilesParams,
        ) -> BoxFuture<'static, Result<GetAttributesFromFilesResultV2, GetAttributesFromFilesV2Error>>;

        fn getAttributesFromFiles(
            &self,
            params: &GetAttributesFromFilesParams,
        ) -> BoxFuture<'static, Result<GetAttributesFromFilesResult, GetAttributesFromFilesError>>;

        fn readdir(
            &self,
            params: &ReaddirParams,
        ) -> BoxFuture<'static, Result<ReaddirResult, ReaddirError>>;

        fn glob(
            &self,
            mount_point: &PathString,
            globs: &[String],
        ) -> BoxFuture<'static, Result<Vec<PathString>, GlobError>>;

        fn globFiles(&self, params: &GlobParams) -> BoxFuture<'static, Result<Glob, GlobFilesError>>;

        fn prefetchFiles(
            &self,
            params: &PrefetchParams,
        ) -> BoxFuture<'static, Result<(), PrefetchFilesError>>;

        fn prefetchFilesV2(
            &self,
            params: &PrefetchParams,
        ) -> BoxFuture<'static, Result<PrefetchResult, PrefetchFilesV2Error>>;

        fn predictiveGlobFiles(
            &self,
            params: &GlobParams,
        ) -> BoxFuture<'static, Result<Glob, PredictiveGlobFilesError>>;

        fn chown(
            &self,
            mount_point: &PathString,
            uid: i32,
            gid: i32,
        ) -> BoxFuture<'static, Result<(), ChownError>>;

        fn changeOwnership(
            &self,
            request: &ChangeOwnershipRequest,
        ) -> BoxFuture<'static, Result<ChangeOwnershipResponse, ChangeOwnershipError>>;

        fn getScmStatusV2(
            &self,
            params: &GetScmStatusParams,
        ) -> BoxFuture<'static, Result<GetScmStatusResult, GetScmStatusV2Error>>;

        fn getScmStatus(
            &self,
            mount_point: &PathString,
            list_ignored: bool,
            commit: &ThriftRootId,
        ) -> BoxFuture<'static, Result<ScmStatus, GetScmStatusError>>;

        fn getScmStatusBetweenRevisions(
            &self,
            mount_point: &PathString,
            old_hash: &ThriftRootId,
            new_hash: &ThriftRootId,
        ) -> BoxFuture<'static, Result<ScmStatus, GetScmStatusBetweenRevisionsError>>;

        fn matchFilesystem(
            &self,
            params: &MatchFileSystemRequest,
        ) -> BoxFuture<'static, Result<MatchFileSystemResponse, MatchFilesystemError>>;

        fn getDaemonInfo(&self) -> BoxFuture<'static, Result<DaemonInfo, GetDaemonInfoError>>;

        fn checkPrivHelper(&self) -> BoxFuture<'static, Result<PrivHelperInfo, CheckPrivHelperError>>;

        fn getPid(&self) -> BoxFuture<'static, Result<i64, GetPidError>>;

        fn initiateShutdown(
            &self,
            reason: &str,
        ) -> BoxFuture<'static, Result<(), InitiateShutdownError>>;

        fn getConfig(
            &self,
            params: &GetConfigParams,
        ) -> BoxFuture<'static, Result<EdenConfigData, GetConfigError>>;

        fn reloadConfig(&self) -> BoxFuture<'static, Result<(), ReloadConfigError>>;

        fn debugGetScmTree(
            &self,
            mount_point: &PathString,
            id: &ThriftObjectId,
            local_store_only: bool,
        ) -> BoxFuture<'static, Result<Vec<ScmTreeEntry>, DebugGetScmTreeError>>;

        fn debugGetTree(
            &self,
            request: &DebugGetScmTreeRequest,
        ) -> BoxFuture<'static, Result<DebugGetScmTreeResponse, DebugGetTreeError>>;

        fn debugGetBlob(
            &self,
            request: &DebugGetScmBlobRequest,
        ) -> BoxFuture<'static, Result<DebugGetScmBlobResponse, DebugGetBlobError>>;

        fn debugGetBlobMetadata(
            &self,
            request: &DebugGetBlobMetadataRequest,
        ) -> BoxFuture<'static, Result<DebugGetBlobMetadataResponse, DebugGetBlobMetadataError>>;

        fn debugInodeStatus(
            &self,
            mount_point: &PathString,
            path: &PathString,
            flags: i64,
            sync: &SyncBehavior,
        ) -> BoxFuture<'static, Result<Vec<TreeInodeDebugInfo>, DebugInodeStatusError>>;

        fn debugOutstandingFuseCalls(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<Vec<FuseCall>, DebugOutstandingFuseCallsError>>;

        fn debugOutstandingNfsCalls(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<Vec<NfsCall>, DebugOutstandingNfsCallsError>>;

        fn debugOutstandingPrjfsCalls(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<Vec<PrjfsCall>, DebugOutstandingPrjfsCallsError>>;

        fn debugOutstandingThriftRequests(
            &self,
        ) -> BoxFuture<'static, Result<Vec<ThriftRequestMetadata>, DebugOutstandingThriftRequestsError>>;

        fn debugOutstandingHgEvents(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<Vec<HgEvent>, DebugOutstandingHgEventsError>>;

        fn debugStartRecordingActivity(
            &self,
            mount_point: &PathString,
            output_dir: &PathString,
        ) -> BoxFuture<'static, Result<ActivityRecorderResult, DebugStartRecordingActivityError>>;

        fn debugStopRecordingActivity(
            &self,
            mount_point: &PathString,
            unique: i64,
        ) -> BoxFuture<'static, Result<ActivityRecorderResult, DebugStopRecordingActivityError>>;

        fn debugListActivityRecordings(
            &self,
            mount_point: &PathString,
        ) -> BoxFuture<'static, Result<ListActivityRecordingsResult, DebugListActivityRecordingsError>>;

        fn debugGetInodePath(
            &self,
            mount_point: &PathString,
            inode_number: i64,
        ) -> BoxFuture<'static, Result<InodePathDebugInfo, DebugGetInodePathError>>;

        fn clearFetchCounts(&self) -> BoxFuture<'static, Result<(), ClearFetchCountsError>>;

        fn clearFetchCountsByMount(
            &self,
            mount_path: &PathString,
        ) -> BoxFuture<'static, Result<(), ClearFetchCountsByMountError>>;

        fn getAccessCounts(
            &self,
            duration: i64,
        ) -> BoxFuture<'static, Result<GetAccessCountsResult, GetAccessCountsError>>;

        fn startRecordingBackingStoreFetch(
            &self,
        ) -> BoxFuture<'static, Result<(), StartRecordingBackingStoreFetchError>>;

        fn stopRecordingBackingStoreFetch(
            &self,
        ) -> BoxFuture<'static, Result<GetFetchedFilesResult, StopRecordingBackingStoreFetchError>>;

        fn clearAndCompactLocalStore(
            &self,
        ) -> BoxFuture<'static, Result<(), ClearAndCompactLocalStoreError>>;

        fn debugClearLocalStoreCaches(
            &self,
        ) -> BoxFuture<'static, Result<(), DebugClearLocalStoreCachesError>>;

        fn debugCompactLocalStorage(
            &self,
        ) -> BoxFuture<'static, Result<(), DebugCompactLocalStorageError>>;

        fn debugDropAllPendingRequests(
            &self,
        ) -> BoxFuture<'static, Result<i64, DebugDropAllPendingRequestsError>>;

        fn unloadInodeForPath(
            &self,
            mount_point: &PathString,
            path: &PathString,
            age: &TimeSpec,
        ) -> BoxFuture<'static, Result<i64, UnloadInodeForPathError>>;

        fn debugInvalidateNonMaterialized(
            &self,
            params: &DebugInvalidateRequest,
        ) -> BoxFuture<'static, Result<DebugInvalidateResponse, DebugInvalidateNonMaterializedError>>;

        fn flushStatsNow(&self) -> BoxFuture<'static, Result<(), FlushStatsNowError>>;

        fn invalidateKernelInodeCache(
            &self,
            mount_point: &PathString,
            path: &PathString,
        ) -> BoxFuture<'static, Result<(), InvalidateKernelInodeCacheError>>;

        fn getStatInfo(
            &self,
            params: &GetStatInfoParams,
        ) -> BoxFuture<'static, Result<InternalStats, GetStatInfoError>>;

        fn enableTracing(&self) -> BoxFuture<'static, Result<(), EnableTracingError>>;

        fn disableTracing(&self) -> BoxFuture<'static, Result<(), DisableTracingError>>;

        fn getTracePoints(&self) -> BoxFuture<'static, Result<Vec<TracePoint>, GetTracePointsError>>;

        fn getRetroactiveThriftRequestEvents(
            &self,
        ) -> BoxFuture<
            'static,
            Result<GetRetroactiveThriftRequestEventsResult, GetRetroactiveThriftRequestEventsError>,
        >;

        fn getRetroactiveHgEvents(
            &self,
            params: &GetRetroactiveHgEventsParams,
        ) -> BoxFuture<'static, Result<GetRetroactiveHgEventsResult, GetRetroactiveHgEventsError>>;

        fn getRetroactiveInodeEvents(
            &self,
            params: &GetRetroactiveInodeEventsParams,
        ) -> BoxFuture<'static, Result<GetRetroactiveInodeEventsResult, GetRetroactiveInodeEventsError>>;

        fn injectFault(
            &self,
            fault: &FaultDefinition,
        ) -> BoxFuture<'static, Result<(), InjectFaultError>>;

        fn removeFault(
            &self,
            fault: &RemoveFaultArg,
        ) -> BoxFuture<'static, Result<bool, RemoveFaultError>>;

        fn unblockFault(
            &self,
            info: &UnblockFaultArg,
        ) -> BoxFuture<'static, Result<i64, UnblockFaultError>>;

        fn getBlockedFaults(
            &self,
            request: &GetBlockedFaultsRequest,
        ) -> BoxFuture<'static, Result<GetBlockedFaultsResponse, GetBlockedFaultsError>>;

        fn setPathObjectId(
            &self,
            params: &SetPathObjectIdParams,
        ) -> BoxFuture<'static, Result<SetPathObjectIdResult, SetPathObjectIdError>>;

        fn removeRecursively(
            &self,
            params: &RemoveRecursivelyParams,
        ) -> BoxFuture<'static, Result<(), RemoveRecursivelyError>>;

        fn ensureMaterialized(
            &self,
            params: &EnsureMaterializedParams,
        ) -> BoxFuture<'static, Result<(), EnsureMaterializedError>>;

        fn changesSinceV2(
            &self,
            params: &ChangesSinceV2Params,
        ) -> BoxFuture<'static, Result<ChangesSinceV2Result, ChangesSinceV2Error>>;

        fn startFileAccessMonitor(
            &self,
            params: &StartFileAccessMonitorParams,
        ) -> BoxFuture<'static, Result<StartFileAccessMonitorResult, StartFileAccessMonitorError>>;

        fn stopFileAccessMonitor(
            &self,
        ) -> BoxFuture<'static, Result<StopFileAccessMonitorResult, StopFileAccessMonitorError>>;

        fn sendNotification(
            &self,
            request: &SendNotificationRequest,
        ) -> BoxFuture<'static, Result<SendNotificationResponse, SendNotificationError>>;

        fn listRedirections(
            &self,
            request: &ListRedirectionsRequest,
        ) -> BoxFuture<'static, Result<ListRedirectionsResponse, ListRedirectionsError>>;

        fn getFileContent(
            &self,
            request: &GetFileContentRequest,
        ) -> BoxFuture<'static, Result<GetFileContentResponse, GetFileContentError>>;
    }
}
