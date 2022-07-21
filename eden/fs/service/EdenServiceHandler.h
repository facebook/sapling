/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fb303/BaseService.h>
#include <optional>
#include "eden/fs/eden-config.h"
#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"
#include "eden/fs/telemetry/TraceBus.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace {
class PrefetchFetchContext;
} // namespace

namespace facebook::eden {

class Hash20;
class BlobMetadata;
class EdenMount;
class EdenServer;
class TreeInode;
class ObjectFetchContext;
class EntryAttributes;
struct EntryAttributeFlags;
#ifdef EDEN_HAVE_USAGE_SERVICE
class EdenFSSmartPlatformServiceEndpoint;
#endif
template <typename T>
class ImmediateFuture;

extern const char* const kServiceName;

struct ThriftRequestTraceEvent : TraceEventBase {
  enum Type : unsigned char {
    START,
    FINISH,
  };

  ThriftRequestTraceEvent() = delete;

  static ThriftRequestTraceEvent start(
      uint64_t requestId,
      folly::StringPiece method,
      std::optional<pid_t> clientPid);

  static ThriftRequestTraceEvent finish(
      uint64_t requestId,
      folly::StringPiece method,
      std::optional<pid_t> clientPid);

  ThriftRequestTraceEvent(
      Type type,
      uint64_t requestId,
      folly::StringPiece method,
      std::optional<pid_t> clientPid)
      : type(type),
        requestId(requestId),
        method(method),
        clientPid(clientPid) {}

  Type type;
  uint64_t requestId;
  // Safe to use StringPiece because method names are string literals.
  folly::StringPiece method;
  std::optional<pid_t> clientPid;
};

/*
 * Handler for the EdenService thrift interface
 */
class EdenServiceHandler : virtual public StreamingEdenServiceSvIf,
                           public fb303::BaseService {
 public:
  explicit EdenServiceHandler(
      std::vector<std::string> originalCommandLine,
      EdenServer* server);
  ~EdenServiceHandler() override;

  EdenServiceHandler(EdenServiceHandler const&) = delete;
  EdenServiceHandler& operator=(EdenServiceHandler const&) = delete;

  std::unique_ptr<apache::thrift::AsyncProcessor> getProcessor() override;

  void mount(std::unique_ptr<MountArgument> mount) override;

  void unmount(std::unique_ptr<std::string> mountPoint) override;

  void listMounts(std::vector<MountInfo>& results) override;

  void checkOutRevision(
      std::vector<CheckoutConflict>& results,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> hash,
      CheckoutMode checkoutMode,
      std::unique_ptr<CheckOutRevisionParams> params) override;

  void resetParentCommits(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<WorkingDirectoryParents> parents,
      std::unique_ptr<ResetParentCommitsParams> params) override;

  folly::SemiFuture<folly::Unit> semifuture_synchronizeWorkingCopy(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<SynchronizeWorkingCopyParams> params) override;

  void getBindMounts(
      std::vector<std::string>& out,
      std::unique_ptr<std::string> mountPointPtr) override;
  void addBindMount(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> repoPath,
      std::unique_ptr<std::string> targetPath) override;
  void removeBindMount(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> repoPath) override;

  folly::SemiFuture<std::unique_ptr<std::vector<SHA1Result>>>
  semifuture_getSHA1(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths,
      std::unique_ptr<SyncBehavior> sync) override;

  ImmediateFuture<EntryAttributes> getEntryAttributesForPath(
      EntryAttributeFlags reqBitmask,
      AbsolutePathPiece mountPoint,
      folly::StringPiece path,
      ObjectFetchContext& fetchContext);

  void getCurrentJournalPosition(
      JournalPosition& out,
      std::unique_ptr<std::string> mountPoint) override;

  void getFilesChangedSince(
      FileDelta& out,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<JournalPosition> fromPosition) override;

  void setJournalMemoryLimit(
      std::unique_ptr<PathString> mountPoint,
      int64_t limit) override;

  int64_t getJournalMemoryLimit(
      std::unique_ptr<PathString> mountPoint) override;

  void flushJournal(std::unique_ptr<PathString> mountPoint) override;

  void debugGetRawJournal(
      DebugGetRawJournalResponse& out,
      std::unique_ptr<DebugGetRawJournalParams> params) override;

  folly::SemiFuture<std::unique_ptr<std::vector<EntryInformationOrError>>>
  semifuture_getEntryInformation(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths,
      std::unique_ptr<SyncBehavior> sync) override;

  folly::SemiFuture<std::unique_ptr<std::vector<FileInformationOrError>>>
  semifuture_getFileInformation(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths,
      std::unique_ptr<SyncBehavior> sync) override;

  folly::SemiFuture<std::unique_ptr<GetAttributesFromFilesResult>>
  semifuture_getAttributesFromFiles(
      std::unique_ptr<GetAttributesFromFilesParams> params) override;

  folly::SemiFuture<std::unique_ptr<ReaddirResult>> semifuture_readdir(
      std::unique_ptr<ReaddirParams> params) override;

  folly::SemiFuture<std::unique_ptr<Glob>> semifuture_globFiles(
      std::unique_ptr<GlobParams> params) override;

  folly::SemiFuture<std::unique_ptr<Glob>> semifuture_predictiveGlobFiles(
      std::unique_ptr<GlobParams> params) override;

  folly::Future<folly::Unit> future_chown(
      std::unique_ptr<std::string> mountPoint,
      int32_t uid,
      int32_t gid) override;

  apache::thrift::ServerStream<JournalPosition> subscribeStreamTemporary(
      std::unique_ptr<std::string> mountPoint) override;

  apache::thrift::ServerStream<FsEvent> traceFsEvents(
      std::unique_ptr<std::string> mountPoint,
      int64_t eventCategoryMask) override;

  apache::thrift::ServerStream<ThriftRequestEvent> traceThriftRequestEvents()
      override;

  apache::thrift::ServerStream<HgEvent> traceHgEvents(
      std::unique_ptr<std::string> mountPoint) override;

  apache::thrift::ServerStream<InodeEvent> traceInodeEvents(
      std::unique_ptr<std::string> mountPoint) override;

  folly::SemiFuture<std::unique_ptr<GetScmStatusResult>>
  semifuture_getScmStatusV2(
      std::unique_ptr<GetScmStatusParams> params) override;

  apache::thrift::ResponseAndServerStream<ChangesSinceResult, ChangedFileResult>
  streamChangesSince(std::unique_ptr<StreamChangesSinceParams> params) override;

  folly::SemiFuture<std::unique_ptr<ScmStatus>> semifuture_getScmStatus(
      std::unique_ptr<std::string> mountPoint,
      bool listIgnored,
      std::unique_ptr<std::string> commitHash) override;

  folly::SemiFuture<std::unique_ptr<ScmStatus>>
  semifuture_getScmStatusBetweenRevisions(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> oldHash,
      std::unique_ptr<std::string> newHash) override;

  void debugGetScmTree(
      std::vector<ScmTreeEntry>& entries,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> id,
      bool localStoreOnly) override;

  void debugGetScmBlob(
      std::string& data,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> id,
      bool localStoreOnly) override;

  void debugGetScmBlobMetadata(
      ScmBlobMetadata& metadata,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> id,
      bool localStoreOnly) override;

  void debugInodeStatus(
      std::vector<TreeInodeDebugInfo>& inodeInfo,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> path,
      int64_t flags,
      std::unique_ptr<SyncBehavior> sync) override;

  void debugOutstandingFuseCalls(
      std::vector<FuseCall>& outstandingCalls,
      std::unique_ptr<std::string> mountPoint) override;

  void debugOutstandingNfsCalls(
      std::vector<NfsCall>& outstandingCalls,
      std::unique_ptr<std::string> mountPoint) override;

  void debugOutstandingPrjfsCalls(
      std::vector<PrjfsCall>& outstandingCalls,
      std::unique_ptr<std::string> mountPoint) override;

  void debugOutstandingThriftRequests(
      std::vector<ThriftRequestMetadata>& outstandingCalls) override;

  void debugStartRecordingActivity(
      ActivityRecorderResult& result,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> outputPath) override;

  void debugStopRecordingActivity(
      ActivityRecorderResult& result,
      std::unique_ptr<std::string> mountPoint,
      int64_t unique) override;

  void debugListActivityRecordings(
      ListActivityRecordingsResult& result,
      std::unique_ptr<std::string> mountPoint) override;

  void debugGetInodePath(
      InodePathDebugInfo& inodePath,
      std::unique_ptr<std::string> mountPoint,
      int64_t inodeNumber) override;

  void clearFetchCounts() override;

  void clearFetchCountsByMount(std::unique_ptr<std::string> mountPath) override;

  void getAccessCounts(GetAccessCountsResult& result, int64_t duration)
      override;

  void clearAndCompactLocalStore() override;

  void debugClearLocalStoreCaches() override;

  void debugCompactLocalStorage() override;

  int64_t debugDropAllPendingRequests() override;

  int64_t unloadInodeForPath(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> path,
      std::unique_ptr<TimeSpec> age) override;

  void flushStatsNow() override;

  folly::SemiFuture<folly::Unit> semifuture_invalidateKernelInodeCache(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> path) override;

  void getStatInfo(
      InternalStats& result,
      std::unique_ptr<GetStatInfoParams> params) override;

  void enableTracing() override;
  void disableTracing() override;
  void getTracePoints(std::vector<TracePoint>& result) override;

  void getRetroactiveInodeEvents(
      GetRetroactiveInodeEventsResult& result,
      std::unique_ptr<GetRetroactiveInodeEventsParams> params) override;

  void injectFault(std::unique_ptr<FaultDefinition> fault) override;
  bool removeFault(std::unique_ptr<RemoveFaultArg> fault) override;
  int64_t unblockFault(std::unique_ptr<UnblockFaultArg> info) override;

  folly::SemiFuture<std::unique_ptr<SetPathObjectIdResult>>
  semifuture_setPathObjectId(
      std::unique_ptr<SetPathObjectIdParams> params) override;

  folly::SemiFuture<folly::Unit> semifuture_removeRecursively(
      std::unique_ptr<RemoveRecursivelyParams> params) override;

  folly::SemiFuture<folly::Unit> semifuture_ensureMaterialized(
      std::unique_ptr<EnsureMaterializedParams> params) override;

  void reloadConfig() override;

  void getDaemonInfo(DaemonInfo& result) override;

  /**
   * Checks the PrivHelper connection.
   * For Windows, result.connected will always be set to true.
   */
  void checkPrivHelper(PrivHelperInfo& result) override;

  int64_t getPid() override;

  /**
   * A thrift client has requested that we shutdown.
   */
  void initiateShutdown(std::unique_ptr<std::string> reason) override;

  void getConfig(
      EdenConfigData& result,
      std::unique_ptr<GetConfigParams> params) override;

  /**
   * Enable all backing stores to record fetched files
   */
  void startRecordingBackingStoreFetch() override;

  /**
   * Make all backing stores stop recording
   * fetched files. Previous records for different kinds of backing
   * stores will be returned by backing store types.
   */
  void stopRecordingBackingStoreFetch(GetFetchedFilesResult& results) override;

  /**
   * Returns the pid that caused the Thrift request running on the calling
   * Thrift worker thread and registers it with the ProcessNameCache.
   *
   * This must be run from a Thrift worker thread, because the calling pid is
   * stored in a thread local variable.
   */
  std::optional<pid_t> getAndRegisterClientPid();

 private:
  ImmediateFuture<Hash20> getSHA1ForPath(
      const EdenMount& edenMount,
      RelativePath path,
      ObjectFetchContext& fetchContext);

  folly::Synchronized<std::unordered_map<uint64_t, ThriftRequestTraceEvent>>
      outstandingThriftRequests_;
#ifdef EDEN_HAVE_USAGE_SERVICE
  // an endpoint for the edenfs/edenfs_service smartservice used for
  // predictive prefetch profiles
  std::unique_ptr<EdenFSSmartPlatformServiceEndpoint> spServiceEndpoint_;
#endif
  const std::vector<std::string> originalCommandLine_;
  EdenServer* const server_;

  std::vector<TraceSubscriptionHandle<ThriftRequestTraceEvent>>
      thriftRequestTraceSubscriptionHandles_;

  std::shared_ptr<TraceBus<ThriftRequestTraceEvent>> thriftRequestTraceBus_;
};
} // namespace facebook::eden
