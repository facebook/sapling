/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"
#include "eden/fs/utils/PathFuncs.h"
#include "fb303/BaseService.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class Hash;
class EdenMount;
class EdenServer;
class TreeInode;

/*
 * Handler for the EdenService thrift interface
 */
class EdenServiceHandler : virtual public StreamingEdenServiceSvIf,
                           public fb303::BaseService {
 public:
  explicit EdenServiceHandler(
      std::vector<std::string> originalCommandLine,
      EdenServer* server);

  EdenServiceHandler(EdenServiceHandler const&) = delete;
  EdenServiceHandler& operator=(EdenServiceHandler const&) = delete;

  std::unique_ptr<apache::thrift::AsyncProcessor> getProcessor() override;

  fb303::cpp2::fb303_status getStatus() override;

  void mount(std::unique_ptr<MountArgument> mount) override;

  void unmount(std::unique_ptr<std::string> mountPoint) override;

  void listMounts(std::vector<MountInfo>& results) override;

  void checkOutRevision(
      std::vector<CheckoutConflict>& results,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> hash,
      CheckoutMode checkoutMode) override;

  void resetParentCommits(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<WorkingDirectoryParents> parents) override;

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

  void getSHA1(
      std::vector<SHA1Result>& out,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths) override;

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

  folly::Future<std::unique_ptr<std::vector<FileInformationOrError>>>
  future_getFileInformation(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths) override;

  void glob(
      std::vector<std::string>& out,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> globs) override;

  folly::Future<std::unique_ptr<Glob>> future_globFiles(
      std::unique_ptr<GlobParams> params) override;

  folly::Future<folly::Unit> future_chown(
      std::unique_ptr<std::string> mountPoint,
      int32_t uid,
      int32_t gid) override;

#ifndef _WIN32
  apache::thrift::Stream<JournalPosition> subscribeStreamTemporary(
      std::unique_ptr<std::string> mountPoint) override;
#endif // !_WIN32

  void getManifestEntry(
      ManifestEntry& out,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> relativePath) override;

  void async_tm_getScmStatus(
      std::unique_ptr<
          apache::thrift::HandlerCallback<std::unique_ptr<ScmStatus>>> callback,
      std::unique_ptr<std::string> mountPoint,
      bool listIgnored,
      std::unique_ptr<std::string> commitHash) override;

  folly::Future<std::unique_ptr<ScmStatus>> future_getScmStatusBetweenRevisions(
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
      std::unique_ptr<std::string> path) override;

  void debugOutstandingFuseCalls(
      std::vector<FuseCall>& outstandingCalls,
      std::unique_ptr<std::string> mountPoint) override;

  void debugGetInodePath(
      InodePathDebugInfo& inodePath,
      std::unique_ptr<std::string> mountPoint,
      int64_t inodeNumber) override;

  void debugSetLogLevel(
      SetLogLevelResult& result,
      std::unique_ptr<std::string> category,
      std::unique_ptr<std::string> level) override;

  void getAccessCounts(GetAccessCountsResult& result, int64_t duration)
      override;

  void clearAndCompactLocalStore() override;

  void debugClearLocalStoreCaches() override;

  void debugCompactLocalStorage() override;

  int64_t unloadInodeForPath(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> path,
      std::unique_ptr<TimeSpec> age) override;

  void flushStatsNow() override;

  folly::Future<folly::Unit> future_invalidateKernelInodeCache(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> path) override;

  void getStatInfo(InternalStats& result) override;

  void enableTracing() override;
  void disableTracing() override;
  void getTracePoints(std::vector<TracePoint>& result) override;

  void injectFault(std::unique_ptr<FaultDefinition> fault) override;
  bool removeFault(std::unique_ptr<RemoveFaultArg> fault) override;
  int64_t unblockFault(std::unique_ptr<UnblockFaultArg> info) override;

  void reloadConfig() override;

  void getDaemonInfo(DaemonInfo& result) override;

  int64_t getPid() override;

  /**
   * A thrift client has requested that we shutdown.
   */
  void initiateShutdown(std::unique_ptr<std::string> reason) override;

  void getConfig(
      EdenConfigData& result,
      std::unique_ptr<GetConfigParams> params) override;

 private:
  folly::Future<Hash> getSHA1ForPath(
      folly::StringPiece mountPoint,
      folly::StringPiece path);

  folly::Future<Hash> getSHA1ForPathDefensively(
      folly::StringPiece mountPoint,
      folly::StringPiece path) noexcept;

  /**
   * If `filename` exists in the manifest as a file (not a directory), returns
   * the mode of the file as recorded in the manifest.
   */
  std::optional<mode_t> isInManifestAsFile(
      const EdenMount* mount,
      const RelativePathPiece filename);

  const std::vector<std::string> originalCommandLine_;
  EdenServer* const server_;
};
} // namespace eden
} // namespace facebook
