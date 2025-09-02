/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServiceHandler.h"

#include <sys/types.h>
#include <algorithm>
#include <optional>
#include <stdexcept>
#include <typeinfo>

#include <fb303/ServiceData.h>
#include <fmt/format.h>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Portability.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/executors/SerialExecutor.h>
#include <folly/futures/Future.h>
#include <folly/logging/Logger.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <re2/re2.h>
#include <thrift/lib/cpp/util/EnumUtils.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "ThriftGetObjectImpl.h"
#include "eden/common/telemetry/SessionInfo.h"
#include "eden/common/telemetry/Tracing.h"
#include "eden/common/utils/Bug.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/common/utils/StatTimes.h"
#include "eden/common/utils/String.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/Traverse.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/VirtualInodeLoader.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/GlobEntry.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/nfs/Nfsd3.h"
#ifdef _WIN32
#include "eden/fs/notifications/Notifier.h"
#endif
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/prjfs/PrjfsChannel.h"
#include "eden/fs/rust/redirect_ffi/include/ffi.h"
#include "eden/fs/rust/redirect_ffi/src/lib.rs.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/ThriftGetObjectImpl.h"
#include "eden/fs/service/ThriftGlobImpl.h"
#include "eden/fs/service/ThriftPermissionChecker.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/UsageService.h"
#include "eden/fs/service/gen-cpp2/eden_constants.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/service/gen-cpp2/streamingeden_constants.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/Diff.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/FilteredBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/PathLoader.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/store/TreeLookupProcessor.h"
#include "eden/fs/store/filter/GlobFilter.h"
#include "eden/fs/store/hg/SaplingBackingStore.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/TaskTrace.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/GlobMatcher.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/ProcUtil.h"
#include "eden/fs/utils/SourceLocation.h"

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Try;
using folly::Unit;
using std::string;
using std::unique_ptr;
using std::vector;
using namespace std::literals::string_view_literals;

namespace {
using namespace facebook::eden;

std::string getClientCmdline(
    const std::shared_ptr<ServerState>& serverState_,
    const ObjectFetchContextPtr& context_) {
  std::string client_cmdline = "<unknown>";
  if (auto clientPid = context_->getClientPid()) {
    // TODO: we should look up client scope here instead of command line
    // since it will give move context into the overarching process or
    // system producing the expensive query
    const ProcessInfo* processInfoPtr = serverState_->getProcessInfoCache()
                                            ->lookup(clientPid.value().get())
                                            .get_optional();
    if (processInfoPtr) {
      client_cmdline = processInfoPtr->name;
      std::replace(client_cmdline.begin(), client_cmdline.end(), '\0', ' ');
    }
  }
  return client_cmdline;
}

std::string logHash(StringPiece thriftArg) {
  if (thriftArg.size() == Hash20::RAW_SIZE) {
    return Hash20{folly::ByteRange{thriftArg}}.toString();
  } else if (thriftArg.size() == Hash20::RAW_SIZE * 2) {
    return Hash20{thriftArg}.toString();
  } else {
    return folly::hexlify(thriftArg);
  }
}

std::string logPosition(JournalPosition position) {
  return fmt::format(
      "{}:{}:{}",
      position.mountGeneration().value(),
      position.sequenceNumber().value(),
      logHash(position.snapshotHash().value()));
}

/**
 * Convert a vector of strings from a thrift argument to a field
 * that we can log in an INSTRUMENT_THRIFT_CALL() log message.
 *
 * This truncates very log lists to only log the first few elements.
 */
std::string toLogArg(const std::vector<std::string>& args) {
  constexpr size_t limit = 5;
  if (args.size() <= limit) {
    return fmt::format("[{}]", fmt::join(args, ", "));
  } else {
    return fmt::format(
        "[{}, and {} more]",
        fmt::join(args.begin(), args.begin() + limit, ", "),
        args.size() - limit);
  }
}

bool mountIsUsingFilteredFS(const EdenMountHandle& mount) {
  return mount.getEdenMountPtr()
             ->getCheckoutConfig()
             ->getRepoBackingStoreType() == BackingStoreType::FILTEREDHG;
}

bool isValidSearchRoot(const PathString& searchRoot) {
  return searchRoot.empty() || (searchRoot == ".") || (searchRoot == "html") ||
      (searchRoot == "www/html") || (searchRoot == "www\\html");
}

std::string resolveRootId(
    std::string rootId,
    const RootIdOptions& rootIdOptions,
    const EdenMountHandle& mount) {
  if (mountIsUsingFilteredFS(mount)) {
    if (rootIdOptions.filterId()) {
      return FilteredBackingStore::createFilteredRootId(
          rootId, *rootIdOptions.filterId());
    } else {
      return FilteredBackingStore::createNullFilteredRootId(rootId);
    }
  } else {
    return rootId;
  }
}

// parseRootId() assumes that the provided id will contain information
// about the active filter. Some legacy code paths do not respect
// filters (or accept Filters as arguments), so we need to construct a
// FilteredRootId using the last active filter. For non-FilteredFS repos, the
// last filterID will be std::nullopt.
std::string resolveRootIdWithLastFilter(
    std::string rootId,
    const EdenMountHandle& handle) {
  auto filterId =
      handle.getEdenMount().getCheckoutConfig()->getLastActiveFilter();
  RootIdOptions rootIdOptions{};
  rootIdOptions.filterId().from_optional(std::move(filterId));
  return resolveRootId(std::move(rootId), rootIdOptions, handle);
}

// Similar to the above function, but can be used with endpoints that pass in
// many RootIds.
std::vector<std::string> resolveRootsWithLastFilter(
    std::vector<std::string>& originalRootIds,
    const EdenMountHandle& mountHandle) {
  std::vector<std::string> resolvedRootIds;
  resolvedRootIds.reserve(originalRootIds.size());
  for (auto& rev : originalRootIds) {
    resolvedRootIds.push_back(
        resolveRootIdWithLastFilter(std::move(rev), mountHandle));
  }
  return resolvedRootIds;
}

#define EDEN_MICRO reinterpret_cast<const char*>(u8"\u00B5s")

class ThriftFetchContext : public ObjectFetchContext {
 public:
  explicit ThriftFetchContext(
      OptionalProcessId pid,
      folly::StringPiece endpoint)
      : pid_(pid), endpoint_(endpoint) {}

  OptionalProcessId getClientPid() const override {
    return pid_;
  }

  Cause getCause() const override {
    return ObjectFetchContext::Cause::Thrift;
  }

  std::optional<std::string_view> getCauseDetail() const override {
    return endpoint_;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return &requestInfo_;
  }

  /**
   * Update the request info map.
   *
   * This is not thread safe and the caller should make sure that this function
   * isn't called in an unsafe manner.
   */
  void updateRequestInfo(const std::map<std::string, std::string>& another) {
    requestInfo_.insert(another.begin(), another.end());
  }

  void fillClientRequestInfo(
      apache::thrift::optional_field_ref<ClientRequestInfo&>
          clientRequestInfo) {
    if (clientRequestInfo.has_value()) {
      auto correlator = clientRequestInfo->correlator();
      auto entry_point = clientRequestInfo->entry_point();
      if (!(correlator->empty() || entry_point->empty())) {
        updateRequestInfo(
            {{ObjectFetchContext::kClientCorrelator, *correlator},
             {ObjectFetchContext::kClientEntryPoint, *entry_point}});
      }
    }
  }

 private:
  OptionalProcessId pid_;
  std::string_view endpoint_;
  std::unordered_map<std::string, std::string> requestInfo_;
};

class PrefetchFetchContext : public ObjectFetchContext {
 public:
  explicit PrefetchFetchContext(
      OptionalProcessId pid,
      std::string_view endpoint)
      : pid_(pid), endpoint_(endpoint) {}

  OptionalProcessId getClientPid() const override {
    return pid_;
  }

  Cause getCause() const override {
    return ObjectFetchContext::Cause::Prefetch;
  }

  std::optional<std::string_view> getCauseDetail() const override {
    return endpoint_;
  }

  virtual ImportPriority getPriority() const override {
    return kThriftPrefetchPriority;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

 private:
  OptionalProcessId pid_;
  std::string_view endpoint_;
};

/**
 * Lives as long as a Thrift request and primarily exists to record logging and
 * telemetry.
 */
class ThriftRequestScope {
 public:
  ThriftRequestScope(ThriftRequestScope&&) = delete;
  ThriftRequestScope& operator=(ThriftRequestScope&&) = delete;

  template <typename JoinFn>
  ThriftRequestScope(
      std::shared_ptr<TraceBus<ThriftRequestTraceEvent>> traceBus,
      const folly::Logger& logger,
      folly::LogLevel level,
      SourceLocation sourceLocation,
      EdenStatsPtr edenStats,
      ThriftStats::DurationPtr statPtr,
      OptionalProcessId pid,
      JoinFn&& join)
      : traceBus_{std::move(traceBus)},
        requestId_(generateUniqueID()),
        sourceLocation_{sourceLocation},
        edenStats_{std::move(edenStats)},
        statPtr_{std::move(statPtr)},
        level_(level),
        itcLogger_(logger),
        thriftFetchContext_{makeRefPtr<ThriftFetchContext>(
            pid,
            sourceLocation_.function_name())},
        prefetchFetchContext_{makeRefPtr<PrefetchFetchContext>(
            pid,
            sourceLocation_.function_name())} {
    FB_LOG_RAW(
        itcLogger_,
        level,
        sourceLocation.file_name(),
        sourceLocation.line(),
        "")
        << "[" << requestId_ << "] " << sourceLocation.function_name() << "("
        << join() << ")";

    traceBus_->publish(ThriftRequestTraceEvent::start(
        requestId_, sourceLocation_.function_name(), pid));
  }

  ~ThriftRequestScope() {
    // Logging completion time for the request
    // The line number points to where the object was originally created
    auto elapsed = itcTimer_.elapsed();
    auto level = level_;
    if (elapsed > std::chrono::seconds(1)) {
      // When a request takes over a second, let's raise the loglevel to draw
      // attention to it
      level += 1;
    }
    FB_LOG_RAW(
        itcLogger_,
        level,
        sourceLocation_.file_name(),
        sourceLocation_.line(),
        "")
        << "[" << requestId_ << "] "
        << fmt::format(
               "{}() took {} {}",
               sourceLocation_.function_name(),
               elapsed.count(),
               EDEN_MICRO);
    if (edenStats_) {
      edenStats_->addDuration(statPtr_, elapsed);
    }
    traceBus_->publish(ThriftRequestTraceEvent::finish(
        requestId_,
        sourceLocation_.function_name(),
        thriftFetchContext_->getClientPid()));
  }

  const ObjectFetchContextPtr& getPrefetchFetchContext() {
    return prefetchFetchContext_.as<ObjectFetchContext>();
  }

  ThriftFetchContext& getThriftFetchContext() {
    return *thriftFetchContext_;
  }

  const ObjectFetchContextPtr& getFetchContext() {
    return thriftFetchContext_.as<ObjectFetchContext>();
  }

  folly::StringPiece getFunctionName() {
    return sourceLocation_.function_name();
  }

 private:
  std::shared_ptr<TraceBus<ThriftRequestTraceEvent>> traceBus_;
  uint64_t requestId_;
  SourceLocation sourceLocation_;
  EdenStatsPtr edenStats_;
  ThriftStats::DurationPtr statPtr_;
  folly::LogLevel level_;
  folly::Logger itcLogger_;
  folly::stop_watch<std::chrono::microseconds> itcTimer_ = {};
  RefPtr<ThriftFetchContext> thriftFetchContext_;
  RefPtr<PrefetchFetchContext> prefetchFetchContext_;
};

template <typename ReturnType>
Future<ReturnType> wrapFuture(
    std::unique_ptr<ThriftRequestScope> logHelper,
    folly::Future<ReturnType>&& f) {
  return std::move(f).ensure([logHelper = std::move(logHelper)]() {});
}

template <typename ReturnType>
ImmediateFuture<ReturnType> wrapImmediateFuture(
    std::unique_ptr<ThriftRequestScope> logHelper,
    ImmediateFuture<ReturnType>&& f) {
  return std::move(f).ensure([logHelper = std::move(logHelper)]() {});
}

/**
 * Lives as long as a suffix glob request and primarily exists to record logging
 * and telemetry.
 */
class SuffixGlobRequestScope {
 public:
  SuffixGlobRequestScope(SuffixGlobRequestScope&&) = delete;
  SuffixGlobRequestScope& operator=(SuffixGlobRequestScope&&) = delete;

  SuffixGlobRequestScope(
      std::string globberLogString,
      const std::shared_ptr<ServerState>& serverState,
      bool isLocal,
      const ObjectFetchContextPtr& context)
      : globberLogString_{std::move(globberLogString)},
        serverState_{serverState},
        isLocal_{isLocal},
        context_{context} {}

  ~SuffixGlobRequestScope() {
    // Logging completion time for the request
    auto elapsed = itcTimer_.elapsed();
    auto duration = std::chrono::duration<double>{elapsed}.count();
    std::string client_cmdline = getClientCmdline(serverState_, context_);
    XLOGF(
        DBG4,
        "EdenFS asked to evaluate suffix glob by caller '{}'{}: duration={}s",
        client_cmdline,
        globberLogString_,
        duration);
    serverState_->getStructuredLogger()->logEvent(SuffixGlob{
        duration, globberLogString_, std::move(client_cmdline), isLocal_});
  }

 private:
  std::string globberLogString_;
  const std::shared_ptr<ServerState>& serverState_;
  bool isLocal_;
  const ObjectFetchContextPtr& context_;
  folly::stop_watch<std::chrono::microseconds> itcTimer_ = {};
}; // namespace

/**
 * Lives as long as a glob files request and primarily exists to record logging
 * and telemetry.
 */
class GlobFilesRequestScope {
 public:
  GlobFilesRequestScope(GlobFilesRequestScope&&) = delete;
  GlobFilesRequestScope& operator=(GlobFilesRequestScope&&) = delete;

  explicit GlobFilesRequestScope(
      const std::shared_ptr<ServerState>& serverState,
      bool isOffloadable,
      std::string logString,
      const ObjectFetchContextPtr& context)
      : serverState_{serverState},
        isOffloadable_{isOffloadable},
        logString_{logString},
        context_{context} {}

  ~GlobFilesRequestScope() {
    // Logging completion time for the request
    auto elapsed = itcTimer_.elapsed();
    auto duration = std::chrono::duration<double>{elapsed}.count();
    XLOGF(
        DBG4,
        "EdenFS completed globFiles request in {}s using {}{}",
        duration,
        (local ? "Local" : "SaplingRemoteAPI"),
        (fallback ? " Fallback" : ""));

    // Log if this request is an expensive request
    if (duration >= EXPENSIVE_GLOB_FILES_DURATION) {
      std::string client_cmdline = getClientCmdline(serverState_, context_);

      serverState_->getStructuredLogger()->logEvent(ExpensiveGlob{
          duration, logString_, std::move(client_cmdline), local});
    }
    if (local) {
      if (isOffloadable_) {
        serverState_->getStats()->addDuration(
            &ThriftStats::globFilesLocalOffloadableDuration, elapsed);
      } else {
        serverState_->getStats()->addDuration(
            &ThriftStats::globFilesLocalDuration, elapsed);
      }
      serverState_->getStats()->increment(&ThriftStats::globFilesLocal);
    } else {
      if (fallback) {
        serverState_->getStats()->addDuration(
            &ThriftStats::globFilesSaplingRemoteAPIFallbackDuration, elapsed);
        serverState_->getStats()->increment(
            &ThriftStats::globFilesSaplingRemoteAPIFallback);
      } else {
        serverState_->getStats()->addDuration(
            &ThriftStats::globFilesSaplingRemoteAPISuccessDuration, elapsed);
        serverState_->getStats()->increment(
            &ThriftStats::globFilesSaplingRemoteAPISuccess);
      }
    }
    XLOG(DBG4, "End of globFiles");
  }

  void setLocal(bool isLocal) {
    local = isLocal;
  }

  void setFallback(bool isFallback) {
    fallback = isFallback;
  }

 private:
  bool local = true;
  bool fallback = false;
  const std::shared_ptr<ServerState>& serverState_;
  bool isOffloadable_;
  std::string logString_;
  const ObjectFetchContextPtr& context_;
  folly::stop_watch<std::chrono::microseconds> itcTimer_ = {};
}; // namespace
#undef EDEN_MICRO

RelativePath relpathFromUserPath(StringPiece userPath) {
  if (userPath.empty() || userPath == ".") {
    return RelativePath{};
  } else {
    return RelativePath{userPath};
  }
}

RelativePathPiece relpathPieceFromUserPath(StringPiece userPath) {
  if (userPath.empty() || userPath == ".") {
    return RelativePathPiece{};
  } else {
    return RelativePathPiece{userPath};
  }
}

facebook::eden::InodePtr inodeFromUserPath(
    facebook::eden::EdenMount& mount,
    StringPiece rootRelativePath,
    const ObjectFetchContextPtr& context) {
  auto relPath = relpathFromUserPath(rootRelativePath);
  return mount.getInodeSlow(relPath, context).get();
}

bool shouldUseSaplingRemoteAPI(
    bool useSaplingRemoteAPISuffixes,
    const GlobParams& params) {
  // The following parameters will default to local lookup
  // Commands related to prefetching or the working copy
  //   - prefetchFiles
  //   - suppressFileList
  // - searchRoot - root is always the repository root
  // - predictiveGlob - This pathway only accepts suffixes
  // - listOnlyFiles - Only files will be returned
  // Ignore
  //   - prefetchMetadata, it is explicitly called
  // out as having no effect
  //   - sync, not used globFiles. If sync behavior is desired
  //   use synchronizeWorkingCopy

  // Handle unsupported flags
  if (*params.prefetchFiles() || *params.suppressFileList()) {
    XLOGF(
        DBG3,
        "globFiles request cannot be offloaded to SaplingRemoteAPI due to prefetching: prefetchFiles={}, suppressFileList={}. Falling back to local pathway",
        *params.prefetchFiles(),
        *params.suppressFileList());
    useSaplingRemoteAPISuffixes = false;
  } else if (params.predictiveGlob()) {
    XLOG(
        DBG3,
        "globFiles request cannot be offloaded to SaplingRemoteAPI due to predictiveGlob, falling back to local pathway");
    useSaplingRemoteAPISuffixes = false;
  } else if (!(*params.listOnlyFiles())) {
    XLOG(
        DBG3,
        "globFiles request cannot be offloaded to SaplingRemoteAPI due to asking for files and directories, falling back to local pathway");
    useSaplingRemoteAPISuffixes = false;
  }

  return useSaplingRemoteAPISuffixes;
}

bool checkAllowedQuery(
    const std::vector<std::string>& suffixes,
    const std::unordered_set<std::string>& allowedSuffixes) {
  for (auto& suffix : suffixes) {
    if (!allowedSuffixes.contains(suffix)) {
      XLOGF(DBG4, "Suffix {} is not in allowed suffixes", suffix);
      return false;
    }
  }
  XLOGF(DBG4, "All suffixes allowed");
  return true;
}

} // namespace

// INSTRUMENT_THRIFT_CALL returns a unique pointer to
// ThriftRequestScope object. The returned pointer can be used to call
// wrapFuture() to attach a log message on the completion of the Future. This
// must be called in a Thrift worker thread because the calling pid of
// getAndRegisterClientPid is stored in a thread local variable.

// When not attached to Future it will log the completion of the operation and
// time taken to complete it.
#define INSTRUMENT_THRIFT_CALL(level, ...)                    \
  ([&](SourceLocation loc) {                                  \
    static folly::Logger logger(                              \
        fmt::format("eden.thrift.{}", loc.function_name()));  \
    return std::make_unique<ThriftRequestScope>(              \
        this->thriftRequestTraceBus_,                         \
        logger,                                               \
        folly::LogLevel::level,                               \
        loc,                                                  \
        nullptr,                                              \
        nullptr,                                              \
        getAndRegisterClientPid(),                            \
        [&] {                                                 \
          return fmt::to_string(                              \
              fmt::join(std::make_tuple(__VA_ARGS__), ", ")); \
        });                                                   \
  }(EDEN_CURRENT_SOURCE_LOCATION))

#define INSTRUMENT_THRIFT_CALL_WITH_STAT(level, stat, ...)    \
  ([&](SourceLocation loc) {                                  \
    static folly::Logger logger(                              \
        fmt::format("eden.thrift.{}", loc.function_name()));  \
    return std::make_unique<ThriftRequestScope>(              \
        this->thriftRequestTraceBus_,                         \
        logger,                                               \
        folly::LogLevel::level,                               \
        loc,                                                  \
        server_->getStats().copy(),                           \
        stat,                                                 \
        getAndRegisterClientPid(),                            \
        [&] {                                                 \
          return fmt::to_string(                              \
              fmt::join(std::make_tuple(__VA_ARGS__), ", ")); \
        });                                                   \
  }(EDEN_CURRENT_SOURCE_LOCATION))

ThriftRequestTraceEvent ThriftRequestTraceEvent::start(
    uint64_t requestId,
    folly::StringPiece method,
    OptionalProcessId clientPid) {
  return ThriftRequestTraceEvent{
      ThriftRequestTraceEvent::START, requestId, method, clientPid};
}

ThriftRequestTraceEvent ThriftRequestTraceEvent::finish(
    uint64_t requestId,
    folly::StringPiece method,
    OptionalProcessId clientPid) {
  return ThriftRequestTraceEvent{
      ThriftRequestTraceEvent::FINISH, requestId, method, clientPid};
}

template <>
struct fmt::formatter<facebook::eden::MountId> : public formatter<std::string> {
  template <typename Context>
  auto format(const facebook::eden::MountId& mountId, Context& ctx) const {
    return formatter<std::string>::format(*mountId.mountPoint(), ctx);
  }
};

namespace facebook::eden {

const char* const kServiceName = "EdenFS";

std::optional<ActivityBuffer<ThriftRequestTraceEvent>>
EdenServiceHandler::initThriftRequestActivityBuffer() {
  if (server_->getServerState()
          ->getEdenConfig()
          ->enableActivityBuffer.getValue()) {
    return std::make_optional<ActivityBuffer<ThriftRequestTraceEvent>>(
        server_->getServerState()
            ->getEdenConfig()
            ->activityBufferMaxEvents.getValue());
  }
  return std::nullopt;
}

EdenServiceHandler::EdenServiceHandler(
    std::vector<std::string> originalCommandLine,
    EdenServer* server,
    std::unique_ptr<UsageService> usageService)
    : BaseService{kServiceName},
      originalCommandLine_{std::move(originalCommandLine)},
      server_{server},
      usageService_{std::move(usageService)},
      thriftRequestActivityBuffer_(initThriftRequestActivityBuffer()),
      thriftRequestTraceBus_(TraceBus<ThriftRequestTraceEvent>::create(
          "ThriftRequestTrace",
          server_->getServerState()
              ->getEdenConfig()
              ->ThriftTraceBusCapacity.getValue())) {
  thriftRequestTraceHandle_ = thriftRequestTraceBus_->subscribeFunction(
      "Outstanding Thrift request tracing",
      [this](const ThriftRequestTraceEvent& event) {
        switch (event.type) {
          case ThriftRequestTraceEvent::START:
            outstandingThriftRequests_.wlock()->emplace(event.requestId, event);
            break;
          case ThriftRequestTraceEvent::FINISH:
            outstandingThriftRequests_.wlock()->erase(event.requestId);
            break;
        }
        if (thriftRequestActivityBuffer_.has_value()) {
          thriftRequestActivityBuffer_->addEvent(event);
        }
      });
}

EdenServiceHandler::~EdenServiceHandler() = default;

EdenMountHandle EdenServiceHandler::lookupMount(const MountId& mountId) {
  return lookupMount(mountId.mountPoint());
}

EdenMountHandle EdenServiceHandler::lookupMount(
    const std::unique_ptr<std::string>& mountId) {
  return lookupMount(*mountId);
}

EdenMountHandle EdenServiceHandler::lookupMount(
    apache::thrift::field_ref<std::string&> mountId) {
  return lookupMount(*mountId);
}

EdenMountHandle EdenServiceHandler::lookupMount(
    apache::thrift::field_ref<const std::string&> mountId) {
  return lookupMount(*mountId);
}

EdenMountHandle EdenServiceHandler::lookupMount(const std::string& mountId) {
  auto mountPath = absolutePathFromThrift(mountId);
  return server_->getMount(mountPath);
}

std::unique_ptr<apache::thrift::AsyncProcessor>
EdenServiceHandler::getProcessor() {
  auto processor = StreamingEdenServiceSvIf::getProcessor();
  if (server_->getServerState()
          ->getEdenConfig()
          ->thriftUseCustomPermissionChecking.getValue()) {
    processor->addEventHandler(
        std::make_shared<ThriftPermissionChecker>(server_->getServerState()));
  }
  return processor;
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_mount(
    std::unique_ptr<MountArgument> argument) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO, (*argument->mountPoint()));
  return wrapImmediateFuture(
             std::move(helper),
             makeImmediateFutureWith([&] {
               auto mountPoint =
                   absolutePathFromThrift(*argument->mountPoint());
               auto edenClientPath =
                   absolutePathFromThrift(*argument->edenClientPath());
               auto initialConfig = CheckoutConfig::loadFromClientDirectory(
                   mountPoint, edenClientPath);

               return server_
                   ->mount(std::move(initialConfig), *argument->readOnly())
                   .unit();
             }).thenError([](const folly::exception_wrapper& ex) {
               XLOGF(ERR, "Error: {}", ex.what());
               throw newEdenError(ex);
             }))
      .semi();
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_unmount(
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO, *mountPoint);
  return wrapImmediateFuture(
             std::move(helper),
             makeImmediateFutureWith([&]() mutable {
               auto mountPath = absolutePathFromThrift(*mountPoint);
               return server_->unmount(mountPath, UnmountOptions{});
             }).thenError([](const folly::exception_wrapper& ex) {
               throw newEdenError(ex);
             }))
      .semi();
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_unmountV2(
    std::unique_ptr<UnmountArgument> unmountArg) {
  auto helper =
      INSTRUMENT_THRIFT_CALL(INFO, *unmountArg->mountId()->mountPoint());
  return wrapImmediateFuture(
             std::move(helper),
             makeImmediateFutureWith([&]() mutable {
               auto mountPath =
                   absolutePathFromThrift(*unmountArg->mountId()->mountPoint());
               return server_->unmount(
                   mountPath, UnmountOptions{.force = *unmountArg->useForce()});
             }).thenError([](const folly::exception_wrapper& ex) {
               throw newEdenError(ex);
             }))
      .semi();
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  for (const auto& edenMount : server_->getAllMountPoints()) {
    MountInfo info;
    info.mountPoint() = absolutePathToThrift(edenMount->getPath());
    info.edenClientPath() = absolutePathToThrift(
        edenMount->getCheckoutConfig()->getClientDirectory());
    info.state() = edenMount->getState();
    info.backingRepoPath() = edenMount->getCheckoutConfig()->getRepoSource();
    results.push_back(info);
  }
}

folly::SemiFuture<std::unique_ptr<std::vector<CheckoutConflict>>>
EdenServiceHandler::semifuture_checkOutRevision(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash,
    CheckoutMode checkoutMode,
    std::unique_ptr<CheckOutRevisionParams> params) {
  auto rootIdOptions = params->rootIdOptions().ensure();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG1,
      *mountPoint,
      logHash(*hash),
      apache::thrift::util::enumName(checkoutMode, "(unknown)"),
      params->hgRootManifest().has_value() ? logHash(*params->hgRootManifest())
                                           : "(unspecified hg root manifest)",
      rootIdOptions.filterId().has_value() ? *rootIdOptions.filterId()
                                           : "no filter provided");
  helper->getThriftFetchContext().fillClientRequestInfo(params->cri());
  auto& fetchContext = helper->getFetchContext();

  auto mountHandle = lookupMount(mountPoint);

  // If we were passed a FilterID, create a RootID that contains the
  // filter and a varint that indicates the length of the original id.
  std::string parsedId =
      resolveRootId(std::move(*hash), rootIdOptions, mountHandle);
  hash.reset();

  auto mountPath = absolutePathFromThrift(*mountPoint);
  auto checkoutFuture = server_->checkOutRevision(
      mountPath,
      parsedId,
      params->hgRootManifest().to_optional(),
      fetchContext,
      helper->getFunctionName(),
      checkoutMode);

  return wrapImmediateFuture(
             std::move(helper),
             std::move(checkoutFuture).thenValue([](CheckoutResult&& result) {
               return std::make_unique<std::vector<CheckoutConflict>>(
                   std::move(result.conflicts));
             }))
      .semi();
}

folly::SemiFuture<folly::Unit>
EdenServiceHandler::semifuture_resetParentCommits(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<WorkingDirectoryParents> parents,
    std::unique_ptr<ResetParentCommitsParams> params) {
  auto rootIdOptions = params->rootIdOptions().ensure();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG1,
      *mountPoint,
      logHash(*parents->parent1()),
      params->hgRootManifest().has_value() ? logHash(*params->hgRootManifest())
                                           : "(unspecified hg root manifest)",
      rootIdOptions.filterId().has_value() ? *rootIdOptions.filterId()
                                           : "no filter provided");
  helper->getThriftFetchContext().fillClientRequestInfo(params->cri());

  auto mountHandle = lookupMount(mountPoint);

  // If we were passed a FilterID, create a RootID that contains the filter and
  // a varint that indicates the length of the original id.
  std::string parsedParent =
      resolveRootId(std::move(*parents->parent1()), rootIdOptions, mountHandle);
  auto parent1 = mountHandle.getObjectStore().parseRootId(parsedParent);

  auto fut = ImmediateFuture<folly::Unit>{std::in_place};
  if (params->hgRootManifest().has_value()) {
    auto& fetchContext = helper->getFetchContext();
    // The hg client has told us what the root manifest is.
    //
    // This is useful when a commit has just been created.  We won't be able to
    // ask the import helper to map the commit to its root manifest because it
    // won't know about the new commit until it reopens the repo.  Instead,
    // import the manifest for this commit directly.
    auto rootManifest = hash20FromThrift(*params->hgRootManifest());
    fut = mountHandle.getObjectStore().getBackingStore()->importManifestForRoot(
        parent1, rootManifest, fetchContext);
  }

  return wrapImmediateFuture(
             std::move(helper),
             std::move(fut).thenValue([parent1, mountHandle](folly::Unit) {
               mountHandle.getEdenMount().resetParent(parent1);
             }))
      .semi();
}

void EdenServiceHandler::getCurrentSnapshotInfo(
    GetCurrentSnapshotInfoResponse& out,
    std::unique_ptr<GetCurrentSnapshotInfoRequest> params) {
  const auto& mountId = params->mountId();
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountId);
  helper->getThriftFetchContext().fillClientRequestInfo(params->cri());

  auto mountHandle = lookupMount(*mountId);

  auto filterId =
      mountHandle.getEdenMount().getCheckoutConfig()->getLastActiveFilter();

  if (filterId.has_value()) {
    out.filterId() = std::move(filterId.value());
  }
}

namespace {
int64_t getSyncTimeout(const SyncBehavior& sync) {
  return sync.syncTimeoutSeconds().value_or(60);
}

/**
 * Wait for all the pending notifications to be processed.
 *
 * When the SyncBehavior is unset, this default to a timeout of 60 seconds. A
 * negative SyncBehavior mean to wait indefinitely.
 */
ImmediateFuture<folly::Unit> waitForPendingWrites(
    const EdenMount& mount,
    const SyncBehavior& sync) {
  auto seconds = getSyncTimeout(sync);
  if (seconds == 0) {
    return folly::unit;
  }

  auto future = mount.waitForPendingWrites().semi();
  if (seconds > 0) {
    future = std::move(future).within(std::chrono::seconds{seconds});
  }
  return std::move(future);
}
} // namespace

folly::SemiFuture<folly::Unit>
EdenServiceHandler::semifuture_synchronizeWorkingCopy(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<SynchronizeWorkingCopyParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*params->sync()));
  auto mountHandle = lookupMount(mountPoint);

  return wrapImmediateFuture(
             std::move(helper),
             waitForPendingWrites(mountHandle.getEdenMount(), *params->sync()))
      .ensure([mountHandle] {})
      .semi();
}

folly::SemiFuture<std::unique_ptr<std::vector<Blake3Result>>>
EdenServiceHandler::semifuture_getBlake3(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths,
    std::unique_ptr<SyncBehavior> sync) {
  TraceBlock block("getBlake3");
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), toLogArg(*paths));
  auto& fetchContext = helper->getFetchContext();
  auto mountHandle = lookupMount(mountPoint);

  auto notificationFuture =
      waitForPendingWrites(mountHandle.getEdenMount(), *sync);
  return wrapImmediateFuture(
             std::move(helper),
             std::move(notificationFuture)
                 .thenValue(
                     [mountHandle,
                      paths = std::move(paths),
                      fetchContext = fetchContext.copy()](auto&&) mutable {
                       return applyToVirtualInode(
                           mountHandle.getRootInode(),
                           *paths,
                           [mountHandle, fetchContext = fetchContext.copy()](
                               const VirtualInode& inode, RelativePath path) {
                             return inode
                                 .getBlake3(
                                     path,
                                     mountHandle.getObjectStorePtr(),
                                     fetchContext)
                                 .semi();
                           },
                           mountHandle.getObjectStorePtr(),
                           fetchContext);
                     })
                 .ensure([mountHandle] {})
                 .thenValue([](std::vector<folly::Try<Hash32>> results) {
                   auto out = std::make_unique<std::vector<Blake3Result>>();
                   out->reserve(results.size());

                   for (auto& result : results) {
                     auto& blake3Result = out->emplace_back();
                     if (result.hasValue()) {
                       blake3Result.blake3() = thriftHash32(result.value());
                     } else {
                       blake3Result.error() = newEdenError(result.exception());
                     }
                   }
                   return out;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<std::vector<DigestHashResult>>>
EdenServiceHandler::semifuture_getDigestHash(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths,
    std::unique_ptr<SyncBehavior> sync) {
  TraceBlock block("getDigestHash");
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), toLogArg(*paths));
  auto& fetchContext = helper->getFetchContext();
  auto mountHandle = lookupMount(mountPoint);

  auto notificationFuture =
      waitForPendingWrites(mountHandle.getEdenMount(), *sync);
  return wrapImmediateFuture(
             std::move(helper),
             std::move(notificationFuture)
                 .thenValue(
                     [mountHandle,
                      paths = std::move(paths),
                      fetchContext = fetchContext.copy()](auto&&) mutable {
                       return applyToVirtualInode(
                           mountHandle.getRootInode(),
                           *paths,
                           [mountHandle, fetchContext = fetchContext.copy()](
                               const VirtualInode& inode, RelativePath path) {
                             return inode
                                 .getDigestHash(
                                     path,
                                     mountHandle.getObjectStorePtr(),
                                     fetchContext)
                                 .semi();
                           },
                           mountHandle.getObjectStorePtr(),
                           fetchContext);
                     })
                 .ensure([mountHandle] {})
                 .thenValue([](std::vector<folly::Try<std::optional<Hash32>>>
                                   results) {
                   auto out = std::make_unique<std::vector<DigestHashResult>>();
                   out->reserve(results.size());

                   for (auto& result : results) {
                     auto& digestHashResult = out->emplace_back();
                     if (result.hasValue()) {
                       if (result.value().has_value()) {
                         digestHashResult.digestHash() =
                             thriftHash32(result.value().value());
                       } else {
                         digestHashResult.error() = newEdenError(
                             ENOENT,
                             EdenErrorType::ATTRIBUTE_UNAVAILABLE,
                             "tree aux data missing for tree");
                       }
                     } else {
                       digestHashResult.error() =
                           newEdenError(result.exception());
                     }
                   }
                   return out;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<std::vector<SHA1Result>>>
EdenServiceHandler::semifuture_getSHA1(
    std::unique_ptr<string> mountPoint,
    std::unique_ptr<vector<string>> paths,
    std::unique_ptr<SyncBehavior> sync) {
  TraceBlock block("getSHA1");
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), toLogArg(*paths));
  auto& fetchContext = helper->getFetchContext();
  auto mountHandle = lookupMount(mountPoint);

  auto notificationFuture =
      waitForPendingWrites(mountHandle.getEdenMount(), *sync);
  return wrapImmediateFuture(
             std::move(helper),
             std::move(notificationFuture)
                 .thenValue(
                     [mountHandle,
                      paths = std::move(paths),
                      fetchContext = fetchContext.copy()](auto&&) mutable {
                       return applyToVirtualInode(
                           mountHandle.getRootInode(),
                           *paths,
                           [mountHandle, fetchContext = fetchContext.copy()](
                               const VirtualInode& inode, RelativePath path) {
                             return inode
                                 .getSHA1(
                                     path,
                                     mountHandle.getObjectStorePtr(),
                                     fetchContext)
                                 .semi();
                           },
                           mountHandle.getObjectStorePtr(),
                           fetchContext);
                     })
                 .ensure([mountHandle] {})
                 .thenValue([](std::vector<folly::Try<Hash20>> results) {
                   auto out = std::make_unique<std::vector<SHA1Result>>();
                   out->reserve(results.size());

                   for (auto& result : results) {
                     auto& sha1Result = out->emplace_back();
                     if (result.hasValue()) {
                       sha1Result.sha1() = thriftHash20(result.value());
                     } else {
                       sha1Result.error() = newEdenError(result.exception());
                     }
                   }
                   return out;
                 }))
      .semi();
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_addBindMount(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> repoPathStr,
    std::unique_ptr<std::string> targetPath) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);

  auto repoPath = RelativePathPiece{*repoPathStr};
  auto absRepoPath = mountHandle.getEdenMount().getPath() + repoPath;
  auto* privHelper = server_->getServerState()->getPrivHelper();

  auto fut = mountHandle.getEdenMount().ensureDirectoryExists(
      repoPath, helper->getFetchContext());
  return std::move(fut)
      .thenValue([privHelper,
                  target = absolutePathFromThrift(*targetPath),
                  pathInMountDir = absRepoPath.copy()](TreeInodePtr) {
        return privHelper->bindMount(target.view(), pathInMountDir.view());
      })
      .ensure([mountHandle, helper = std::move(helper)] {})
      .semi();
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_removeBindMount(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> repoPathStr) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);

  auto repoPath = RelativePathPiece{*repoPathStr};
  auto absRepoPath = mountHandle.getEdenMount().getPath() + repoPath;
  return server_->getServerState()->getPrivHelper()->bindUnMount(
      absRepoPath.view());
}

void EdenServiceHandler::getCurrentJournalPosition(
    JournalPosition& out,
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  auto latest = mountHandle.getEdenMount().getJournal().getLatest();

  out.mountGeneration() = mountHandle.getEdenMount().getMountGeneration();
  if (latest) {
    out.sequenceNumber() = latest->sequenceID;
    out.snapshotHash() =
        mountHandle.getObjectStore().renderRootId(latest->toRoot);
  } else {
    out.sequenceNumber() = 0;
    out.snapshotHash() = mountHandle.getObjectStore().renderRootId(RootId{});
  }
}

apache::thrift::ServerStream<JournalPosition>
EdenServiceHandler::subscribeStreamTemporary(
    std::unique_ptr<std::string> mountPoint) {
  return streamJournalChanged(std::move(mountPoint));
}

apache::thrift::ServerStream<JournalPosition>
EdenServiceHandler::streamJournalChanged(
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);

  // We need a weak ref on the mount because the thrift stream plumbing
  // may outlive the mount point
  std::weak_ptr<EdenMount> weakMount(mountHandle.getEdenMountPtr());

  // We'll need to pass the subscriber id to both the disconnect
  // and change callbacks.  We can't know the id until after we've
  // created them both, so we need to share an optional id between them.
  auto handle = std::make_shared<std::optional<Journal::SubscriberId>>();
  auto disconnected = std::make_shared<std::atomic<bool>>(false);

  // This is called when the subscription channel is torn down
  auto onDisconnect = [weakMount, handle, disconnected] {
    XLOG(INFO, "streaming client disconnected");
    auto mount = weakMount.lock();
    if (mount) {
      disconnected->store(true);
      mount->getJournal().cancelSubscriber(handle->value());
    }
  };

  // Set up the actual publishing instance
  auto streamAndPublisher =
      apache::thrift::ServerStream<JournalPosition>::createPublisher(
          std::move(onDisconnect));

  // A little wrapper around the StreamPublisher.
  // This is needed because the destructor for StreamPublisherState
  // triggers a FATAL if the stream has not been completed.
  // We don't have an easy way to trigger this outside of just calling
  // it in a destructor, so that's what we do here.
  struct Publisher {
    apache::thrift::ServerStreamPublisher<JournalPosition> publisher;
    std::shared_ptr<std::atomic<bool>> disconnected;

    explicit Publisher(
        apache::thrift::ServerStreamPublisher<JournalPosition> publisher,
        std::shared_ptr<std::atomic<bool>> disconnected)
        : publisher(std::move(publisher)),
          disconnected(std::move(disconnected)) {}

    ~Publisher() {
      // We have to send an exception as part of the completion, otherwise
      // thrift doesn't seem to notify the peer of the shutdown
      if (!disconnected->load()) {
        std::move(publisher).complete(
            folly::make_exception_wrapper<std::runtime_error>(
                "subscriber terminated"));
      }
    }
  };

  auto stream = std::make_shared<Publisher>(
      std::move(streamAndPublisher.second), std::move(disconnected));

  // Register onJournalChange with the journal subsystem, and assign
  // the subscriber id into the handle so that the callbacks can consume it.
  handle->emplace(mountHandle.getEdenMount().getJournal().registerSubscriber(
      [stream = std::move(stream)]() mutable {
        JournalPosition pos;
        // The value is intentionally undefined and should not be used. Instead,
        // the subscriber should call getCurrentJournalPosition or
        // getFilesChangedSince.
        stream->publisher.next(pos);
      }));

  return std::move(streamAndPublisher.first);
}

namespace {
TraceEventTimes thriftTraceEventTimes(const TraceEventBase& event) {
  using namespace std::chrono;

  TraceEventTimes times;
  times.timestamp() =
      duration_cast<nanoseconds>(event.systemTime.time_since_epoch()).count();
  times.monotonic_time_ns() =
      duration_cast<nanoseconds>(event.monotonicTime.time_since_epoch())
          .count();
  return times;
}

RequestInfo thriftRequestInfo(pid_t pid, ProcessInfoCache& processInfoCache) {
  RequestInfo info;
  info.pid() = pid;
  info.processName().from_optional(processInfoCache.getProcessName(pid));
  return info;
}

template <typename T>
class ThriftStreamPublisherOwner {
 public:
  explicit ThriftStreamPublisherOwner(
      apache::thrift::ServerStreamPublisher<T> publisher)
      : owner(true), publisher{std::move(publisher)} {}

  ThriftStreamPublisherOwner(ThriftStreamPublisherOwner&& that) noexcept
      : owner{std::exchange(that.owner, false)},
        publisher{std::move(that.publisher)} {}

  ThriftStreamPublisherOwner& operator=(ThriftStreamPublisherOwner&&) = delete;

  void next(T payload) const {
    if (owner) {
      publisher.next(std::move(payload));
    }
  }

  void next(folly::exception_wrapper ew) && {
    if (owner) {
      owner = false;
      std::move(publisher).complete(std::move(ew));
    }
  }

  // Destroying a publisher without calling complete() aborts the process, so
  // ensure complete() is called when this object is dropped.
  ~ThriftStreamPublisherOwner() {
    if (owner) {
      std::move(publisher).complete();
    }
  }

 private:
  bool owner;
  apache::thrift::ServerStreamPublisher<T> publisher;
};

} // namespace

#ifndef _WIN32

namespace {
FuseCall populateFuseCall(
    uint64_t unique,
    const FuseTraceEvent::RequestHeader& request,
    ProcessInfoCache& processInfoCache) {
  FuseCall fc;
  fc.opcode() = request.opcode;
  fc.unique() = unique;
  fc.nodeid() = request.nodeid;
  fc.uid() = request.uid;
  fc.gid() = request.gid;
  fc.pid() = request.pid;

  fc.opcodeName() = fuseOpcodeName(request.opcode);
  fc.processName().from_optional(processInfoCache.getProcessName(request.pid));
  return fc;
}

NfsCall populateNfsCall(const NfsTraceEvent& event) {
  NfsCall nfsCall;
  nfsCall.xid() = event.getXid();
  nfsCall.procNumber() = event.getProcNumber();
  nfsCall.procName() = nfsProcName(event.getProcNumber());
  return nfsCall;
}

/**
 * Returns true if event should not be traced.
 */

bool isEventMasked(
    int64_t eventCategoryMask,
    ProcessAccessLog::AccessType accessType) {
  using AccessType = ProcessAccessLog::AccessType;
  switch (accessType) {
    case AccessType::FsChannelRead:
      return 0 == (eventCategoryMask & streamingeden_constants::FS_EVENT_READ_);
    case AccessType::FsChannelWrite:
      return 0 ==
          (eventCategoryMask & streamingeden_constants::FS_EVENT_WRITE_);
    case AccessType::FsChannelOther:
    default:
      return 0 ==
          (eventCategoryMask & streamingeden_constants::FS_EVENT_OTHER_);
  }
}

bool isEventMasked(int64_t eventCategoryMask, const FuseTraceEvent& event) {
  return isEventMasked(
      eventCategoryMask, fuseOpcodeAccessType(event.getRequest().opcode));
}

bool isEventMasked(int64_t eventCategoryMask, const NfsTraceEvent& event) {
  return isEventMasked(
      eventCategoryMask, nfsProcAccessType(event.getProcNumber()));
}

} // namespace

#endif //!_WIN32

#ifdef _WIN32
PrjfsCall populatePrjfsCall(
    const PrjfsTraceCallType callType,
    const PrjfsTraceEvent::PrjfsOperationData& data) {
  PrjfsCall prjfsCall;
  prjfsCall.callType_ref() = callType;
  prjfsCall.commandId_ref() = data.commandId;
  prjfsCall.pid_ref() = data.pid;
  return prjfsCall;
}

PrjfsCall populatePrjfsCall(const PrjfsTraceEvent& event) {
  return populatePrjfsCall(event.getCallType(), event.getData());
}
#endif

ThriftRequestMetadata populateThriftRequestMetadata(
    const ThriftRequestTraceEvent& request) {
  ThriftRequestMetadata thriftRequestMetadata;
  thriftRequestMetadata.requestId() = request.requestId;
  thriftRequestMetadata.method() = request.method;
  if (auto client_pid = request.clientPid) {
    thriftRequestMetadata.clientPid() = client_pid.value().get();
  }
  return thriftRequestMetadata;
}

/**
 * Helper function to convert a ThriftRequestTraceEvent to a ThriftRequestEvent
 * type. Used in EdenServiceHandler::traceThriftRequestEvents and
 * EdenServiceHandler::getRetroactiveThriftRequestEvents.
 */
void convertThriftRequestTraceEventToThriftRequestEvent(
    const ThriftRequestTraceEvent& event,
    ThriftRequestEvent& te) {
  te.times() = thriftTraceEventTimes(event);
  switch (event.type) {
    case ThriftRequestTraceEvent::START:
      te.eventType() = ThriftRequestEventType::START;
      break;
    case ThriftRequestTraceEvent::FINISH:
      te.eventType() = ThriftRequestEventType::FINISH;
      break;
  }
  te.requestMetadata() = populateThriftRequestMetadata(event);
}

apache::thrift::ServerStream<ThriftRequestEvent>
EdenServiceHandler::traceThriftRequestEvents() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  struct SubscriptionHandleOwner {
    TraceBus<ThriftRequestTraceEvent>::SubscriptionHandle handle;
  };

  auto h = std::make_shared<SubscriptionHandleOwner>();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<ThriftRequestEvent>::createPublisher([h] {
        // on disconnect, release subscription handle
      });

  h->handle = thriftRequestTraceBus_->subscribeFunction(
      "Live Thrift request tracing",
      [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)}](
          const ThriftRequestTraceEvent& event) mutable {
        ThriftRequestEvent thriftEvent;
        convertThriftRequestTraceEventToThriftRequestEvent(event, thriftEvent);
        publisher_2.next(thriftEvent);
      });

  return std::move(serverStream);
}

apache::thrift::ServerStream<TaskEvent> EdenServiceHandler::traceTaskEvents(
    std::unique_ptr<::facebook::eden::TraceTaskEventsRequest> /* request */) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  struct SubscriptionHandleOwner {
    TraceBus<TaskTraceEvent>::SubscriptionHandle handle;
  };

  auto h = std::make_shared<SubscriptionHandleOwner>();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<TaskEvent>::createPublisher([h] {
        // on disconnect, release subscription handle
      });

  h->handle = TaskTraceEvent::getTraceBus()->subscribeFunction(
      "Live Thrift request tracing",
      [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)}](
          const TaskTraceEvent& event) mutable {
        TaskEvent taskEvent;
        taskEvent.times() = thriftTraceEventTimes(event);
        taskEvent.name() = event.name;
        taskEvent.threadName() = event.threadName;
        taskEvent.threadId() = event.threadId;
        taskEvent.duration() = event.duration.count();
        taskEvent.start() = event.start.count();
        publisher_2.next(taskEvent);
      });

  return std::move(serverStream);
}

apache::thrift::ServerStream<FsEvent> EdenServiceHandler::traceFsEvents(
    std::unique_ptr<std::string> mountPoint,
    int64_t eventCategoryMask) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  auto& edenMount = mountHandle.getEdenMount();

  // Treat an empty bitset as an unfiltered stream. This is for clients that
  // predate the addition of the mask and for clients that don't care.
  // 0 would be meaningless anyway: it would never return any events.
  if (0 == eventCategoryMask) {
    eventCategoryMask = ~0;
  }

  struct Context {
    // While subscribed to FuseChannel's TraceBus, request detailed argument
    // strings.
    TraceDetailedArgumentsHandle argHandle;
#ifdef _WIN32
    TraceSubscriptionHandle<PrjfsTraceEvent> subHandle;
#else
    std::variant<
        TraceSubscriptionHandle<FuseTraceEvent>,
        TraceSubscriptionHandle<NfsTraceEvent>>
        subHandle;
#endif // _WIN32
  };

  auto context = std::make_shared<Context>();
#ifdef _WIN32
  auto prjfsChannel = edenMount.getPrjfsChannel()->getInner();
  if (prjfsChannel) {
    context->argHandle = prjfsChannel->traceDetailedArguments();
  } else {
    EDEN_BUG() << "tracing isn't supported yet for the "
               << fmt::underlying(
                      edenMount.getCheckoutConfig()->getMountProtocol())
               << " filesystem type";
  }
#else
  auto* fuseChannel = edenMount.getFuseChannel();
  auto* nfsdChannel = edenMount.getNfsdChannel();
  if (fuseChannel) {
    context->argHandle = fuseChannel->traceDetailedArguments();
  } else if (nfsdChannel) {
    context->argHandle = nfsdChannel->traceDetailedArguments();
  } else {
    EDEN_BUG() << "tracing isn't supported yet for the "
               << fmt::underlying(
                      edenMount.getCheckoutConfig()->getMountProtocol())
               << " filesystem type";
  }
#endif // _WIN32

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<FsEvent>::createPublisher([context] {
        // on disconnect, release context and the TraceSubscriptionHandle
      });

#ifdef _WIN32
  if (prjfsChannel) {
    context->subHandle = prjfsChannel->getTraceBusPtr()->subscribeFunction(
        fmt::format("strace-{}", edenMount.getPath().basename()),
        [publisher = ThriftStreamPublisherOwner{std::move(publisher)}](
            const PrjfsTraceEvent& event) {
          FsEvent te;
          auto times = thriftTraceEventTimes(event);
          te.times_ref() = times;

          // Legacy timestamp fields.
          te.timestamp_ref() = *times.timestamp_ref();
          te.monotonic_time_ns_ref() = *times.monotonic_time_ns_ref();

          te.prjfsRequest_ref() = populatePrjfsCall(event);

          switch (event.getType()) {
            case PrjfsTraceEvent::START:
              te.type_ref() = FsEventType::START;
              if (auto& arguments = event.getArguments()) {
                te.arguments_ref() = *arguments;
              }
              break;
            case PrjfsTraceEvent::FINISH:
              te.type_ref() = FsEventType::FINISH;
              break;
          }

          te.requestInfo_ref() = RequestInfo{};

          publisher.next(te);
        });
  }
#else
  if (fuseChannel) {
    context->subHandle = fuseChannel->getTraceBus().subscribeFunction(
        fmt::format("strace-{}", edenMount.getPath().basename()),
        [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)},
         serverState = server_->getServerState(),
         eventCategoryMask](const FuseTraceEvent& event) {
          if (isEventMasked(eventCategoryMask, event)) {
            return;
          }

          FsEvent te;
          auto times = thriftTraceEventTimes(event);
          te.times() = times;

          // Legacy timestamp fields.
          te.timestamp() = *times.timestamp();
          te.monotonic_time_ns() = *times.monotonic_time_ns();

          te.fuseRequest() = populateFuseCall(
              event.getUnique(),
              event.getRequest(),
              *serverState->getProcessInfoCache());

          switch (event.getType()) {
            case FuseTraceEvent::START:
              te.type() = FsEventType::START;
              if (auto& arguments = event.getArguments()) {
                te.arguments() = *arguments;
              }
              break;
            case FuseTraceEvent::FINISH:
              te.type() = FsEventType::FINISH;
              te.result().from_optional(event.getResponseCode());
              break;
          }

          te.requestInfo() = thriftRequestInfo(
              event.getRequest().pid, *serverState->getProcessInfoCache());

          publisher_2.next(te);
        });
  } else if (nfsdChannel) {
    context->subHandle = nfsdChannel->getTraceBus().subscribeFunction(
        fmt::format("strace-{}", edenMount.getPath().basename()),
        [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)},
         eventCategoryMask](const NfsTraceEvent& event) {
          if (isEventMasked(eventCategoryMask, event)) {
            return;
          }

          FsEvent te;
          auto times = thriftTraceEventTimes(event);
          te.times() = times;

          // Legacy timestamp fields.
          te.timestamp() = *times.timestamp();
          te.monotonic_time_ns() = *times.monotonic_time_ns();

          te.nfsRequest() = populateNfsCall(event);

          switch (event.getType()) {
            case NfsTraceEvent::START:
              te.type() = FsEventType::START;
              if (auto arguments = event.getArguments()) {
                te.arguments() = arguments.value();
              }
              break;
            case NfsTraceEvent::FINISH:
              te.type() = FsEventType::FINISH;
              break;
          }

          te.requestInfo() = RequestInfo{};

          publisher_2.next(te);
        });
  }
#endif // _WIN32
  return std::move(serverStream);
}

/**
 * Helper function to get a cast a BackingStore shared_ptr to a
 * SaplingBackingStore shared_ptr. Returns an error if the type of backingStore
 * provided is not truly an SaplingBackingStore. Used in
 * EdenServiceHandler::traceHgEvents,
 * EdenServiceHandler::getRetroactiveHgEvents and
 * EdenServiceHandler::debugOutstandingHgEvents.
 */
std::shared_ptr<SaplingBackingStore> castToSaplingBackingStore(
    std::shared_ptr<BackingStore>& backingStore,
    AbsolutePathPiece mountPath) {
  std::shared_ptr<SaplingBackingStore> saplingBackingStore{nullptr};

  // If FilteredFS is enabled, we'll see a FilteredBackingStore first
  auto filteredBackingStore =
      std::dynamic_pointer_cast<FilteredBackingStore>(backingStore);
  if (filteredBackingStore) {
    // FilteredBackingStore -> SaplingBackingStore
    saplingBackingStore = std::dynamic_pointer_cast<SaplingBackingStore>(
        filteredBackingStore->getBackingStore());
  } else {
    // BackingStore -> SaplingBackingStore
    saplingBackingStore =
        std::dynamic_pointer_cast<SaplingBackingStore>(backingStore);
  }

  if (!saplingBackingStore) {
    // typeid() does not evaluate expressions
    auto& r = *backingStore.get();
    throw newEdenError(
        EdenErrorType::GENERIC_ERROR,
        fmt::format(
            "mount {} must use SaplingBackingStore, type is {}",
            mountPath,
            typeid(r).name()));
  }

  return saplingBackingStore;
}

/**
 * Helper function to convert an HgImportTraceEvent to a thrift HgEvent type.
 * Used in EdenServiceHandler::traceHgEvents,
 * EdenServiceHandler::getRetroactiveHgEvents and
 * EdenServiceHandler::debugOutstandingHgEvents.
 */
void convertHgImportTraceEventToHgEvent(
    const HgImportTraceEvent& event,
    ProcessInfoCache& processInfoCache,
    HgEvent& te) {
  te.times() = thriftTraceEventTimes(event);
  switch (event.eventType) {
    case HgImportTraceEvent::QUEUE:
      te.eventType() = HgEventType::QUEUE;
      break;
    case HgImportTraceEvent::START:
      te.eventType() = HgEventType::START;
      break;
    case HgImportTraceEvent::FINISH:
      te.eventType() = HgEventType::FINISH;
      break;
  }

  switch (event.resourceType) {
    case HgImportTraceEvent::BLOB:
      te.resourceType() = HgResourceType::BLOB;
      break;
    case HgImportTraceEvent::TREE:
      te.resourceType() = HgResourceType::TREE;
      break;
    case HgImportTraceEvent::BLOB_AUX:
      te.resourceType() = HgResourceType::BLOBMETA;
      break;
    case HgImportTraceEvent::TREE_AUX:
      te.resourceType() = HgResourceType::TREEMETA;
      break;
  }

  switch (event.importPriority) {
    case ImportPriority::Class::Low:
      te.importPriority() = HgImportPriority::LOW;
      break;
    case ImportPriority::Class::Normal:
      te.importPriority() = HgImportPriority::NORMAL;
      break;
    case ImportPriority::Class::High:
      te.importPriority() = HgImportPriority::HIGH;
      break;
  }

  switch (event.importCause) {
    case ObjectFetchContext::Cause::Unknown:
      te.importCause() = HgImportCause::UNKNOWN;
      break;
    case ObjectFetchContext::Cause::Fs:
      te.importCause() = HgImportCause::FS;
      break;
    case ObjectFetchContext::Cause::Thrift:
      te.importCause() = HgImportCause::THRIFT;
      break;
    case ObjectFetchContext::Cause::Prefetch:
      te.importCause() = HgImportCause::PREFETCH;
      break;
  }

  if (event.fetchedSource.has_value()) {
    switch (event.fetchedSource.value()) {
      case ObjectFetchContext::FetchedSource::Local:
        te.fetchedSource() = FetchedSource::LOCAL;
        break;
      case ObjectFetchContext::FetchedSource::Remote:
        te.fetchedSource() = FetchedSource::REMOTE;
        break;
      case ObjectFetchContext::FetchedSource::Unknown:
        te.fetchedSource() = FetchedSource::UNKNOWN;
        break;
    }
  } else {
    te.fetchedSource() = FetchedSource::NOT_AVAILABLE_YET;
  }

  te.unique() = event.unique;

  te.manifestNodeId() = event.manifestNodeId.toString();
  te.path() = event.getPath();

  if (auto pid = event.pid) {
    te.requestInfo() = thriftRequestInfo(pid.value().get(), processInfoCache);
  }
}

apache::thrift::ServerStream<HgEvent> EdenServiceHandler::traceHgEvents(
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  auto backingStore = mountHandle.getObjectStore().getBackingStore();
  std::shared_ptr<SaplingBackingStore> saplingBackingStore =
      castToSaplingBackingStore(
          backingStore, mountHandle.getEdenMount().getPath());

  struct Context {
    TraceSubscriptionHandle<HgImportTraceEvent> subHandle;
  };

  auto context = std::make_shared<Context>();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<HgEvent>::createPublisher([context] {
        // on disconnect, release context and the TraceSubscriptionHandle
      });

  context->subHandle = saplingBackingStore->getTraceBus().subscribeFunction(
      fmt::format(
          "hgtrace-{}", mountHandle.getEdenMount().getPath().basename()),
      [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)},
       processInfoCache =
           mountHandle.getEdenMount().getServerState()->getProcessInfoCache()](
          const HgImportTraceEvent& event) {
        HgEvent thriftEvent;
        convertHgImportTraceEventToHgEvent(
            event, *processInfoCache, thriftEvent);
        publisher_2.next(thriftEvent);
      });

  return std::move(serverStream);
}

/**
 * Helper function to convert an InodeTraceEvent to a thrift InodeEvent type.
 * Used in EdenServiceHandler::traceInodeEvents and
 * EdenServiceHandler::getRetroactiveInodeEvents. Note paths are not set here
 * and are set by the calling functions. For traceInodeEvents full paths may
 * need to be computed whereas for getRetroactiveInodeEvents full paths would
 * have already been computed when the event gets added to the ActivityBuffer.
 */
void ConvertInodeTraceEventToThriftInodeEvent(
    InodeTraceEvent traceEvent,
    InodeEvent& thriftEvent) {
  thriftEvent.times() = thriftTraceEventTimes(traceEvent);
  thriftEvent.ino() = traceEvent.ino.getRawValue();
  thriftEvent.inodeType() = traceEvent.inodeType;
  thriftEvent.eventType() = traceEvent.eventType;
  thriftEvent.progress() = traceEvent.progress;
  thriftEvent.duration() = traceEvent.duration.count();
  // TODO: trace requesting pid
  // thriftEvent.requestInfo() = thriftRequestInfo(pid);
}

apache::thrift::ServerStream<InodeEvent> EdenServiceHandler::traceInodeEvents(
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  auto inodeMap = mountHandle.getEdenMount().getInodeMap();

  struct Context {
    TraceSubscriptionHandle<InodeTraceEvent> subHandle;
  };

  auto context = std::make_shared<Context>();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<InodeEvent>::createPublisher([context] {
        // on disconnect, release context and the TraceSubscriptionHandle
      });

  context->subHandle =
      mountHandle.getEdenMount().getInodeTraceBus().subscribeFunction(
          fmt::format(
              "inodetrace-{}", mountHandle.getEdenMount().getPath().basename()),
          [publisher_2 = ThriftStreamPublisherOwner{std::move(publisher)},
           inodeMap](const InodeTraceEvent& event) {
            InodeEvent thriftEvent;
            ConvertInodeTraceEventToThriftInodeEvent(event, thriftEvent);
            try {
              auto relativePath = inodeMap->getPathForInode(event.ino);
              thriftEvent.path() =
                  relativePath ? relativePath->asString() : event.getPath();
            } catch (const std::system_error& /* e */) {
              thriftEvent.path() = event.getPath();
            }
            publisher_2.next(thriftEvent);
          });

  return std::move(serverStream);
}

namespace {
void checkMountGeneration(
    const JournalPosition& position,
    const EdenMount& mount,
    std::string_view fieldName) {
  if (folly::to_unsigned(*position.mountGeneration()) !=
      mount.getMountGeneration()) {
    throw newEdenError(
        ERANGE,
        EdenErrorType::MOUNT_GENERATION_CHANGED,
        fieldName,
        ".mountGeneration does not match the current "
        "mountGeneration.  "
        "You need to compute a new basis for delta queries.");
  }
}

void publishFile(
    const folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>&
        publisher,
    folly::StringPiece path,
    ScmFileStatus status,
    dtype_t type) {
  ChangedFileResult fileResult;
  fileResult.name() = path.str();
  fileResult.status() = status;
  fileResult.dtype() = static_cast<Dtype>(type);
  publisher.rlock()->next(std::move(fileResult));
}

/**
 * This method computes all uncommitted changes and save the result to publisher
 */
void sumUncommittedChanges(
    const JournalDeltaRange& range,
    const folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>&
        publisher,
    std::optional<std::reference_wrapper<GlobFilter>> filter) {
  for (auto& entry : range.changedFilesInOverlay) {
    const auto& changeInfo = entry.second;

    // the path is filtered don't consider it
    if (filter) {
      // TODO(T167750650): This .get() will block Thrift threads and could lead
      // to Queue Timeouts. Instead of calling .get(), we should chain futures
      // together.
      if (filter->get()
              .getFilterCoverageForPath(entry.first, folly::StringPiece(""))
              .get() == FilterCoverage::RECURSIVELY_FILTERED) {
        continue;
      }
    }

    ScmFileStatus status;
    if (!changeInfo.existedBefore && changeInfo.existedAfter) {
      status = ScmFileStatus::ADDED;
    } else if (changeInfo.existedBefore && !changeInfo.existedAfter) {
      status = ScmFileStatus::REMOVED;
    } else {
      status = ScmFileStatus::MODIFIED;
    }

    publishFile(publisher, entry.first.asString(), status, dtype_t::Unknown);
  }

  for (const auto& name : range.uncleanPaths) {
    if (filter) {
      // TODO(T167750650): This .get() will block Thrift threads and could lead
      // to Queue Timeouts. Instead of calling .get(), we should chain futures
      // together.
      if (filter->get()
              .getFilterCoverageForPath(name, folly::StringPiece(""))
              .get() == FilterCoverage::RECURSIVELY_FILTERED) {
        continue;
      }
    }
    publishFile(
        publisher, name.asString(), ScmFileStatus::MODIFIED, dtype_t::Unknown);
  }
}

class StreamingDiffCallback : public DiffCallback {
 public:
  explicit StreamingDiffCallback(
      std::shared_ptr<
          folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>>
          publisher)
      : publisher_{std::move(publisher)} {}

  void ignoredPath(RelativePathPiece, dtype_t) override {}

  void addedPath(RelativePathPiece path, dtype_t type) override {
    publishFile(*publisher_, path.view(), ScmFileStatus::ADDED, type);
  }

  void removedPath(RelativePathPiece path, dtype_t type) override {
    publishFile(*publisher_, path.view(), ScmFileStatus::REMOVED, type);
  }

  void modifiedPath(RelativePathPiece path, dtype_t type) override {
    publishFile(*publisher_, path.view(), ScmFileStatus::MODIFIED, type);
  }

  void diffError(RelativePathPiece /*path*/, const folly::exception_wrapper& ew)
      override {
    auto publisher = std::move(*publisher_->wlock());
    std::move(publisher).next(newEdenError(ew));
  }

 private:
  std::shared_ptr<
      folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>>
      publisher_;
};

/**
 * Compute the difference between the passed in roots.
 *
 * The order of the roots matters: a file added in toRoot will be returned as
 * ScmFileStatus::ADDED, while if the order of arguments were reversed, it
 * would be returned as ScmFileStatus::REMOVED.
 */
ImmediateFuture<folly::Unit> diffBetweenRoots(
    const RootId& fromRoot,
    const RootId& toRoot,
    const CheckoutConfig& checkoutConfig,
    const std::shared_ptr<ObjectStore>& objectStore,
    folly::CancellationToken cancellation,
    const ObjectFetchContextPtr& fetchContext,
    DiffCallback* callback) {
  auto diffContext = std::make_unique<DiffContext>(
      callback,
      cancellation,
      fetchContext,
      true,
      checkoutConfig.getCaseSensitive(),
      checkoutConfig.getEnableWindowsSymlinks(),
      objectStore,
      nullptr);
  auto fut = diffRoots(diffContext.get(), fromRoot, toRoot);
  return std::move(fut).ensure([diffContext = std::move(diffContext)] {});
}

} // namespace

apache::thrift::ResponseAndServerStream<ChangesSinceResult, ChangedFileResult>
EdenServiceHandler::streamChangesSince(
    std::unique_ptr<StreamChangesSinceParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL_WITH_STAT(
      DBG3, &ThriftStats::streamChangesSince, *params->mountPoint());
  auto mountHandle = lookupMount(params->mountPoint());
  const auto& fromPosition = *params->fromPosition();
  auto& fetchContext = helper->getFetchContext();

  // Streaming in Thrift can be done via a Stream Generator, or via a Stream
  // Publisher. We're using the latter here as the former can only be used with
  // coroutines which EdenFS hasn't been converted to. Generators also have the
  // property to be driven by the client, internally, Thrift will wait for the
  // client to have consumed an element before requesting more from the server.
  // Publishers on the other hand are driven by the server and are publishing
  // as fast as possible.
  //
  // What this means is that in the case where EdenFS can publish elements
  // faster than the client can read them, EdenFS's memory usage can grow
  // potentially unbounded.

  checkMountGeneration(
      fromPosition, mountHandle.getEdenMount(), "fromPosition"sv);

  // The +1 is because the core merge stops at the item prior to
  // its limitSequence parameter and we want the changes *since*
  // the provided sequence number.
  auto summed = mountHandle.getJournal().accumulateRange(
      *fromPosition.sequenceNumber() + 1);

  ChangesSinceResult result;
  if (!summed) {
    // No changes, just return the fromPosition and an empty stream.
    result.toPosition() = fromPosition;

    return {
        std::move(result),
        apache::thrift::ServerStream<ChangedFileResult>::createEmpty()};
  }

  if (summed->isTruncated) {
    throw newEdenError(
        EDOM,
        EdenErrorType::JOURNAL_TRUNCATED,
        "Journal entry range has been truncated.");
  }

  auto cancellationSource = std::make_shared<folly::CancellationSource>();
  auto [serverStream, publisher] =
      apache::thrift::ServerStream<ChangedFileResult>::createPublisher(
          [cancellationSource] { cancellationSource->requestCancellation(); });
  auto sharedPublisherLock = std::make_shared<
      folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>>(
      ThriftStreamPublisherOwner{std::move(publisher)});

  RootIdCodec& rootIdCodec = mountHandle.getObjectStore();

  JournalPosition toPosition;
  toPosition.mountGeneration() =
      mountHandle.getEdenMount().getMountGeneration();
  toPosition.sequenceNumber() = summed->toSequence;
  toPosition.snapshotHash() =
      rootIdCodec.renderRootId(summed->snapshotTransitions.back());
  result.toPosition() = toPosition;

  sumUncommittedChanges(*summed, *sharedPublisherLock, std::nullopt);

  if (summed->snapshotTransitions.size() > 1) {
    auto callback =
        std::make_shared<StreamingDiffCallback>(sharedPublisherLock);

    std::vector<ImmediateFuture<folly::Unit>> futures;
    for (auto rootIt = summed->snapshotTransitions.begin();
         std::next(rootIt) != summed->snapshotTransitions.end();
         ++rootIt) {
      const auto& from = *rootIt;
      const auto& to = *(rootIt + 1);

      // We want to make sure the diff is performed on a background thread so
      // the Thrift client can interrupt us whenever desired. To do this, let's
      // start from an not ready ImmediateFuture.
      futures.push_back(makeNotReadyImmediateFuture().thenValue(
          [from,
           to,
           mountHandle,
           token = cancellationSource->getToken(),
           fetchContext = fetchContext.copy(),
           callback = callback.get()](auto&&) {
            return diffBetweenRoots(
                from,
                to,
                *mountHandle.getEdenMount().getCheckoutConfig(),
                mountHandle.getObjectStorePtr(),
                token,
                fetchContext,
                callback);
          }));
    }

    folly::futures::detachOn(
        server_->getServerState()->getThreadPool().get(),
        collectAllSafe(std::move(futures))
            // Make sure that the edenMount, callback, helper and
            // cancellationSource lives for the duration of the stream by
            // copying them.
            .thenTry(
                [mountHandle,
                 sharedPublisherLock,
                 callback = std::move(callback),
                 helper = std::move(helper),
                 cancellationSource](
                    folly::Try<std::vector<folly::Unit>>&& result) mutable {
                  if (result.hasException()) {
                    auto sharedPublisher =
                        std::move(*sharedPublisherLock->wlock());
                    std::move(sharedPublisher)
                        .next(newEdenError(std::move(result).exception()));
                  }
                })
            .semi());
  }

  return {std::move(result), std::move(serverStream)};
}

std::pair<std::vector<RelativePath>, std::vector<RelativePath>>
buildIncludedAndExcludedRoots(
    bool includeVCSRoots,
    bool includeStateChanges,
    const std::vector<RelativePath>& vcsDirectories,
    const std::vector<PathString>& includedRoots,
    const std::vector<PathString>& excludedRoots,
    const RelativePathPiece& root,
    const RelativePathPiece& notificationsStateDirectory) {
  // This uses/returns RelativePath instead of RelativePathPiece due to the
  // value constructed with the root + include/excludeRoot going out of
  // scope
  std::vector<RelativePath> outIncludedRoots;
  outIncludedRoots.reserve(includedRoots.size());
  // If there are includedRoots, append them to the root if there
  // is one, otherwise just fill out the vector with the values
  if (includedRoots.size() > 0) {
    std::transform(
        includedRoots.begin(),
        includedRoots.end(),
        std::back_inserter(outIncludedRoots),
        [root](const PathString& includedRoot) {
          return root + relpathPieceFromUserPath(includedRoot);
        });
  } else if (!root.empty()) {
    // If there are no includedRoots and there is a root, use
    // it as an includedRoot
    outIncludedRoots.emplace_back(root);
  }
  if (includeStateChanges) {
    outIncludedRoots.emplace_back(notificationsStateDirectory);
  }

  std::vector<RelativePath> outExcludedRoots(excludedRoots.size());
  std::transform(
      excludedRoots.begin(),
      excludedRoots.end(),
      outExcludedRoots.begin(),
      [root](const PathString& excludedRoot) {
        return root + relpathPieceFromUserPath(excludedRoot);
      });

  if (includeVCSRoots) {
    outIncludedRoots.insert(
        outIncludedRoots.end(), vcsDirectories.begin(), vcsDirectories.end());
  } else {
    outExcludedRoots.insert(
        outExcludedRoots.end(), vcsDirectories.begin(), vcsDirectories.end());
  }

  return std::make_pair(
      std::move(outIncludedRoots), std::move(outExcludedRoots));
}

/*
 * Determines if a given path should be returned based on the includedRoots and
 * excludedRoots provided by the caller of changesSinceV2.
 *
 */
bool isPathIncluded(
    const std::vector<RelativePath>& includedRoots,
    const std::vector<RelativePath>& excludedRoots,
    const std::vector<std::string>& includedSuffixes,
    const std::vector<std::string>& excludedSuffixes,
    RelativePath path) {
  if (!includedRoots.empty()) {
    bool included = false;
    // test to see if path matches includedRoots - include the path
    for (const auto& includedRoot : includedRoots) {
      if (includedRoot == path || includedRoot.isParentDirOf(path)) {
        included = true;
        break;
      }
    }

    // includedRoots not empty and no match - do not include the path
    if (!included) {
      return false;
    }
  }

  if (!includedSuffixes.empty()) {
    bool included = false;
    // test to see if path matches includedSuffixes - include the path
    for (const auto& includedSuffix : includedSuffixes) {
      if (ends_with(path.asString(), includedSuffix)) {
        included = true;
        break;
      }
    }

    // includedSuffixes not empty and no match - do not include the path
    if (!included) {
      return false;
    }
  }

  // if exclude filter is not empty
  if (!excludedRoots.empty()) {
    // test to see if path matches excludedRoots - exclude the path
    for (const auto& excludedRoot : excludedRoots) {
      if (excludedRoot == path || excludedRoot.isParentDirOf(path)) {
        return false;
      }
    }
  }

  if (!excludedSuffixes.empty()) {
    for (const auto& excludedSuffix : excludedSuffixes) {
      if (ends_with(path.asString(), excludedSuffix)) {
        return false;
      }
    }
  }

  // Path should be included
  return true;
}

void EdenServiceHandler::sync_changesSinceV2(
    ChangesSinceV2Result& result,
    std::unique_ptr<ChangesSinceV2Params> params) {
  uint64_t numSmallChanges = 0;
  uint64_t numStateChanges = 0;
  uint64_t numRenamedDirectory = 0;
  uint64_t numCommitTransition = 0;
  std::optional<uint64_t> lostChangesReason;
  uint64_t numFilteredResults = 0;

  auto mountHandle = lookupMount(params->mountPoint());
  const auto& fromPosition = *params->fromPosition();
  RelativePathPiece root = params->root().has_value()
      ? RelativePathPiece{params->root().value()}
      : RelativePathPiece{};

  auto includedRoots = params->includedRoots().has_value()
      ? params->includedRoots().value()
      : std::vector<PathString>{};
  auto excludedRoots = params->excludedRoots().has_value()
      ? params->excludedRoots().value()
      : std::vector<PathString>{};

  auto includedSuffixes = params->includedSuffixes().has_value()
      ? params->includedSuffixes().value()
      : std::vector<std::string>{};
  auto excludedSuffixes = params->excludedSuffixes().has_value()
      ? params->excludedSuffixes().value()
      : std::vector<std::string>{};

  auto includeStateChanges = params->includeStateChanges().has_value()
      ? params->includeStateChanges().value()
      : false;

  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint(),
      fmt::format(
          "fromPosition={}, root:{}, includedRoots:{}, excludedRoots:{}, includedSuffixes:{}, excludedSuffixes:{}, includeStateChanges: {}",
          logPosition(fromPosition),
          root,
          toLogArg(includedRoots),
          toLogArg(excludedRoots),
          toLogArg(includedSuffixes),
          toLogArg(excludedSuffixes),
          includeStateChanges));

  auto latestJournalEntry = mountHandle.getJournal().getLatest();
  std::optional<JournalDelta::SequenceNumber> toSequence;
  RootId toSnapshotId = RootId{};
  if (latestJournalEntry.has_value()) {
    toSequence = latestJournalEntry->sequenceID;
    toSnapshotId = latestJournalEntry->toRoot;
  }
  RootId currentRoot = toSnapshotId;
  RootIdCodec& rootIdCodec = mountHandle.getObjectStore();

  auto& fetchContext = helper->getFetchContext();

  auto& mount = mountHandle.getEdenMount();
  auto& mountPath = mount.getPath();
  if (!root.empty()) {
    bool rootExists =
        waitForPendingWrites(mount, *params->sync())
            .thenValue(
                [&mount, root, fetchContext = fetchContext.copy()](auto&&) {
                  return mount.getVirtualInode(root, fetchContext)
                      .thenTry([](folly::Try<VirtualInode> tree) mutable {
                        if (tree.hasException()) {
                          // Root does not exist, or something else went wrong
                          return false;
                        }
                        // Files are not valid roots
                        return tree.value().isDirectory();
                      });
                })
            .get();
    if (!rootExists) {
      throw newEdenError(
          EINVAL,
          EdenErrorType::ARGUMENT_ERROR,
          fmt::format("Invalid root path \"{}\" in mount {}", root, mountPath));
    }
  }

  bool includeVCSRoots = params->includeVCSRoots().has_value()
      ? params->includeVCSRoots().value()
      : false;
  // Has EdenFS restarted or remounted
  if (folly::to_unsigned(*fromPosition.mountGeneration()) !=
      mountHandle.getEdenMount().getMountGeneration()) {
    if (!toSequence.has_value()) {
      // If there is no journal entry after restart/remount it should be
      // OK to use the initial SequenceNumber - 1.
      toSequence = 1;
    }
    LostChanges lostChanges;
    lostChanges.reason() = LostChangesReason::EDENFS_REMOUNTED;

    LargeChangeNotification largeChange;
    largeChange.lostChanges() = std::move(lostChanges);

    ChangeNotification change;
    change.largeChange() = std::move(largeChange);
    lostChangesReason =
        static_cast<uint64_t>(LostChangesReason::EDENFS_REMOUNTED);

    result.changes()->push_back(std::move(change));
  } else {
    // TODO: move to helper
    auto config = server_->getServerState()->getEdenConfig();
    auto maxNumberOfChanges = config->notifyMaxNumberOfChanges.getValue();
    auto notificationsStateDirectory =
        config->notificationsStateDirectory.getValue();
    auto includedAndExcludedRoots = buildIncludedAndExcludedRoots(
        includeVCSRoots,
        includeStateChanges,
        config->vcsDirectories.getValue(),
        includedRoots,
        excludedRoots,
        root,
        notificationsStateDirectory);
    auto includedAndExcludedSuffixes =
        std::make_pair(includedSuffixes, excludedSuffixes);

    const auto isTruncated = mountHandle.getJournal().forEachDelta(
        *fromPosition.sequenceNumber() + 1,
        std::nullopt,
        [&](const FileChangeJournalDelta& current) -> bool {
          if (!current.isPath1Valid) {
            XLOG(
                DFATAL,
                "FileChangeJournalDetal::isPath1Valid should never be false");
          }

          // Check if it's a notifications state event.
          // Changes that happen inside the notifications state directory
          // will not be reported with other changes.
          if (notificationsStateDirectory.isParentDirOf(current.path1)) {
            XLOGF(
                DBG3,
                "Eden notifications file event at path {}",
                current.path1.asString());
            const auto& info = current.info1;
            ChangeNotification change;
            StateChangeNotification stateChange;
            if (ends_with(current.path1.asString(), ".notify")) {
              if (!info.existedBefore) {
                StateEntered stateEntered;
                XLOGF(
                    DBG3,
                    "Entered notifications state {}",
                    current.path1.stem().asString());
                stateEntered.name() = current.path1.stem().asString();
                stateChange.stateEntered_ref() = std::move(stateEntered);
              } else if (!info.existedAfter) {
                StateLeft stateLeft;
                XLOGF(
                    DBG3,
                    "Left notifications state {}",
                    current.path1.stem().asString());
                stateLeft.name() = current.path1.stem().asString();
                stateChange.stateLeft_ref() = std::move(stateLeft);
              } else {
                // Modified state file happens on linux platforms immediately
                // after creation. Ignore it, since it doesn't change the state
                return true;
              }
            } else {
              // Other changes caused by the locking mechanism. Ignored
              // Return value = Should continue
              return true;
            }
            if (includeStateChanges) {
              StateChangeNotification stateChangeCopy =
                  StateChangeNotification(stateChange);
              change.stateChange_ref() = std::move(stateChange);
              result.changes_ref()->push_back(std::move(change));
              numStateChanges += 1;
            }
            // Return value = Should continue
            return true;
          }

          // Changes can effect either path1 or both paths
          // Determine if path1 pass the filters and default path2 to not
          bool includePath1 = isPathIncluded(
              includedAndExcludedRoots.first,
              includedAndExcludedRoots.second,
              includedAndExcludedSuffixes.first,
              includedAndExcludedSuffixes.second,
              current.path1);
          bool includePath2 = false;

          ChangeNotification change;
          LargeChangeNotification largeChange;
          SmallChangeNotification smallChange;

          if (current.isPath2Valid) {
            // Determine if path2 passes filters, but only if path1 one
            // doesn't to avoid an extra lookup
            includePath2 = includePath1
                ? false
                : isPathIncluded(
                      includedAndExcludedRoots.first,
                      includedAndExcludedRoots.second,
                      includedAndExcludedSuffixes.first,
                      includedAndExcludedSuffixes.second,
                      current.path2);
            if (includePath1 || includePath2) {
              const auto& info = current.info2;
              // NOTE: we could do a bunch of runtime checks here to
              // validate the infoN states would be a lot simpler if we
              // removed this infoN state and replaced them with a simple
              // enum

              // Constructs a Piece from the RelativePath
              RelativePathPiece pathString1 = current.path1;
              RelativePathPiece pathString2 = current.path2;

              // If root is empty, returns true if pathString is not also empty
              bool path1InRoot = pathString1.isSubDirOf(root);
              bool path2InRoot = pathString2.isSubDirOf(root);

              // if state change, skip over root handling
              if (!root.empty()) {
                // Filters include the path that matches the filter, but we want
                // to exclude it from a root
                // This is to match watchman's behavior regarding
                // relative roots.
                if (pathString1 == root || pathString2 == root) {
                  // Return value ignored here
                  return true;
                }
                // Trim the root + the separator
                if (path1InRoot) {
                  pathString1 = pathString1.substr(root.view().size() + 1);
                }
                if (path2InRoot) {
                  pathString2 = pathString2.substr(root.view().size() + 1);
                }
              }

              if (path1InRoot && path2InRoot) {
                if (info.existedBefore) {
                  // Replaced
                  Replaced replaced;
                  replaced.from() = pathString1.asString();
                  replaced.to() = pathString2.asString();
                  replaced.fileType() = static_cast<Dtype>(current.type);
                  smallChange.replaced() = std::move(replaced);
                  change.smallChange() = std::move(smallChange);
                  numSmallChanges += 1;
                } else {
                  // Renamed
                  if (current.type == dtype_t::Dir) {
                    DirectoryRenamed directoryRenamed;
                    directoryRenamed.from() = pathString1.asString();
                    directoryRenamed.to() = pathString2.asString();
                    largeChange.directoryRenamed() =
                        std::move(directoryRenamed);
                    change.largeChange() = std::move(largeChange);
                    numRenamedDirectory += 1;
                  } else {
                    Renamed renamed;
                    renamed.from() = pathString1.asString();
                    renamed.to() = pathString2.asString();
                    renamed.fileType() = static_cast<Dtype>(current.type);
                    smallChange.renamed() = std::move(renamed);
                    change.smallChange() = std::move(smallChange);
                    numSmallChanges += 1;
                  }
                }
              } else {
                if (path1InRoot) {
                  // File/Directory was renamed or replaced to a path outside of
                  // root. Report change as removed.
                  Removed removed;
                  removed.path() = pathString1.asString();
                  removed.fileType() = static_cast<Dtype>(current.type);
                  smallChange.removed() = std::move(removed);
                  change.smallChange() = std::move(smallChange);
                  numSmallChanges += 1;
                } else {
                  // File/Directory was renamed or replaced to a path inside of
                  // root. Report change as added (if renamed) or modified (if
                  // replaced).
                  if (info.existedBefore) {
                    // Modified
                    Modified modified;
                    modified.path() = pathString2.asString();
                    modified.fileType() = static_cast<Dtype>(current.type);
                    smallChange.modified() = std::move(modified);
                    change.smallChange() = std::move(smallChange);
                    numSmallChanges += 1;
                  } else {
                    Added added;
                    added.path() = pathString2.asString();
                    added.fileType() = static_cast<Dtype>(current.type);
                    smallChange.added() = std::move(added);
                    change.smallChange() = std::move(smallChange);
                    numSmallChanges += 1;
                  }
                }
              }
            }
          }
          // All single file changes have path1 pass the filters
          else if (includePath1) {
            const auto& info = current.info1;

            // Filters include the path that matches the filter, but we want
            // to exclude it from a root
            // This is to match watchman's behavior regarding
            // relative roots.
            if (!root.empty() && current.path1 == root) {
              // Return value ignored here
              return true;
            }

            // If a root is specified, it should be present due to being added
            // to includedRoots. Strip it and the first '/' out

            // Need to explicitly allocate storage in this scope for
            // mac/windows
            RelativePathPiece pathString = current.path1;

            // if state change, skip over roots truncation
            if (!root.empty()) {
              pathString = pathString.substr(root.view().size());
            }
            if (!info.existedBefore) {
              // Added
              Added added;
              added.path() = pathString.asString();
              added.fileType() = static_cast<Dtype>(current.type);
              smallChange.added() = std::move(added);
              change.smallChange() = std::move(smallChange);
              numSmallChanges += 1;
            } else if (!info.existedAfter) {
              // Removed
              Removed removed;
              removed.path() = pathString.asString();
              removed.fileType() = static_cast<Dtype>(current.type);
              smallChange.removed() = std::move(removed);
              change.smallChange() = std::move(smallChange);
              numSmallChanges += 1;
            } else {
              // Modified
              Modified modified;
              modified.path() = pathString.asString();
              modified.fileType() = static_cast<Dtype>(current.type);
              smallChange.modified() = std::move(modified);
              change.smallChange() = std::move(smallChange);
              numSmallChanges += 1;
            }
          }

          // Include a change if either path passes the filters
          if (includePath1 || includePath2) {
            result.changes()->push_back(std::move(change));
          } else {
            numFilteredResults += 1;
          }
          // Return value ignored here
          return true;
        },
        [&](const RootUpdateJournalDelta& current) -> bool {
          CommitTransition commitTransition;
          commitTransition.from() = rootIdCodec.renderRootId(current.fromRoot);
          commitTransition.to() = rootIdCodec.renderRootId(currentRoot);
          currentRoot = current.fromRoot;

          LargeChangeNotification largeChange;
          largeChange.commitTransition() = std::move(commitTransition);

          ChangeNotification change;
          change.largeChange() = std::move(largeChange);

          result.changes()->push_back(std::move(change));
          numCommitTransition += 1;
          // Return value ignored here
          return true;
        });

    if (isTruncated || result.changes()->size() > maxNumberOfChanges) {
      LostChanges lostChanges;
      lostChanges.reason() = isTruncated ? LostChangesReason::JOURNAL_TRUNCATED
                                         : LostChangesReason::TOO_MANY_CHANGES;
      lostChangesReason = static_cast<uint64_t>(lostChanges.reason().value());

      LargeChangeNotification largeChange;
      largeChange.lostChanges() = std::move(lostChanges);

      ChangeNotification change;
      change.largeChange() = std::move(largeChange);

      result.changes()->clear();
      result.changes()->push_back(std::move(change));
    } else {
      // TODO: this will be replace soon with in order processing. For now
      // we can accept a slight performance hit here by reversing the vector.

      // Results are neither truncated nor too many to return - reverse the
      // order to be oldest to newest
      std::reverse(result.changes()->begin(), result.changes()->end());
    }
  }

  if (!toSequence.has_value()) {
    // No changes, just use fromPosition.
    result.toPosition() = fromPosition;
  } else {
    JournalPosition toPosition;
    toPosition.mountGeneration() =
        mountHandle.getEdenMount().getMountGeneration();
    toPosition.sequenceNumber() = toSequence.value();
    toPosition.snapshotHash() = rootIdCodec.renderRootId(toSnapshotId);

    result.toPosition() = std::move(toPosition);
  }

  server_->getServerState()->getNotificationsStructuredLogger()->logEvent(
      ChangesSince{
          getClientCmdline(server_->getServerState(), fetchContext),
          logPosition(fromPosition),
          mountPath.asString(),
          root.asString(),
          includedRoots,
          excludedRoots,
          includedSuffixes,
          excludedSuffixes,
          includeVCSRoots,
          numSmallChanges,
          numStateChanges,
          numRenamedDirectory,
          numCommitTransition,
          lostChangesReason,
          numFilteredResults,
      });
}

folly::SemiFuture<std::unique_ptr<StartFileAccessMonitorResult>>
EdenServiceHandler::semifuture_startFileAccessMonitor(
    [[maybe_unused]] std::unique_ptr<StartFileAccessMonitorParams> params) {
#ifdef __APPLE__
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1, *params->paths_ref());

  constexpr std::string_view FAM_TMP_OUTPUT_DIR = "/tmp/edenfs/fam/";

  // Get the current time
  std::time_t nowTime =
      std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());

  // Create a character string to format the date and time
  char datetimeString[20];
  std::strftime(
      datetimeString,
      sizeof(datetimeString),
      "%Y%m%d_%H%M%S",
      std::localtime(&nowTime));

  // form the path to tmp file
  std::string tmpPath =
      fmt::format("{}fam_{}.out", FAM_TMP_OUTPUT_DIR, datetimeString);

  auto fut = ImmediateFuture<pid_t>(
      server_->getServerState()->getPrivHelper()->startFam(
          *params->paths(),
          tmpPath,
          params->specifiedOutputPath().value_or(tmpPath),
          *params->shouldUpload_ref()));
  return wrapImmediateFuture(
             std::move(helper),
             std::move(fut).thenValue(
                 [tmpPath = std::move(tmpPath)](pid_t pid) mutable {
                   auto out = std::make_unique<StartFileAccessMonitorResult>();
                   out->pid() = pid;
                   out->tmpOutputPath() = std::move(tmpPath);
                   return out;
                 }))
      .semi();
#else // !__APPLE__
  NOT_IMPLEMENTED();
#endif
}

folly::SemiFuture<std::unique_ptr<StopFileAccessMonitorResult>>
EdenServiceHandler::semifuture_stopFileAccessMonitor() {
#ifdef __APPLE__
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1);

  auto fut = ImmediateFuture<StopFileAccessMonitorResponse>(
      server_->getServerState()->getPrivHelper()->stopFam());

  return wrapImmediateFuture(
             std::move(helper), std::move(fut).thenValue([&](auto&& response) {
               auto out = std::make_unique<StopFileAccessMonitorResult>();
               out->tmpOutputPath() = response.tmpOutputPath;
               out->specifiedOutputPath() = response.specifiedOutputPath;
               out->shouldUpload() = response.shouldUpload;
               return out;
             }))
      .semi();
#else // !__APPLE__
  NOT_IMPLEMENTED();
#endif
}

void EdenServiceHandler::sendNotification(
    [[maybe_unused]] SendNotificationResponse&,
    [[maybe_unused]] std::unique_ptr<SendNotificationRequest> request) {
#ifdef _WIN32
  server_->getServerState()->getNotifier()->showHealthReportNotification(
      request->title().value(), request->description().value());
#else
  (void)request;
  NOT_IMPLEMENTED();
#endif
}

apache::thrift::ResponseAndServerStream<ChangesSinceResult, ChangedFileResult>
EdenServiceHandler::streamSelectedChangesSince(
    std::unique_ptr<StreamSelectedChangesSinceParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL_WITH_STAT(
      DBG3,
      &ThriftStats::streamSelectedChangesSince,
      *params->changesParams()->mountPoint());
  auto mountHandle = lookupMount(params->changesParams()->mountPoint().value());
  const auto& fromPosition = *params->changesParams()->fromPosition();
  auto& fetchContext = helper->getFetchContext();

  checkMountGeneration(
      fromPosition, mountHandle.getEdenMount(), "fromPosition"sv);

  auto summed = mountHandle.getJournal().accumulateRange(
      *fromPosition.sequenceNumber() + 1);

  ChangesSinceResult result;
  if (!summed) {
    // No changes, just return the fromPosition and an empty stream.
    result.toPosition() = fromPosition;

    return {
        std::move(result),
        apache::thrift::ServerStream<ChangedFileResult>::createEmpty()};
  }

  if (summed->isTruncated) {
    throw newEdenError(
        EDOM,
        EdenErrorType::JOURNAL_TRUNCATED,
        "Journal entry range has been truncated.");
  }

  auto cancellationSource = std::make_shared<folly::CancellationSource>();
  auto [serverStream, publisher] =
      apache::thrift::ServerStream<ChangedFileResult>::createPublisher(
          [cancellationSource] { cancellationSource->requestCancellation(); });
  auto sharedPublisherLock = std::make_shared<
      folly::Synchronized<ThriftStreamPublisherOwner<ChangedFileResult>>>(
      ThriftStreamPublisherOwner{std::move(publisher)});

  RootIdCodec& rootIdCodec = mountHandle.getObjectStore();

  JournalPosition toPosition;
  toPosition.mountGeneration() =
      mountHandle.getEdenMount().getMountGeneration();
  toPosition.sequenceNumber() = summed->toSequence;
  toPosition.snapshotHash() =
      rootIdCodec.renderRootId(summed->snapshotTransitions.back());
  result.toPosition() = toPosition;

  auto caseSensitivity =
      mountHandle.getEdenMount().getCheckoutConfig()->getCaseSensitive();
  auto filter =
      std::make_unique<GlobFilter>(params->globs().value(), caseSensitivity);

  sumUncommittedChanges(
      *summed, *sharedPublisherLock, std::reference_wrapper(*filter));

  if (summed->snapshotTransitions.size() > 1) {
    // create filtered backing store
    std::shared_ptr<FilteredBackingStore> backingStore =
        std::make_shared<FilteredBackingStore>(
            mountHandle.getEdenMountPtr()->getObjectStore()->getBackingStore(),
            std::move(filter));
    // pass filtered backing store to object store
    auto objectStore = ObjectStore::create(
        backingStore,
        server_->getLocalStore(),
        server_->getTreeCache(),
        server_->getServerState()->getStats().copy(),
        server_->getServerState()->getProcessInfoCache(),
        server_->getServerState()->getStructuredLogger(),
        server_->getServerState()->getReloadableConfig(),
        mountHandle.getEdenMount()
            .getCheckoutConfig()
            ->getEnableWindowsSymlinks(),
        caseSensitivity);
    auto callback =
        std::make_shared<StreamingDiffCallback>(sharedPublisherLock);

    std::vector<ImmediateFuture<folly::Unit>> futures;
    // now iterate all commits
    for (auto rootIt = summed->snapshotTransitions.begin();
         std::next(rootIt) != summed->snapshotTransitions.end();
         ++rootIt) {
      const auto from =
          backingStore->createFilteredRootId(rootIt->value(), rootIt->value());
      const auto& toRootId = *(rootIt + 1);
      const auto to = backingStore->createFilteredRootId(
          toRootId.value(), toRootId.value());

      futures.push_back(makeNotReadyImmediateFuture().thenValue(
          [from,
           to,
           mountHandle,
           objectStore,
           token = cancellationSource->getToken(),
           fetchContext = fetchContext.copy(),
           callback = callback.get()](auto&&) {
            return diffBetweenRoots(
                RootId{from},
                RootId{to},
                *mountHandle.getEdenMount().getCheckoutConfig(),
                objectStore,
                token,
                fetchContext,
                callback);
          }));
    }

    folly::futures::detachOn(
        server_->getServerState()->getThreadPool().get(),
        collectAllSafe(std::move(futures))
            .thenTry(
                [mountHandle,
                 sharedPublisherLock,
                 callback = std::move(callback),
                 helper = std::move(helper),
                 cancellationSource](
                    folly::Try<std::vector<folly::Unit>>&& result) mutable {
                  if (result.hasException()) {
                    auto sharedPublisher =
                        std::move(*sharedPublisherLock->wlock());
                    std::move(sharedPublisher)
                        .next(newEdenError(std::move(result).exception()));
                  }
                })
            .semi());
  }
  return {std::move(result), std::move(serverStream)};
}

void EdenServiceHandler::getFilesChangedSince(
    FileDelta& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<JournalPosition> fromPosition) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);

  checkMountGeneration(
      *fromPosition, mountHandle.getEdenMount(), "fromPosition"sv);

  // The +1 is because the core merge stops at the item prior to
  // its limitSequence parameter and we want the changes *since*
  // the provided sequence number.
  auto summed = mountHandle.getJournal().accumulateRange(
      *fromPosition->sequenceNumber() + 1);

  // We set the default toPosition to be where we where if summed is null
  out.toPosition()->sequenceNumber() = *fromPosition->sequenceNumber();
  out.toPosition()->snapshotHash() = *fromPosition->snapshotHash();
  out.toPosition()->mountGeneration() =
      mountHandle.getEdenMount().getMountGeneration();

  out.fromPosition() = *out.toPosition();

  if (summed) {
    if (summed->isTruncated) {
      throw newEdenError(
          EDOM,
          EdenErrorType::JOURNAL_TRUNCATED,
          "Journal entry range has been truncated.");
    }

    RootIdCodec& rootIdCodec = mountHandle.getObjectStore();

    out.toPosition()->sequenceNumber() = summed->toSequence;
    out.toPosition()->snapshotHash() =
        rootIdCodec.renderRootId(summed->snapshotTransitions.back());
    out.toPosition()->mountGeneration() =
        mountHandle.getEdenMount().getMountGeneration();

    out.fromPosition()->sequenceNumber() = summed->fromSequence;
    out.fromPosition()->snapshotHash() =
        rootIdCodec.renderRootId(summed->snapshotTransitions.front());
    out.fromPosition()->mountGeneration() =
        *out.toPosition()->mountGeneration();

    for (const auto& entry : summed->changedFilesInOverlay) {
      auto& path = entry.first;
      auto& changeInfo = entry.second;
      if (changeInfo.isNew()) {
        out.createdPaths()->emplace_back(path.asString());
      } else {
        out.changedPaths()->emplace_back(path.asString());
      }
    }

    for (auto& path : summed->uncleanPaths) {
      out.uncleanPaths()->emplace_back(path.asString());
    }

    out.snapshotTransitions()->reserve(summed->snapshotTransitions.size());
    for (auto& id : summed->snapshotTransitions) {
      out.snapshotTransitions()->push_back(rootIdCodec.renderRootId(id));
    }
  }
}

void EdenServiceHandler::setJournalMemoryLimit(
    std::unique_ptr<PathString> mountPoint,
    int64_t limit) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  if (limit < 0) {
    throw newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "memory limit must be non-negative");
  }
  mountHandle.getJournal().setMemoryLimit(static_cast<size_t>(limit));
}

int64_t EdenServiceHandler::getJournalMemoryLimit(
    std::unique_ptr<PathString> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  return static_cast<int64_t>(mountHandle.getJournal().getMemoryLimit());
}

void EdenServiceHandler::flushJournal(std::unique_ptr<PathString> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint);
  auto mountHandle = lookupMount(mountPoint);
  mountHandle.getJournal().flush();
}

void EdenServiceHandler::debugGetRawJournal(
    DebugGetRawJournalResponse& out,
    std::unique_ptr<DebugGetRawJournalParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *params->mountPoint());
  auto mountHandle = lookupMount(params->mountPoint());
  auto mountGeneration =
      static_cast<ssize_t>(mountHandle.getEdenMount().getMountGeneration());

  std::optional<size_t> limitopt = std::nullopt;
  if (auto limit = params->limit()) {
    limitopt = static_cast<size_t>(*limit);
  }

  out.allDeltas() = mountHandle.getJournal().getDebugRawJournalInfo(
      *params->fromSequenceNumber(),
      limitopt,
      mountGeneration,
      mountHandle.getObjectStore());
}

folly::SemiFuture<std::unique_ptr<std::vector<EntryInformationOrError>>>
EdenServiceHandler::semifuture_getEntryInformation(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths,
    std::unique_ptr<SyncBehavior> sync) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), toLogArg(*paths));
  auto mountHandle = lookupMount(mountPoint);
  auto& fetchContext = helper->getFetchContext();

  return wrapImmediateFuture(
             std::move(helper),
             waitForPendingWrites(mountHandle.getEdenMount(), *sync)
                 .thenValue([mountHandle,
                             paths = std::move(paths),
                             fetchContext = fetchContext.copy()](auto&&) {
                   bool windowsSymlinksEnabled =
                       mountHandle.getEdenMount()
                           .getCheckoutConfig()
                           ->getEnableWindowsSymlinks();
                   return applyToVirtualInode(
                       mountHandle.getRootInode(),
                       *paths,
                       [windowsSymlinksEnabled](
                           const VirtualInode& inode, RelativePath) {
                         return filteredEntryDtype(
                             inode.getDtype(), windowsSymlinksEnabled);
                       },
                       mountHandle.getObjectStorePtr(),
                       fetchContext);
                 })
                 .thenValue([](vector<Try<dtype_t>> done) {
                   auto out =
                       std::make_unique<vector<EntryInformationOrError>>();
                   out->reserve(done.size());
                   for (auto& item : done) {
                     EntryInformationOrError result;
                     if (item.hasException()) {
                       result.error() = newEdenError(item.exception());
                     } else {
                       EntryInformation info;
                       info.dtype() = static_cast<Dtype>(item.value());
                       result.info() = info;
                     }
                     out->emplace_back(std::move(result));
                   }
                   return out;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<std::vector<FileInformationOrError>>>
EdenServiceHandler::semifuture_getFileInformation(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths,
    std::unique_ptr<SyncBehavior> sync) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), toLogArg(*paths));
  auto mountHandle = lookupMount(mountPoint);
  auto& fetchContext = helper->getFetchContext();
  auto lastCheckoutTime =
      mountHandle.getEdenMount().getLastCheckoutTime().toTimespec();

  return wrapImmediateFuture(
             std::move(helper),
             waitForPendingWrites(mountHandle.getEdenMount(), *sync)
                 .thenValue([mountHandle,
                             paths = std::move(paths),
                             lastCheckoutTime,
                             fetchContext = fetchContext.copy()](auto&&) {
                   return applyToVirtualInode(
                       mountHandle.getRootInode(),
                       *paths,
                       [mountHandle,
                        lastCheckoutTime,
                        fetchContext = fetchContext.copy()](
                           const VirtualInode& inode, RelativePath) {
                         return inode
                             .stat(
                                 lastCheckoutTime,
                                 mountHandle.getObjectStorePtr(),
                                 fetchContext)
                             .thenValue([](struct stat st) {
                               FileInformation info;
                               info.size() = st.st_size;
                               auto ts = stMtime(st);
                               info.mtime()->seconds() = ts.tv_sec;
                               info.mtime()->nanoSeconds() = ts.tv_nsec;
                               info.mode() = st.st_mode;

                               FileInformationOrError result;
                               result.info() = info;

                               return result;
                             })
                             .semi();
                       },
                       mountHandle.getObjectStorePtr(),
                       fetchContext);
                 })
                 .thenValue([](vector<Try<FileInformationOrError>>&& done) {
                   auto out =
                       std::make_unique<vector<FileInformationOrError>>();
                   out->reserve(done.size());
                   for (auto& item : done) {
                     if (item.hasException()) {
                       FileInformationOrError result;
                       result.error() = newEdenError(item.exception());
                       out->emplace_back(std::move(result));
                     } else {
                       out->emplace_back(item.value());
                     }
                   }
                   return out;
                 }))
      .ensure([mountHandle] {})
      .semi();
}

namespace {
SourceControlType entryTypeToThriftType(std::optional<TreeEntryType> type) {
  if (!type.has_value()) {
    return SourceControlType::UNKNOWN;
  }
  switch (type.value()) {
    case TreeEntryType::TREE:
      return SourceControlType::TREE;
    case TreeEntryType::REGULAR_FILE:
      return SourceControlType::REGULAR_FILE;
    case TreeEntryType::EXECUTABLE_FILE:
      return SourceControlType::EXECUTABLE_FILE;
    case TreeEntryType::SYMLINK:
      return SourceControlType::SYMLINK;
    default:
      throw std::system_error(EINVAL, std::generic_category());
  }
}

ImmediateFuture<
    std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>
getAllEntryAttributes(
    EntryAttributeFlags requestedAttributes,
    const EdenMount& edenMount,
    std::string path,
    const ObjectFetchContextPtr& fetchContext) {
  auto virtualInode =
      edenMount.getVirtualInode(RelativePathPiece{path}, fetchContext);
  return std::move(virtualInode)
      .thenValue(
          [path = std::move(path),
           requestedAttributes,
           objectStore = edenMount.getObjectStore(),
           lastCheckoutTime = edenMount.getLastCheckoutTime().toTimespec(),
           fetchContext = fetchContext.copy()](VirtualInode tree) mutable {
            if (!tree.isDirectory()) {
              return ImmediateFuture<std::vector<
                  std::pair<PathComponent, folly::Try<EntryAttributes>>>>(
                  newEdenError(
                      EINVAL,
                      EdenErrorType::ARGUMENT_ERROR,
                      fmt::format("{}: path must be a directory", path)));
            }
            return tree.getChildrenAttributes(
                requestedAttributes,
                RelativePath{path},
                objectStore,
                lastCheckoutTime,
                fetchContext);
          });
}

template <typename SerializedT, typename T>
bool fillErrorRef(
    SerializedT& result,
    std::optional<folly::Try<T>> rawResult,
    folly::StringPiece path,
    folly::StringPiece attributeName) {
  if (!rawResult.has_value()) {
    result.error_ref() = newEdenError(
        ENOENT,
        EdenErrorType::ATTRIBUTE_UNAVAILABLE,
        fmt::format(
            "{}: {} requested, but no {} available",
            path,
            attributeName,
            attributeName));
    return true;
  }
  if (rawResult.value().hasException()) {
    result.error_ref() = newEdenError(rawResult.value().exception());
    return true;
  }
  return false;
}

FileAttributeDataOrErrorV2 serializeEntryAttributes(
    ObjectStore& objectStore,
    folly::StringPiece entryPath,
    const folly::Try<EntryAttributes>& attributes,
    EntryAttributeFlags requestedAttributes) {
  FileAttributeDataOrErrorV2 fileResult;

  if (attributes.hasException()) {
    fileResult.error() = newEdenError(attributes.exception());
    return fileResult;
  }

  FileAttributeDataV2 fileData;
  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SHA1)) {
    Sha1OrError sha1;
    if (!fillErrorRef(sha1, attributes->sha1, entryPath, "sha1")) {
      sha1.sha1() = thriftHash20(attributes->sha1.value().value());
    }
    fileData.sha1() = std::move(sha1);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_BLAKE3)) {
    Blake3OrError blake3;
    if (!fillErrorRef(blake3, attributes->blake3, entryPath, "blake3")) {
      blake3.blake3() = thriftHash32(attributes->blake3.value().value());
    }
    fileData.blake3() = std::move(blake3);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SIZE)) {
    SizeOrError size;
    if (!fillErrorRef(size, attributes->size, entryPath, "size")) {
      size.size() = attributes->size.value().value();
    }
    fileData.size() = std::move(size);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE)) {
    SourceControlTypeOrError type;
    if (!fillErrorRef(type, attributes->type, entryPath, "type")) {
      type.sourceControlType() =
          entryTypeToThriftType(attributes->type.value().value());
    }
    fileData.sourceControlType() = std::move(type);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_OBJECT_ID)) {
    ObjectIdOrError objectId;
    if (!fillErrorRef(objectId, attributes->objectId, entryPath, "objectid")) {
      const std::optional<ObjectId>& oid = attributes->objectId.value().value();
      if (oid) {
        objectId.objectId() = objectStore.renderObjectId(*oid);
      }
    }
    fileData.objectId() = std::move(objectId);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_DIGEST_SIZE)) {
    DigestSizeOrError digestSize;
    if (!fillErrorRef(
            digestSize, attributes->digestSize, entryPath, "digestsize")) {
      digestSize.digestSize() = attributes->digestSize.value().value();
    }
    fileData.digestSize() = std::move(digestSize);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_DIGEST_HASH)) {
    DigestHashOrError digestHash;
    if (!fillErrorRef(
            digestHash, attributes->digestHash, entryPath, "digesthash")) {
      digestHash.digestHash() =
          thriftHash32(attributes->digestHash.value().value());
    }
    fileData.digestHash() = std::move(digestHash);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_MTIME)) {
    MtimeOrError mtime;
    if (!fillErrorRef(mtime, attributes->mtime, entryPath, "mtime")) {
      mtime.mtime() = thriftTimeSpec(attributes->mtime.value().value());
    }
    fileData.mtime() = std::move(mtime);
  }

  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_MODE)) {
    ModeOrError mode;
    if (!fillErrorRef(mode, attributes->mode, entryPath, "mode")) {
      mode.mode() = attributes->mode.value().value();
    }
    fileData.mode() = std::move(mode);
  }

  fileResult.fileAttributeData() = fileData;
  return fileResult;
}

DirListAttributeDataOrError serializeEntryAttributes(
    ObjectStore& objectStore,
    const folly::Try<
        std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>&
        entries,
    EntryAttributeFlags requestedAttributes) {
  DirListAttributeDataOrError result;
  if (entries.hasException()) {
    result.error() = newEdenError(*entries.exception().get_exception());
    return result;
  }
  std::map<std::string, FileAttributeDataOrErrorV2> thriftEntryResult;
  for (auto& [path_component, attributes] : entries.value()) {
    thriftEntryResult.emplace(
        path_component.asString(),
        serializeEntryAttributes(
            objectStore,
            path_component.piece().view(),
            attributes,
            requestedAttributes));
  }

  result.dirListAttributeData() = std::move(thriftEntryResult);
  return result;
}

} // namespace

folly::SemiFuture<std::unique_ptr<ReaddirResult>>
EdenServiceHandler::semifuture_readdir(std::unique_ptr<ReaddirParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());
  auto paths = *params->directoryPaths();
  // Get requested attributes for each path
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint(),
      getSyncTimeout(*params->sync()),
      toLogArg(paths));
  auto& fetchContext = helper->getFetchContext();
  auto requestedAttributes =
      EntryAttributeFlags::raw(*params->requestedAttributes());

  return wrapImmediateFuture(
             std::move(helper),
             waitForPendingWrites(mountHandle.getEdenMount(), *params->sync())
                 .thenValue(
                     [mountHandle,
                      requestedAttributes,
                      paths = std::move(paths),
                      fetchContext = fetchContext.copy()](auto&&) mutable
                     -> ImmediateFuture<
                         std::vector<DirListAttributeDataOrError>> {
                       std::vector<ImmediateFuture<DirListAttributeDataOrError>>
                           futures;
                       futures.reserve(paths.size());
                       for (auto& path : paths) {
                         futures.emplace_back(
                             getAllEntryAttributes(
                                 requestedAttributes,
                                 mountHandle.getEdenMount(),
                                 std::move(path),
                                 fetchContext)
                                 .thenTry([requestedAttributes, mountHandle](
                                              folly::Try<std::vector<std::pair<
                                                  PathComponent,
                                                  folly::Try<EntryAttributes>>>>
                                                  entries) {
                                   return serializeEntryAttributes(
                                       mountHandle.getObjectStore(),
                                       entries,
                                       requestedAttributes);
                                 })

                         );
                       }

                       // Collect all futures into a single tuple
                       return facebook::eden::collectAllSafe(
                           std::move(futures));
                     })
                 .thenValue(
                     [](std::vector<DirListAttributeDataOrError>&& allRes)
                         -> std::unique_ptr<ReaddirResult> {
                       auto res = std::make_unique<ReaddirResult>();
                       res->dirLists() = std::move(allRes);
                       return res;
                     })
                 .ensure([mountHandle] {}))
      .semi();
}

ImmediateFuture<std::vector<folly::Try<EntryAttributes>>>
EdenServiceHandler::getEntryAttributes(
    const EdenMount& edenMount,
    const std::vector<std::string>& paths,
    EntryAttributeFlags reqBitmask,
    AttributesRequestScope reqScope,
    SyncBehavior sync,
    const ObjectFetchContextPtr& fetchContext) {
  return waitForPendingWrites(edenMount, sync)
      .thenValue([this,
                  &edenMount,
                  &paths,
                  fetchContext = fetchContext.copy(),
                  reqBitmask,
                  reqScope](auto&&) mutable {
        vector<ImmediateFuture<EntryAttributes>> futures;
        for (const auto& path : paths) {
          futures.emplace_back(getEntryAttributesForPath(
              edenMount, reqBitmask, reqScope, path, fetchContext));
        }

        // Collect all futures into a single tuple
        return facebook::eden::collectAll(std::move(futures));
      });
}

namespace {
bool dtypeMatchesRequestScope(
    VirtualInode inode,
    AttributesRequestScope reqScope) {
  if (reqScope == AttributesRequestScope::TREES_AND_FILES) {
    return true;
  }

  if (inode.isDirectory()) {
    return reqScope == AttributesRequestScope::TREES;
  } else {
    return reqScope == AttributesRequestScope::FILES;
  }
}
} // namespace

ImmediateFuture<EntryAttributes> EdenServiceHandler::getEntryAttributesForPath(
    const EdenMount& edenMount,
    EntryAttributeFlags reqBitmask,
    AttributesRequestScope reqScope,
    std::string_view path,
    const ObjectFetchContextPtr& fetchContext) {
  if (path.empty()) {
    return ImmediateFuture<EntryAttributes>(newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "path cannot be the empty string"));
  }

  try {
    RelativePathPiece relativePath{path};

    return edenMount.getVirtualInode(relativePath, fetchContext)
        .thenValue([&edenMount,
                    reqBitmask,
                    reqScope,
                    relativePath = relativePath.copy(),
                    fetchContext =
                        fetchContext.copy()](const VirtualInode& virtualInode) {
          if (dtypeMatchesRequestScope(virtualInode, reqScope)) {
            return virtualInode.getEntryAttributes(
                reqBitmask,
                relativePath,
                edenMount.getObjectStore(),
                edenMount.getLastCheckoutTime().toTimespec(),
                fetchContext);
          }
          return makeImmediateFuture<EntryAttributes>(PathError(
              reqScope == AttributesRequestScope::TREES ? ENOTDIR : EISDIR,
              relativePath));
        });
  } catch (const std::exception& e) {
    return ImmediateFuture<EntryAttributes>(
        newEdenError(EINVAL, EdenErrorType::ARGUMENT_ERROR, e.what()));
  }
}

folly::SemiFuture<std::unique_ptr<GetAttributesFromFilesResultV2>>
EdenServiceHandler::semifuture_getAttributesFromFilesV2(
    std::unique_ptr<GetAttributesFromFilesParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());
  auto reqScope =
      params->scope().value_or(AttributesRequestScope::TREES_AND_FILES);
  auto reqBitmask = EntryAttributeFlags::raw(*params->requestedAttributes());
  std::vector<std::string>& paths = params->paths().value();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint(),
      getSyncTimeout(*params->sync()),
      toLogArg(paths));
  auto& fetchContext = helper->getFetchContext();

  auto config = server_->getServerState()->getEdenConfig();
  auto entryAttributesFuture = getEntryAttributes(
      mountHandle.getEdenMount(),
      paths,
      reqBitmask,
      reqScope,
      *params->sync(),
      fetchContext);

  return wrapImmediateFuture(
             std::move(helper),
             std::move(entryAttributesFuture)
                 .thenValue(
                     [reqBitmask, mountHandle, &paths](
                         std::vector<folly::Try<EntryAttributes>>&& allRes) {
                       auto res =
                           std::make_unique<GetAttributesFromFilesResultV2>();
                       size_t index = 0;
                       for (const auto& tryAttributes : allRes) {
                         res->res()->emplace_back(serializeEntryAttributes(
                             mountHandle.getObjectStore(),
                             basename(paths.at(index)),
                             tryAttributes,
                             reqBitmask));
                         ++index;
                       }
                       return res;
                     }))
      .ensure([mountHandle, params = std::move(params)]() {
        // keeps the params memory around for the duration of the thrift call,
        // so that we can safely use the paths by reference to avoid making
        // copies.
      })
      .semi();
}

folly::SemiFuture<std::unique_ptr<SetPathObjectIdResult>>
EdenServiceHandler::semifuture_setPathObjectId(
    std::unique_ptr<SetPathObjectIdParams> params) {
#ifndef _WIN32
  auto mountHandle = lookupMount(params->mountPoint());
  std::vector<SetPathObjectIdObjectAndPath> objects;
  std::vector<std::string> object_strings;
  auto objectSize =
      params->objects().is_set() ? params->objects()->size() + 1 : 1;
  objects.reserve(objectSize);
  object_strings.reserve(objectSize);

  // TODO deprecate non-batch fields once all clients moves to the batch
  // fields. Rust clients might set to default and is_set() would return false
  // negative
  if (params->objectId().is_set() && !params->objectId()->empty()) {
    SetPathObjectIdObjectAndPath objectAndPath;
    objectAndPath.path = RelativePath{*params->path()};
    objectAndPath.id =
        mountHandle.getObjectStore().parseObjectId(*params->objectId());
    objectAndPath.type = *params->type();
    object_strings.emplace_back(objectAndPath.toString());
    objects.emplace_back(std::move(objectAndPath));
  }

  for (auto& object : *params->objects()) {
    SetPathObjectIdObjectAndPath objectAndPath;
    objectAndPath.path = RelativePath{*object.path()};
    objectAndPath.id =
        mountHandle.getObjectStore().parseObjectId(object.objectId().value());
    objectAndPath.type = *object.type();
    object_strings.emplace_back(objectAndPath.toString());
    objects.emplace_back(std::move(objectAndPath));
  }

  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG1, *params->mountPoint(), toLogArg(object_strings));

  if (auto requestInfo = params->requestInfo()) {
    helper->getThriftFetchContext().updateRequestInfo(std::move(*requestInfo));
  }
  ObjectFetchContextPtr context = helper->getFetchContext().copy();
  return wrapImmediateFuture(
             std::move(helper),
             mountHandle.getEdenMount()
                 .setPathsToObjectIds(
                     std::move(objects), (*params->mode()), context)
                 .thenValue([](auto&& resultAndTimes) {
                   return std::make_unique<SetPathObjectIdResult>(
                       std::move(resultAndTimes.result));
                 }))
      .ensure([mountHandle] {})
      .semi();
#else
  (void)params;
  NOT_IMPLEMENTED();
#endif
}

folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_removeRecursively(
    std::unique_ptr<RemoveRecursivelyParams> params) {
  auto mountPoint = *params->mountPoint();
  auto repoPath = *params->path();

  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, mountPoint, repoPath);
  auto mountHandle = lookupMount(mountPoint);

  auto relativePath = RelativePath{repoPath};
  auto& fetchContext = helper->getFetchContext();

  return wrapImmediateFuture(
             std::move(helper),
             waitForPendingWrites(mountHandle.getEdenMount(), *params->sync())
                 .thenValue([mountHandle,
                             relativePath,
                             fetchContext = fetchContext.copy()](folly::Unit) {
                   return mountHandle.getEdenMount().getInodeSlow(
                       relativePath, fetchContext);
                 })
                 .thenValue([relativePath, fetchContext = fetchContext.copy()](
                                InodePtr inode) {
                   return inode->getParentRacy()->removeRecursively(
                       relativePath.basename(),
                       InvalidationRequired::Yes,
                       fetchContext);
                 }))
      .ensure([mountHandle] {})
      .semi();
}

namespace {
template <typename ReturnType>
ImmediateFuture<std::unique_ptr<ReturnType>> detachIfBackgrounded(
    ImmediateFuture<std::unique_ptr<ReturnType>> future,
    const std::shared_ptr<ServerState>& serverState,
    bool background) {
  if (!background) {
    return future;
  } else {
    folly::futures::detachOn(
        serverState->getThreadPool().get(), std::move(future).semi());
    return ImmediateFuture<std::unique_ptr<ReturnType>>(
        std::make_unique<ReturnType>());
  }
}

template <typename ReturnType>
folly::SemiFuture<std::unique_ptr<ReturnType>> serialDetachIfBackgrounded(
    ImmediateFuture<std::unique_ptr<ReturnType>> future,
    EdenServer* const server,
    bool background) {
  // If we're already using serial execution across the board, just do a
  // normal detachIfBackgrounded
  if (server->usingThriftSerialExecution()) {
    return detachIfBackgrounded(
               std::move(future), server->getServerState(), background)
        .semi();
  }

  folly::Executor::KeepAlive<> serial;
  if (server->getServerState()
          ->getEdenConfig()
          ->thriftUseSmallSerialExecutor.getValue()) {
    serial = folly::SmallSerialExecutor::create(
        server->getServer()->getThreadManager().get());
  } else {
    serial = folly::SerialExecutor::create(
        server->getServer()->getThreadManager().get());
  }

  if (background) {
    folly::futures::detachOn(serial, std::move(future).semi());
    future = ImmediateFuture<std::unique_ptr<ReturnType>>(
        std::make_unique<ReturnType>());
  }

  if (future.isReady()) {
    return std::move(future).semi();
  }

  return std::move(future).semi().via(serial);
}

ImmediateFuture<folly::Unit> detachIfBackgrounded(
    ImmediateFuture<folly::Unit> future,
    const std::shared_ptr<ServerState>& serverState,
    bool background) {
  if (!background) {
    return future;
  } else {
    folly::futures::detachOn(
        serverState->getThreadPool().get(), std::move(future).semi());
    return ImmediateFuture<folly::Unit>(folly::unit);
  }
}

folly::SemiFuture<folly::Unit> serialDetachIfBackgrounded(
    ImmediateFuture<folly::Unit> future,
    EdenServer* const server,
    bool background) {
  // If we're already using serial execution across the board, just do a
  // normal detachIfBackgrounded
  if (server->usingThriftSerialExecution()) {
    return detachIfBackgrounded(
               std::move(future), server->getServerState(), background)
        .semi();
  }

  folly::Executor::KeepAlive<> serial;
  if (server->getServerState()
          ->getEdenConfig()
          ->thriftUseSmallSerialExecutor.getValue()) {
    serial = folly::SmallSerialExecutor::create(
        server->getServer()->getThreadManager().get());
  } else {
    serial = folly::SerialExecutor::create(
        server->getServer()->getThreadManager().get());
  }

  if (background) {
    folly::futures::detachOn(serial, std::move(future).semi());
    future = ImmediateFuture<folly::Unit>(folly::unit);
  }

  if (future.isReady()) {
    return std::move(future).semi();
  }

  return std::move(future).semi().via(serial);
}

void maybeLogExpensiveGlob(
    const std::vector<std::string>& globs,
    const folly::StringPiece searchRoot,
    const ThriftGlobImpl& globber,
    const ObjectFetchContextPtr& context,
    const std::shared_ptr<ServerState>& serverState) {
  bool shouldLogExpensiveGlob = false;

  if (searchRoot.empty()) {
    for (const auto& glob : globs) {
      if (string_view{glob}.starts_with("**")) {
        shouldLogExpensiveGlob = true;
      }
    }
  }

  if (shouldLogExpensiveGlob) {
    auto logString = globber.logString(globs);
    std::string client_cmdline = getClientCmdline(serverState, context);
    XLOGF(
        WARN,
        "EdenFS asked to evaluate expensive glob by caller {} : {}",
        client_cmdline,
        logString);
    serverState->getStructuredLogger()->logEvent(
        StarGlob{std::move(logString), std::move(client_cmdline)});
  }
}
} // namespace

#ifndef _WIN32
namespace {
ImmediateFuture<folly::Unit> ensureMaterializedImpl(
    std::shared_ptr<EdenMount> edenMount,
    const std::vector<std::string>& repoPaths,
    std::unique_ptr<ThriftRequestScope> helper,
    bool followSymlink) {
  std::vector<ImmediateFuture<folly::Unit>> futures;
  futures.reserve(repoPaths.size());

  auto& fetchContext = helper->getFetchContext();

  for (auto& path : repoPaths) {
    futures.emplace_back(
        makeNotReadyImmediateFuture()
            .thenValue([edenMount = edenMount.get(),
                        path = RelativePath{path},
                        fetchContext = fetchContext.copy()](auto&&) {
              return edenMount->getInodeSlow(path, fetchContext);
            })
            .thenValue([fetchContext = fetchContext.copy(),
                        followSymlink](InodePtr inode) {
              return inode->ensureMaterialized(fetchContext, followSymlink)
                  .ensure([inode]() {});
            }));
  }

  return wrapImmediateFuture(
      std::move(helper), collectAll(std::move(futures)).unit());
}
} // namespace
#endif

folly::SemiFuture<folly::Unit>
EdenServiceHandler::semifuture_ensureMaterialized(
    std::unique_ptr<EnsureMaterializedParams> params) {
#ifndef _WIN32
  auto mountPoint = *params->mountPoint();
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG4, mountPoint, toLogArg(*params->paths()));

  auto mountHandle = lookupMount(mountPoint);
  // The background mode is not fully running on background, instead, it will
  // start to load inodes in a blocking way, and then collect unready
  // materialization process then throws to the background. This is most
  // efficient way for the local execution of virtualized buck-out as avoid
  // cache exchange by materializing smaller random reads, and not prevent
  // execution starting by read large files on the background.
  bool background = *params->background();

  auto waitForPendingWritesFuture =
      waitForPendingWrites(mountHandle.getEdenMount(), *params->sync());
  auto ensureMaterializedFuture =
      std::move(waitForPendingWritesFuture)
          .thenValue([params = std::move(params),
                      mountHandle,
                      helper = std::move(helper)](auto&&) mutable {
            return ensureMaterializedImpl(
                mountHandle.getEdenMountPtr(),
                (*params->paths()),
                std::move(helper),
                (*params->followSymlink()));
          })
          .ensure([mountHandle] {})
          .semi();

  if (background) {
    folly::futures::detachOn(
        server_->getServerState()->getThreadPool().get(),
        std::move(ensureMaterializedFuture));
    return folly::unit;
  } else {
    return ensureMaterializedFuture;
  }
#else
  (void)params;
  NOT_IMPLEMENTED();
#endif
}

folly::SemiFuture<std::unique_ptr<Glob>>
EdenServiceHandler::semifuture_predictiveGlobFiles(
    std::unique_ptr<GlobParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());
  if (!params->revisions().value().empty()) {
    params->revisions() =
        resolveRootsWithLastFilter(params->revisions().value(), mountHandle);
  }
  ThriftGlobImpl globber{*params};
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG3, *params->mountPoint(), globber.logString());

  /* set predictive glob fetch parameters */
  // if numResults is not specified, use default predictivePrefetchProfileSize
  auto& serverState = server_->getServerState();
  auto numResults =
      serverState->getEdenConfig()->predictivePrefetchProfileSize.getValue();
  // if user is not specified, get user info from the server state
  auto user = folly::StringPiece{serverState->getUserInfo().getUsername()};
  auto backingStore = mountHandle.getObjectStore().getBackingStore();
  // if repo is not specified, get repository name from the backingstore
  auto repo_optional = backingStore->getRepoName();
  if (repo_optional == std::nullopt) {
    // typeid() does not evaluate expressions
    auto& r = *backingStore.get();
    throw std::runtime_error(folly::to<std::string>(
        "mount must use SaplingBackingStore, type is ", typeid(r).name()));
  }

  auto repo = repo_optional.value();
  auto os = getOperatingSystemName();

  // sandcastleAlias, startTime, and endTime are optional parameters
  std::optional<std::string> sandcastleAlias;
  std::optional<uint64_t> startTime;
  std::optional<uint64_t> endTime;
  // check if this is a sandcastle job (getenv will return nullptr if the env
  // variable is not set)
  auto scAliasEnv = std::getenv("SANDCASTLE_ALIAS");
  sandcastleAlias = scAliasEnv ? std::make_optional(std::string(scAliasEnv))
                               : sandcastleAlias;

  // check specified predictive parameters
  const auto& predictiveGlob = params->predictiveGlob();
  if (predictiveGlob.has_value()) {
    numResults = predictiveGlob->numTopDirectories().value_or(numResults);
    user = predictiveGlob->user().has_value() ? predictiveGlob->user().value()
                                              : user;
    repo = predictiveGlob->repo().has_value() ? predictiveGlob->repo().value()
                                              : repo;
    os = predictiveGlob->os().has_value() ? predictiveGlob->os().value() : os;
    startTime = predictiveGlob->startTime().has_value()
        ? predictiveGlob->startTime().value()
        : startTime;
    endTime = predictiveGlob->endTime().has_value()
        ? predictiveGlob->endTime().value()
        : endTime;
  }

  auto& fetchContext = helper->getPrefetchFetchContext();
  bool isBackground = *params->background();

  auto future =
      ImmediateFuture{
          usageService_->getTopUsedDirs(
              user, repo, numResults, os, startTime, endTime, sandcastleAlias)}
          .thenValue([globber = std::move(globber),
                      mountHandle,
                      serverState,
                      fetchContext = fetchContext.copy()](
                         std::vector<std::string>&& globs) mutable {
            return globber.glob(
                mountHandle.getEdenMountPtr(),
                serverState,
                globs,
                fetchContext);
          })
          .thenTry([mountHandle,
                    params = std::move(params),
                    helper = std::move(helper)](
                       folly::Try<std::unique_ptr<Glob>> tryGlob) {
            if (tryGlob.hasException()) {
              auto& ew = tryGlob.exception();
              XLOGF(
                  ERR,
                  "Error fetching predictive file globs: {}",
                  folly::exceptionStr(ew));
            }
            return tryGlob;
          });

  // The glob code has a very large fan-out that can easily overload the
  // Thrift CPU worker pool. To combat with that, we limit the execution to a
  // single thread by using `folly::SerialExecutor` so the glob queries will
  // not overload the executor.
  return serialDetachIfBackgrounded<Glob>(
      std::move(future), server_, isBackground);
}

folly::SemiFuture<std::unique_ptr<Glob>>
EdenServiceHandler::semifuture_globFiles(std::unique_ptr<GlobParams> params) {
  TaskTraceBlock block{"EdenServiceHandler::globFiles"};
  auto mountHandle = lookupMount(params->mountPoint());
  if (!params->revisions().value().empty()) {
    params->revisions() =
        resolveRootsWithLastFilter(params->revisions().value(), mountHandle);
  }
  ThriftGlobImpl globber{*params};
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint(),
      toLogArg(*params->globs()),
      globber.logString());
  auto& context = helper->getFetchContext();
  auto isBackground = *params->background();

  ImmediateFuture<folly::Unit> backgroundFuture{std::in_place};
  if (isBackground) {
    backgroundFuture = makeNotReadyImmediateFuture();
  }

  maybeLogExpensiveGlob(
      *params->globs(),
      *params->searchRoot(),
      globber,
      context,
      server_->getServerState());

  std::unique_ptr<SuffixGlobRequestScope> suffixGlobRequestScope;
  auto edenConfig = server_->getServerState()->getEdenConfig();

  ImmediateFuture<unique_ptr<Glob>> globFut{std::in_place};

  // Offload suffix queries to EdenAPI
  bool useSaplingRemoteAPISuffixes = shouldUseSaplingRemoteAPI(
      edenConfig->enableEdenAPISuffixQuery.getValue(), *params);

  // Matches **/*.suffix
  // Captures the .suffix
  static const re2::RE2 suffixRegex("\\*\\*/\\*(\\.[A-z0-9]+)");
  std::vector<std::string> suffixGlobs;
  std::vector<std::string> nonSuffixGlobs;

  // Copying to new vectors, since we want to keep the original around
  // in case we need to fall back to the legacy pathway
  for (const auto& glob : *params->globs()) {
    std::string capture;
    if (re2::RE2::FullMatch(glob, suffixRegex, &capture)) {
      suffixGlobs.push_back(capture);
    } else {
      nonSuffixGlobs.push_back(glob);
    }
  }

  bool requestIsOffloadable = !suffixGlobs.empty() && nonSuffixGlobs.empty() &&
      isValidSearchRoot(*params->searchRoot());

  // Allow only specific queries that have been determined to operate faster
  // when offloaded
  requestIsOffloadable = requestIsOffloadable &&
      checkAllowedQuery(suffixGlobs,
                        edenConfig->allowedSuffixQueries.getValue());

  auto globFilesRequestScope = std::make_shared<GlobFilesRequestScope>(
      server_->getServerState(),
      requestIsOffloadable,
      globber.logString(*params->globs()),
      context);

  if (requestIsOffloadable) {
    XLOG(
        DBG4,
        "globFiles request is only suffix globs, can be offloaded to EdenAPI");
    auto suffixGlobLogString = globber.logString(suffixGlobs);
    suffixGlobRequestScope = std::make_unique<SuffixGlobRequestScope>(
        suffixGlobLogString,
        server_->getServerState(),
        !useSaplingRemoteAPISuffixes,
        context);
  }

  if (useSaplingRemoteAPISuffixes) {
    if (requestIsOffloadable) {
      XLOG(DBG4, "globFiles request offloaded to EdenAPI");
      // Only use BSSM if there are only suffix queries
      globFilesRequestScope->setLocal(false);
      // Attempt to resolve all EdenAPI futures. If any of
      // them result in an error we will fall back to local lookup

      auto searchRoot = params->searchRoot().value();
      size_t pos = 0;
      while ((pos = searchRoot.find('\\', pos)) != std::string::npos) {
        searchRoot.replace(pos, 1, "/");
      }

      auto combinedFuture =
          std::move(backgroundFuture)
              .thenValue([revisions = params->revisions().value(),
                          mountHandle,
                          suffixGlobs = std::move(suffixGlobs),
                          searchRoot,
                          serverState = server_->getServerState(),
                          includeDotfiles = *params->includeDotfiles(),
                          context = context.copy()](auto&&) mutable {
                auto& store = mountHandle.getObjectStore();
                const auto& edenMount = mountHandle.getEdenMountPtr();
                const auto& rootInode = mountHandle.getRootInode();

                std::vector<std::string> prefixes;
                // Despite the API supporting multiple prefixes, we only use one
                // derived from the search root
                if (!searchRoot.empty() && searchRoot != ".") {
                  prefixes.push_back(searchRoot);
                }

                if (revisions.empty()) {
                  return getLocalGlobResults(
                      edenMount,
                      serverState,
                      includeDotfiles,
                      suffixGlobs,
                      prefixes,
                      rootInode,
                      context);
                }
                std::vector<ImmediateFuture<BackingStore::GetGlobFilesResult>>
                    globFilesResultFutures;
                for (auto& id : revisions) {
                  // ID is either a 20b binary hash or a 40b human readable
                  // text version globFiles takes as input the human readable
                  // version, so convert using the store's parse method
                  globFilesResultFutures.push_back(store.getGlobFiles(
                      store.parseRootId(id), suffixGlobs, prefixes, context));
                }
                return collectAllSafe(std::move(globFilesResultFutures));
              });

      globFut =
          std::move(combinedFuture)
              .thenValue([mountHandle,
                          rootInode = mountHandle.getRootInode(),
                          wantDtype = params->wantDtype().value(),
                          includeDotfiles = params->includeDotfiles().value(),
                          searchRoot,
                          &context](auto&& globResults) mutable {
                auto edenMount = mountHandle.getEdenMountPtr();
                std::vector<ImmediateFuture<GlobEntry>> globEntryFuts;
                for (auto& glob : globResults) {
                  std::string originId =
                      mountHandle.getObjectStore().renderRootId(glob.rootId);
                  for (auto& entry : glob.globFiles) {
                    if (!includeDotfiles) {
                      bool skip_due_to_dotfile = false;
                      auto rp = RelativePath(std::string_view{entry});
                      for (auto component : rp.components()) {
                        // Use facebook::eden string_view
                        if (string_view{component.view()}.starts_with(".")) {
                          XLOGF(
                              DBG5,
                              "Skipping dotfile: {} in {}",
                              component.view(),
                              entry);
                          skip_due_to_dotfile = true;
                          break;
                        }
                      }
                      if (skip_due_to_dotfile) {
                        continue;
                      }
                    }

                    if (wantDtype) {
                      ImmediateFuture<GlobEntry> entryFuture{std::in_place};
                      if (glob.isLocal) {
                        entryFuture =
                            rootInode
                                ->getChildRecursive(
                                    RelativePathPiece{entry}, context)
                                .thenValue([entry,
                                            originId](InodePtr child) mutable {
                                  return ImmediateFuture<GlobEntry>{GlobEntry{
                                      std::move(entry),
                                      static_cast<OsDtype>(child->getType()),
                                      std::move(originId)}};
                                });
                      } else {
                        // TODO(T192408118) get the root tree a single time
                        // per glob instead of per-entry
                        entryFuture =
                            edenMount->getObjectStore()
                                ->getRootTree(
                                    std::move(glob.rootId), context.copy())
                                .thenValue([entry,
                                            edenMount,
                                            context = context.copy()](
                                               auto&& tree) mutable {
                                  auto stringPiece = folly::StringPiece{entry};
                                  return ::facebook::eden::getTreeOrTreeEntry(
                                      std::move(tree.tree),
                                      RelativePath{stringPiece},
                                      edenMount->getObjectStore(),
                                      std::move(context));
                                })
                                .thenValue([entry, originId](
                                               auto&& treeEntry) mutable {
                                  if (TreeEntry* treeEntryPtr =
                                          std::get_if<TreeEntry>(&treeEntry)) {
                                    auto dtype = treeEntryPtr->getDtype();
                                    return ImmediateFuture<GlobEntry>{GlobEntry{
                                        std::move(entry),
                                        static_cast<OsDtype>(dtype),
                                        std::move(originId)}};
                                  } else {
                                    EDEN_BUG()
                                        << "Received a Tree when expecting TreeEntry for path "
                                        << entry;
                                  }
                                });
                      }
                      globEntryFuts.emplace_back(
                          std::move(entryFuture)
                              .thenError([entry,
                                          originId,
                                          isLocal = glob.isLocal](
                                             const folly::exception_wrapper&
                                                 ex) mutable {
                                XLOGF(
                                    ERR,
                                    "Error for getting file dtypes for {} file {}: {}",
                                    isLocal ? "local" : "remote",
                                    entry,
                                    ex.what());
                                return ImmediateFuture<GlobEntry>{GlobEntry{
                                    std::move(entry),
                                    DT_UNKNOWN,
                                    std::move(originId)}};
                              }));
                    } else {
                      globEntryFuts.emplace_back(ImmediateFuture<GlobEntry>{
                          folly::Try<GlobEntry>{GlobEntry{
                              std::move(entry), DT_UNKNOWN, originId}}});
                    }
                  }
                }
                return collectAllSafe(std::move(globEntryFuts))
                    .thenValue([searchRoot,
                                wantDtype](auto&& globEntries) mutable {
                      // Windows
                      XLOGF(
                          DBG5, "Building Glob with searchroot {}", searchRoot);
                      auto glob = std::make_unique<Glob>();
                      std::sort(
                          globEntries.begin(),
                          globEntries.end(),
                          [](GlobEntry a, GlobEntry b) {
                            return a.file < b.file;
                          });
                      for (GlobEntry& globEntry : globEntries) {
                        // Check that the files match the relative root,
                        // if there is one
                        std::string filePath = globEntry.file;
                        if (!searchRoot.empty() && searchRoot != ".") {
                          // If the file is in the relative root, remove the
                          // prefix and pass it through. Otherwise drop it
                          if (filePath.rfind(searchRoot, 0) == 0) {
                            // Remove the prefix and the leading /
                            filePath = filePath.substr(searchRoot.length() + 1);
                          } else {
                            continue;
                          }
                        }
                        glob->matchingFiles().value().emplace_back(
                            std::move(filePath));
                        if (wantDtype) {
                          // This can happen if a file is missing on disk but
                          // exists on the server. Instead of silently failing
                          // use the local lookup.
                          if (globEntry.dType == DT_UNKNOWN) {
                            throw newEdenError(
                                ENOENT,
                                EdenErrorType::POSIX_ERROR,
                                "could not get Dtype for file ",
                                globEntry.file);
                          }
                          glob->dtypes().value().emplace_back(globEntry.dType);
                        }
                        glob->originHashes().value().emplace_back(
                            globEntry.originId);
                      }
                      XLOG(
                          DBG5,
                          "Glob successfully created, returning SaplingRemoteAPI results");
                      return glob;
                    });
              })
              .thenError(
                  [mountHandle,
                   globFilesRequestScope,
                   serverState = server_->getServerState(),
                   globs = std::move(*params->globs()),
                   globber = std::move(globber),
                   &context](const folly::exception_wrapper& ex) mutable {
                    // Fallback to local if an error was encountered while using
                    // the SaplingRemoteAPI method
                    XLOGF(
                        DBG3,
                        "Encountered error when evaluating globFiles: {}",
                        ex.what());
                    XLOG(DBG3, "Using local globFiles");
                    globFilesRequestScope->setFallback(true);
                    return globber.glob(
                        mountHandle.getEdenMountPtr(),
                        serverState,
                        std::move(globs),
                        context);
                  });
    } else {
      globFut =
          std::move(backgroundFuture)
              .thenValue([mountHandle,
                          serverState = server_->getServerState(),
                          globs = std::move(*params->globs()),
                          globber = std::move(globber),
                          &context](auto&&) mutable {
                XLOG(DBG3, "No suffixes, or mixed suffixes and non-suffixes");
                XLOG(DBG3, "Using local globFiles");
                // TODO: Insert ODS log for globs here
                return globber.glob(
                    mountHandle.getEdenMountPtr(),
                    serverState,
                    std::move(globs),
                    context);
              });
    }
  } else {
    globFut = std::move(backgroundFuture)
                  .thenValue([mountHandle,
                              serverState = server_->getServerState(),
                              globs = std::move(*params->globs()),
                              globber = std::move(globber),
                              &context](auto&&) mutable {
                    XLOG(DBG3, "Using local globFiles");
                    // TODO: Insert ODS log for globs here
                    return globber.glob(
                        mountHandle.getEdenMountPtr(),
                        serverState,
                        std::move(globs),
                        context);
                  });
  }

  globFut = std::move(globFut).ensure(
      [mountHandle,
       helper = std::move(helper),
       params = std::move(params),
       suffixGlobRequestScope = std::move(suffixGlobRequestScope),
       globFilesRequestScope = std::move(globFilesRequestScope)] {});

  // The glob code has a very large fan-out that can easily overload the
  // Thrift CPU worker pool. To combat with that, we limit the execution to a
  // single thread by using `folly::SerialExecutor` so the glob queries will
  // not overload the executor.
  return serialDetachIfBackgrounded<Glob>(
      std::move(globFut), server_, isBackground);
}

// DEPRECATED. Use semifuture_prefetchFilesV2 instead.
folly::SemiFuture<folly::Unit> EdenServiceHandler::semifuture_prefetchFiles(
    std::unique_ptr<PrefetchParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());
  if (!params->revisions().value().empty()) {
    params->revisions() =
        resolveRootsWithLastFilter(params->revisions().value(), mountHandle);
  }
  ThriftGlobImpl globber{*params};
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2,
      *params->mountPoint(),
      toLogArg(*params->globs()),
      globber.logString());
  auto& context = helper->getFetchContext();
  auto isBackground = *params->background();

  ImmediateFuture<folly::Unit> backgroundFuture{std::in_place};
  if (isBackground) {
    backgroundFuture = makeNotReadyImmediateFuture();
  }

  maybeLogExpensiveGlob(
      *params->globs(),
      *params->searchRoot(),
      globber,
      context,
      server_->getServerState());

  auto globFut =
      std::move(backgroundFuture)
          .thenValue([mountHandle,
                      serverState = server_->getServerState(),
                      globs = std::move(*params->globs()),
                      globber = std::move(globber),
                      context = helper->getPrefetchFetchContext().copy()](
                         auto&&) mutable {
            return globber.glob(
                mountHandle.getEdenMountPtr(),
                serverState,
                std::move(globs),
                context);
          })
          .ensure([mountHandle] {})
          .thenValue([](std::unique_ptr<Glob>) { return folly::unit; });
  globFut = std::move(globFut).ensure(
      [helper = std::move(helper), params = std::move(params)] {});

  // The glob code has a very large fan-out that can easily overload the
  // Thrift CPU worker pool. To combat with that, we limit the execution to a
  // single thread by using `folly::SerialExecutor` so the glob queries will
  // not overload the executor.
  return serialDetachIfBackgrounded(std::move(globFut), server_, isBackground);
}

folly::SemiFuture<std::unique_ptr<PrefetchResult>>
EdenServiceHandler::semifuture_prefetchFilesV2(
    std::unique_ptr<PrefetchParams> params) {
  TaskTraceBlock block{"EdenServiceHandler::prefetchFilesV2"};
  auto mountHandle = lookupMount(params->mountPoint());
  if (!params->revisions().value().empty()) {
    params->revisions() =
        resolveRootsWithLastFilter(params->revisions().value(), mountHandle);
  }
  ThriftGlobImpl globber{*params};
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2,
      *params->mountPoint(),
      toLogArg(*params->globs()),
      globber.logString());
  auto& context = helper->getFetchContext();
  auto isBackground = *params->background();
  auto returnPrefetchedFiles = *params->returnPrefetchedFiles();

  ImmediateFuture<folly::Unit> backgroundFuture{std::in_place};
  if (isBackground) {
    backgroundFuture = makeNotReadyImmediateFuture();
  }

  maybeLogExpensiveGlob(
      *params->globs(),
      *params->searchRoot(),
      globber,
      context,
      server_->getServerState());

  auto globFut =
      std::move(backgroundFuture)
          .thenValue([mountHandle,
                      serverState = server_->getServerState(),
                      globs = std::move(*params->globs()),
                      globber = std::move(globber),
                      context = helper->getPrefetchFetchContext().copy()](
                         auto&&) mutable {
            return globber.glob(
                mountHandle.getEdenMountPtr(),
                serverState,
                std::move(globs),
                context);
          });

  // If returnPrefetchedFiles is set then return the list of globs
  auto prefetchResult = std::move(globFut)
                            .thenValue([returnPrefetchedFiles](
                                           std::unique_ptr<Glob> glob) mutable {
                              std::unique_ptr<PrefetchResult> result =
                                  std::make_unique<PrefetchResult>();
                              if (!returnPrefetchedFiles) {
                                return result;
                              }
                              result->prefetchedFiles() = std::move(*glob);
                              return result;
                            })
                            .ensure([mountHandle,
                                     helper = std::move(helper),
                                     params = std::move(params)] {});

  // The glob code has a very large fan-out that can easily overload the
  // Thrift CPU worker pool. To combat with that, we limit the execution to a
  // single thread by using `folly::SerialExecutor` so the glob queries will
  // not overload the executor.
  return serialDetachIfBackgrounded<PrefetchResult>(
      std::move(prefetchResult), server_, isBackground);
}

folly::SemiFuture<struct folly::Unit> EdenServiceHandler::semifuture_chown(
    [[maybe_unused]] std::unique_ptr<std::string> mountPoint,
    [[maybe_unused]] int32_t uid,
    [[maybe_unused]] int32_t gid) {
#ifndef _WIN32
  auto handle = lookupMount(mountPoint);
  return handle.getEdenMount().chown(uid, gid).ensure([handle] {}).semi();
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

folly::SemiFuture<std::unique_ptr<ChangeOwnershipResponse>>
EdenServiceHandler::semifuture_changeOwnership(
    unique_ptr<ChangeOwnershipRequest> request) {
#ifndef _WIN32
  auto handle = lookupMount(*request->mountPoint());
  return handle.getEdenMount()
      .chown(*request->uid(), *request->gid())
      .ensure([handle] {})
      .thenValue([](folly::Unit&&) {
        return std::make_unique<ChangeOwnershipResponse>();
      })
      .semi();
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

folly::SemiFuture<std::unique_ptr<GetScmStatusResult>>
EdenServiceHandler::semifuture_getScmStatusV2(
    unique_ptr<GetScmStatusParams> params) {
  auto* context = getRequestContext();
  auto rootIdOptions = params->rootIdOptions().ensure();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint(),
      folly::to<string>("commitHash=", logHash(*params->commit())),
      folly::to<string>("listIgnored=", *params->listIgnored()),
      folly::to<string>(
          "filterId=",
          rootIdOptions.filterId().has_value() ? *rootIdOptions.filterId()
                                               : "(none)"));
  helper->getThriftFetchContext().fillClientRequestInfo(params->cri());

  auto& fetchContext = helper->getFetchContext();

  auto mountHandle = lookupMount(params->mountPoint());

  // If we were passed a FilterID, create a RootID that contains the filter
  // and a varint that indicates the length of the original id.
  std::string parsedCommit =
      resolveRootId(std::move(*params->commit()), rootIdOptions, mountHandle);
  auto rootId = mountHandle.getObjectStore().parseRootId(parsedCommit);

  const auto& enforceParents = server_->getServerState()
                                   ->getReloadableConfig()
                                   ->getEdenConfig()
                                   ->enforceParents.getValue();
  return wrapImmediateFuture(
             std::move(helper),
             mountHandle.getEdenMount()
                 .diff(
                     mountHandle.getRootInode(),
                     rootId,
                     context->getConnectionContext()->getCancellationToken(),
                     fetchContext,
                     *params->listIgnored(),
                     enforceParents)
                 .ensure([mountHandle] {})
                 .thenValue([this](std::unique_ptr<ScmStatus>&& status) {
                   auto result = std::make_unique<GetScmStatusResult>();
                   result->status() = std::move(*status);
                   result->version() = server_->getVersion();
                   return result;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<ScmStatus>>
EdenServiceHandler::semifuture_getScmStatus(
    unique_ptr<string> mountPoint,
    bool listIgnored,
    unique_ptr<string> commitHash) {
  auto* context = getRequestContext();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2,
      *mountPoint,
      folly::to<string>("listIgnored=", listIgnored ? "true" : "false"),
      folly::to<string>("commitHash=", logHash(*commitHash)));
  auto& fetchContext = helper->getFetchContext();

  // Unlike getScmStatusV2(), this older getScmStatus() call does not enforce
  // that the caller specified the current commit.  In the future we might
  // want to enforce that even for this call, if we confirm that all existing
  // callers of this method can deal with the error.
  auto mountHandle = lookupMount(mountPoint);

  // parseRootId assumes that the passed in id will contain information
  // about the active filter. This legacy code path does not respect filters,
  // so the last active filter will always be passed in if it exists. For
  // non-FFS repos, the last filterID will be std::nullopt.
  std::string parsedCommit =
      resolveRootIdWithLastFilter(std::move(*commitHash), mountHandle);
  auto id = mountHandle.getObjectStore().parseRootId(parsedCommit);
  return wrapImmediateFuture(
             std::move(helper),
             mountHandle.getEdenMount().diff(
                 mountHandle.getRootInode(),
                 id,
                 context->getConnectionContext()->getCancellationToken(),
                 fetchContext,
                 listIgnored,
                 /*enforceCurrentParent=*/false))
      .ensure([mountHandle] {})
      .semi();
}

folly::SemiFuture<unique_ptr<ScmStatus>>
EdenServiceHandler::semifuture_getScmStatusBetweenRevisions(
    unique_ptr<string> mountPoint,
    unique_ptr<string> oldHash,
    unique_ptr<string> newHash) {
  auto* context = getRequestContext();
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2,
      *mountPoint,
      folly::to<string>("oldHash=", logHash(*oldHash)),
      folly::to<string>("newHash=", logHash(*newHash)));
  auto mountHandle = lookupMount(mountPoint);
  auto& fetchContext = helper->getFetchContext();

  // parseRootId assumes that the passed in id will contain information
  // about the active filter. This legacy code path does not respect filters,
  // so the last active filter will always be passed in if it exists. For
  // non-FFS repos, the last filterID will be std::nullopt.
  std::string resolvedOldId =
      resolveRootIdWithLastFilter(std::move(*oldHash), mountHandle);
  std::string resolvedNewId =
      resolveRootIdWithLastFilter(std::move(*newHash), mountHandle);

  auto callback = std::make_unique<ScmStatusDiffCallback>();
  auto diffFuture = diffBetweenRoots(
      mountHandle.getObjectStore().parseRootId(resolvedOldId),
      mountHandle.getObjectStore().parseRootId(resolvedNewId),
      *mountHandle.getEdenMount().getCheckoutConfig(),
      mountHandle.getObjectStorePtr(),
      context->getConnectionContext()->getCancellationToken(),
      fetchContext,
      callback.get());
  return wrapImmediateFuture(
             std::move(helper),
             std::move(diffFuture)
                 .thenValue([callback = std::move(callback)](auto&&) {
                   return std::make_unique<ScmStatus>(
                       callback->extractStatus());
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<MatchFileSystemResponse>>
EdenServiceHandler::semifuture_matchFilesystem(
    std::unique_ptr<MatchFileSystemRequest> params) {
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG2, *params->mountPoint(), *params->paths());
#ifdef _WIN32
  auto mountHandle = lookupMount(params->mountPoint()->mountPoint());
  if (auto* prjfsChannel = mountHandle.getEdenMount().getPrjfsChannel()) {
    std::vector<ImmediateFuture<folly::Unit>> results;
    results.reserve(params->paths()->size());
    for (auto& path : *params->paths()) {
      results.push_back(prjfsChannel->matchEdenViewOfFileToFS(
          relpathFromUserPath(path), helper->getFetchContext()));
    }
    return wrapImmediateFuture(
               std::move(helper),
               ImmediateFuture{
                   collectAll(std::move(results))
                       .ensure([mountHandle]() {})
                       .thenValue([](std::vector<folly::Try<folly::Unit>>
                                         raw_results) {
                         std::vector<MatchFilesystemPathResult> results;
                         results.reserve(raw_results.size());
                         for (auto& raw_result : raw_results) {
                           MatchFilesystemPathResult result{};
                           if (raw_result.hasException()) {
                             result.error() =
                                 newEdenError(raw_result.exception());
                           }
                           results.push_back(std::move(result));
                         }
                         auto final_result =
                             std::make_unique<MatchFileSystemResponse>();
                         final_result->results() = std::move(results);
                         return final_result;
                       })})
        .semi();
  }
#endif
  throw newEdenError(
      ENOTSUP,
      EdenErrorType::POSIX_ERROR,
      "matchFilesystemStat only supported for PrjFs repos which {} is not",
      *params->mountPoint());
}

void EdenServiceHandler::debugGetScmTree(
    vector<ScmTreeEntry>& entries,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint, logHash(*idStr));
  auto mountHandle = lookupMount(mountPoint);
  auto& store = mountHandle.getObjectStore();
  auto id = store.parseObjectId(*idStr);

  std::shared_ptr<const Tree> tree;
  if (localStoreOnly) {
    auto localStore = server_->getLocalStore();
    tree = localStore->getTree(id).get();
  } else {
    tree = store.getTree(id, helper->getFetchContext()).get();
  }

  if (!tree) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no tree found for id ",
        store.renderObjectId(id));
  }

  for (const auto& entry : *tree) {
    const auto& [name, treeEntry] = entry;
    entries.emplace_back();
    auto& out = entries.back();
    out.name() = name.asString();
    out.mode() = modeFromTreeEntryType(treeEntry.getType());
    out.id() = store.renderObjectId(treeEntry.getObjectId());
  }
}

folly::SemiFuture<std::unique_ptr<DebugGetScmBlobResponse>>
EdenServiceHandler::semifuture_debugGetBlob(
    std::unique_ptr<DebugGetScmBlobRequest> request) {
  const auto& mountid = request->mountId();
  const auto& idStr = request->id();
  const auto& origins = request->origins();
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG2, *mountid, logHash(*idStr), *origins);

  auto mountHandle = lookupMount(*mountid);
  auto edenMount = mountHandle.getEdenMountPtr();
  auto id = edenMount->getObjectStore()->parseObjectId(*idStr);
  auto originFlags = DataFetchOriginFlags::raw(*origins);
  auto store = edenMount->getObjectStore();

  std::vector<ImmediateFuture<ScmBlobWithOrigin>> blobFutures;

  if (originFlags.contains(FROMWHERE_MEMORY_CACHE)) {
    blobFutures.emplace_back(transformToBlobFromOrigin(
        edenMount,
        id,
        folly::Try<std::shared_ptr<const Blob>>{
            edenMount->getBlobCache()->get(id).object},
        DataFetchOrigin::MEMORY_CACHE));
  }
  if (originFlags.contains(FROMWHERE_DISK_CACHE)) {
    auto localStore = server_->getLocalStore();
    blobFutures.emplace_back(
        localStore->getBlob(id).thenTry([edenMount, id](auto&& blob) {
          return transformToBlobFromOrigin(
              edenMount, id, std::move(blob), DataFetchOrigin::DISK_CACHE);
        }));
  }

  auto& context = helper->getFetchContext();

  if (originFlags.contains(FROMWHERE_LOCAL_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetScmBlob",
        *server_->getServerState()->getStats());
    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());

    blobFutures.emplace_back(transformToBlobFromOrigin(
        edenMount,
        id,
        saplingBackingStore->getBlobLocal(proxyHash, context),
        DataFetchOrigin::LOCAL_BACKING_STORE));
  }
  if (originFlags.contains(FROMWHERE_REMOTE_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetScmBlob",
        *server_->getServerState()->getStats());
    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());
    blobFutures.emplace_back(transformToBlobFromOrigin(
        edenMount,
        id,
        saplingBackingStore->getBlobRemote(proxyHash, context),
        DataFetchOrigin::REMOTE_BACKING_STORE));
  }
  if (originFlags.contains(FROMWHERE_ANYWHERE)) {
    blobFutures.emplace_back(
        store->getBlob(id, context).thenTry([edenMount, id](auto&& blob) {
          return transformToBlobFromOrigin(
              edenMount, id, std::move(blob), DataFetchOrigin::ANYWHERE);
        }));
  }

  return wrapImmediateFuture(
             std::move(helper),
             collectAllSafe(std::move(blobFutures))
                 .thenValue([](std::vector<ScmBlobWithOrigin> blobs) {
                   auto response = std::make_unique<DebugGetScmBlobResponse>();
                   response->blobs() = std::move(blobs);
                   return response;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<DebugGetBlobMetadataResponse>>
EdenServiceHandler::semifuture_debugGetBlobMetadata(
    std::unique_ptr<DebugGetBlobMetadataRequest> request) {
  const auto& mountid = request->mountId();
  const auto& idStr = request->id();
  const auto& origins = request->origins();
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG2, *mountid, logHash(*idStr), *origins);

  auto mountHandle = lookupMount(*mountid);
  auto edenMount = mountHandle.getEdenMountPtr();
  auto id = edenMount->getObjectStore()->parseObjectId(*idStr);
  auto originFlags = DataFetchOriginFlags::raw(*origins);
  auto store = edenMount->getObjectStore();

  auto& fetchContext = helper->getFetchContext();

  std::vector<ImmediateFuture<BlobMetadataWithOrigin>> blobFutures;

  if (originFlags.contains(FROMWHERE_MEMORY_CACHE)) {
    auto auxData = store->getBlobAuxDataFromInMemoryCache(id, fetchContext);
    blobFutures.emplace_back(transformToBlobMetadataFromOrigin(
        edenMount, id, auxData, DataFetchOrigin::MEMORY_CACHE));
  }
  if (originFlags.contains(FROMWHERE_DISK_CACHE)) {
    auto localStore = server_->getLocalStore();
    blobFutures.emplace_back(
        localStore->getBlobAuxData(id).thenTry([edenMount, id](auto&& auxData) {
          return transformToBlobMetadataFromOrigin(
              edenMount,
              id,
              std::move(auxData.value()),
              DataFetchOrigin::DISK_CACHE);
        }));
  }
  if (originFlags.contains(FROMWHERE_LOCAL_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetScmBlob",
        *server_->getServerState()->getStats());
    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());

    auto auxData =
        saplingBackingStore->getLocalBlobAuxData(proxyHash).value_or(nullptr);

    blobFutures.emplace_back(transformToBlobMetadataFromOrigin(
        edenMount,
        id,
        std::move(auxData),
        DataFetchOrigin::LOCAL_BACKING_STORE));
  }
  if (originFlags.contains(FROMWHERE_REMOTE_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetScmBlob",
        *server_->getServerState()->getStats());
    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());

    blobFutures.emplace_back(
        ImmediateFuture{saplingBackingStore->getBlobAuxDataEnqueue(
                            id, proxyHash, fetchContext)}
            .thenValue([edenMount, id](BackingStore::GetBlobAuxResult result) {
              return transformToBlobMetadataFromOrigin(
                  edenMount,
                  id,
                  std::move(result.blobAux),
                  DataFetchOrigin::REMOTE_BACKING_STORE);
            }));
  }
  if (originFlags.contains(FROMWHERE_ANYWHERE)) {
    blobFutures.emplace_back(store->getBlobAuxData(id, fetchContext)
                                 .thenTry([edenMount, id](auto&& auxData) {
                                   return transformToBlobMetadataFromOrigin(
                                       std::move(auxData),
                                       DataFetchOrigin::ANYWHERE);
                                 }));
  }

  return wrapImmediateFuture(
             std::move(helper),
             collectAllSafe(std::move(blobFutures))
                 .thenValue([](std::vector<BlobMetadataWithOrigin> blobs) {
                   auto response =
                       std::make_unique<DebugGetBlobMetadataResponse>();
                   response->metadatas() = std::move(blobs);
                   return response;
                 }))
      .semi();
}

folly::SemiFuture<std::unique_ptr<DebugGetScmTreeResponse>>
EdenServiceHandler::semifuture_debugGetTree(
    std::unique_ptr<DebugGetScmTreeRequest> request) {
  const auto& mountid = request->mountId();
  const auto& idStr = request->id();
  const auto& origins = request->origins();
  auto helper =
      INSTRUMENT_THRIFT_CALL(DBG2, *mountid, logHash(*idStr), *origins);

  auto mountHandle = lookupMount(*mountid);
  auto edenMount = mountHandle.getEdenMountPtr();
  auto id = edenMount->getObjectStore()->parseObjectId(*idStr);
  auto originFlags = DataFetchOriginFlags::raw(*origins);
  auto store = edenMount->getObjectStore();

  std::vector<ImmediateFuture<ScmTreeWithOrigin>> treeFutures;

  if (originFlags.contains(FROMWHERE_MEMORY_CACHE)) {
    treeFutures.emplace_back(transformToTreeFromOrigin(
        edenMount,
        id,
        folly::Try<std::shared_ptr<const Tree>>{store->getTreeCache()->get(id)},
        DataFetchOrigin::MEMORY_CACHE));
  }

  if (originFlags.contains(FROMWHERE_DISK_CACHE)) {
    auto localStore = server_->getLocalStore();
    treeFutures.emplace_back(localStore->getTree(id).thenTry(
        [edenMount, id, store](auto&& tree) mutable {
          return transformToTreeFromOrigin(
              std::move(edenMount),
              id,
              std::move(tree),
              DataFetchOrigin::DISK_CACHE);
        }));
  }

  auto& context = helper->getFetchContext();

  if (originFlags.contains(FROMWHERE_LOCAL_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetTree",
        *server_->getServerState()->getStats());

    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());

    treeFutures.emplace_back(transformToTreeFromOrigin(
        edenMount,
        id,
        folly::Try<std::shared_ptr<const Tree>>{
            saplingBackingStore->getTreeLocal(id, context, proxyHash)},
        DataFetchOrigin::LOCAL_BACKING_STORE));
  }

  if (originFlags.contains(FROMWHERE_REMOTE_BACKING_STORE)) {
    auto proxyHash = HgProxyHash::load(
        server_->getLocalStore().get(),
        id,
        "debugGetTree",
        *server_->getServerState()->getStats());
    auto backingStore = edenMount->getObjectStore()->getBackingStore();
    std::shared_ptr<SaplingBackingStore> saplingBackingStore =
        castToSaplingBackingStore(backingStore, edenMount->getPath());
    treeFutures.emplace_back(transformToTreeFromOrigin(
        edenMount,
        id,
        saplingBackingStore->getTreeRemote(
            proxyHash.path().copy(), proxyHash.revHash(), id, context),
        DataFetchOrigin::REMOTE_BACKING_STORE));
  }

  if (originFlags.contains(FROMWHERE_ANYWHERE)) {
    treeFutures.emplace_back(store->getTree(id, helper->getFetchContext())
                                 .thenTry([edenMount, id](auto&& tree) mutable {
                                   return transformToTreeFromOrigin(
                                       std::move(edenMount),
                                       id,
                                       std::move(tree),
                                       DataFetchOrigin::ANYWHERE);
                                 }));
  }

  return wrapImmediateFuture(
             std::move(helper),
             collectAllSafe(std::move(treeFutures))
                 .thenValue([](std::vector<ScmTreeWithOrigin> trees) {
                   auto response = std::make_unique<DebugGetScmTreeResponse>();
                   response->trees() = std::move(trees);
                   return response;
                 }))
      .ensure([mountHandle] {})
      .semi();
}

namespace {
class InodeStatusCallbacks : public TraversalCallbacks {
 public:
  explicit InodeStatusCallbacks(
      EdenMount* mount,
      int64_t flags,
      std::vector<TreeInodeDebugInfo>& results)
      : mount_{mount}, flags_{flags}, results_{results} {}

  void visitTreeInode(
      RelativePathPiece path,
      InodeNumber ino,
      const std::optional<ObjectId>& id,
      uint64_t fsRefcount,
      const std::vector<ChildEntry>& entries) override {
#ifndef _WIN32
    auto* inodeMetadataTable = mount_->getInodeMetadataTable();
#endif

    TreeInodeDebugInfo info;
    info.inodeNumber() = ino.get();
    info.path() = path.asString();
    info.materialized() = !id.has_value();
    if (id.has_value()) {
      info.treeHash() = mount_->getObjectStore()->renderObjectId(id.value());
    }
    info.refcount() = fsRefcount;

    info.entries()->reserve(entries.size());

    for (auto& entry : entries) {
      TreeInodeEntryDebugInfo entryInfo;
      entryInfo.name() = entry.name.asString();
      entryInfo.inodeNumber() = entry.ino.get();

      // This could be enabled on Windows if InodeMetadataTable was removed.
#ifndef _WIN32
      if (auto metadata = (flags_ & eden_constants::DIS_COMPUTE_ACCURATE_MODE_)
              ? inodeMetadataTable->getOptional(entry.ino)
              : std::nullopt) {
        entryInfo.mode() = metadata->mode;
      } else {
        entryInfo.mode() = dtype_to_mode(entry.dtype);
      }
#else
      entryInfo.mode_ref() = dtype_to_mode(entry.dtype);
#endif

      entryInfo.loaded() = entry.loadedChild != nullptr;
      entryInfo.materialized() = !entry.id.has_value();
      if (entry.id.has_value()) {
        entryInfo.hash() =
            mount_->getObjectStore()->renderObjectId(entry.id.value());
      }

      if ((flags_ & eden_constants::DIS_COMPUTE_BLOB_SIZES_) &&
          dtype_t::Dir != entry.dtype) {
        if (entry.id.has_value()) {
          // schedule fetching size from ObjectStore::getBlobSize
          requestedSizes_.push_back(RequestedSize{
              results_.size(), info.entries()->size(), entry.id.value()});
        } else {
#ifndef _WIN32
          entryInfo.fileSize() = mount_->getOverlayFileAccess()->getFileSize(
              entry.ino, entry.loadedChild.get());
#else
          // This following code ends up doing a stat in the working
          // directory. This is safe to do as Windows works very differently
          // from Linux/macOS when dealing with materialized files. In this
          // code, we know that the file is materialized because we do not
          // have a id for it, and every materialized file is present on
          // disk and reading/stating it is guaranteed to be done without
          // EdenFS involvement. If somehow EdenFS is wrong, and this ends up
          // triggering a recursive call into EdenFS, we are detecting this
          // and simply bailing out very early in the callback.
          auto filePath = mount_->getPath() + path + entry.name;
          struct stat fileStat;
          if (::stat(filePath.c_str(), &fileStat) == 0) {
            entryInfo.fileSize_ref() = fileStat.st_size;
          } else {
            // Couldn't read the file, let's pretend it has a size of 0.
            entryInfo.fileSize_ref() = 0;
          }
#endif
        }
      }

      info.entries()->push_back(entryInfo);
    }

    results_.push_back(std::move(info));
  }

  bool shouldRecurse(const ChildEntry& entry) override {
    if (flags_ & eden_constants::DIS_NOT_RECURSIVE_) {
      return false;
    }

    if ((flags_ & eden_constants::DIS_REQUIRE_LOADED_) && !entry.loadedChild) {
      return false;
    }
    if ((flags_ & eden_constants::DIS_REQUIRE_MATERIALIZED_) &&
        entry.id.has_value()) {
      return false;
    }
    return true;
  }

  void fillBlobSizes(const ObjectFetchContextPtr& fetchContext) {
    std::vector<ImmediateFuture<folly::Unit>> futures;
    futures.reserve(requestedSizes_.size());
    for (auto& request : requestedSizes_) {
      futures.push_back(mount_->getObjectStore()
                            ->getBlobSize(request.id, fetchContext)
                            .thenValue([this, request](uint64_t blobSize) {
                              results_.at(request.resultIndex)
                                  .entries()
                                  ->at(request.entryIndex)
                                  .fileSize() = blobSize;
                            }));
    }
    collectAll(std::move(futures)).get();
  }

 private:
  struct RequestedSize {
    size_t resultIndex;
    size_t entryIndex;
    ObjectId id;
  };

  EdenMount* mount_;
  int64_t flags_;
  std::vector<TreeInodeDebugInfo>& results_;
  std::vector<RequestedSize> requestedSizes_;
};

} // namespace

void EdenServiceHandler::debugInodeStatus(
    vector<TreeInodeDebugInfo>& inodeInfo,
    unique_ptr<string> mountPoint,
    unique_ptr<std::string> path,
    int64_t flags,
    std::unique_ptr<SyncBehavior> sync) {
  if (0 == flags) {
    flags = eden_constants::DIS_REQUIRE_LOADED_ |
        eden_constants::DIS_COMPUTE_BLOB_SIZES_;
  }

  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2, *mountPoint, *path, flags, getSyncTimeout(*sync));
  auto mountHandle = lookupMount(mountPoint);

  waitForPendingWrites(mountHandle.getEdenMount(), *sync)
      .thenValue([mountHandle,
                  &inodeInfo,
                  path = std::move(path),
                  flags,
                  helper = std::move(helper)](auto&&) mutable {
        auto inode =
            inodeFromUserPath(
                mountHandle.getEdenMount(), *path, helper->getFetchContext())
                .asTreePtr();
        auto inodePath = inode->getPath().value();

        InodeStatusCallbacks callbacks{
            &mountHandle.getEdenMount(), flags, inodeInfo};
        traverseObservedInodes(*inode, inodePath, callbacks);
        callbacks.fillBlobSizes(helper->getFetchContext());
      })
      .ensure([mountHandle] {})
      .get();
}

void EdenServiceHandler::debugOutstandingFuseCalls(
    [[maybe_unused]] std::vector<FuseCall>& outstandingCalls,
    [[maybe_unused]] std::unique_ptr<std::string> mountPoint) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto mountHandle = lookupMount(mountPoint);

  if (auto* fuseChannel = mountHandle.getEdenMount().getFuseChannel()) {
    for (const auto& call : fuseChannel->getOutstandingRequests()) {
      outstandingCalls.push_back(populateFuseCall(
          call.unique,
          call.request,
          *server_->getServerState()->getProcessInfoCache()));
    }
  }
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServiceHandler::debugOutstandingNfsCalls(
    std::vector<NfsCall>& outstandingCalls,
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto mountHandle = lookupMount(mountPoint);

  if (auto* nfsdChannel = mountHandle.getEdenMount().getNfsdChannel()) {
    for (const auto& call : nfsdChannel->getOutstandingRequests()) {
      NfsCall nfsCall;
      nfsCall.xid() = call.xid;
      outstandingCalls.push_back(nfsCall);
    }
  }
}

void EdenServiceHandler::debugOutstandingPrjfsCalls(
    [[maybe_unused]] std::vector<PrjfsCall>& outstandingCalls,
    [[maybe_unused]] std::unique_ptr<std::string> mountPoint) {
#ifdef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto mountHandle = lookupMount(mountPoint);

  if (auto* prjfsChannel = mountHandle.getEdenMount().getPrjfsChannel()) {
    for (const auto& call :
         prjfsChannel->getInner()->getOutstandingRequests()) {
      outstandingCalls.push_back(populatePrjfsCall(call.type, call.data));
    }
  }
#else
  NOT_IMPLEMENTED();
#endif // _WIN32
}

void EdenServiceHandler::debugOutstandingThriftRequests(
    std::vector<ThriftRequestMetadata>& outstandingRequests) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  const auto requestsLockedPtr = outstandingThriftRequests_.rlock();
  for (const auto& item : *requestsLockedPtr) {
    outstandingRequests.emplace_back(
        populateThriftRequestMetadata(item.second));
  }
}

void EdenServiceHandler::debugOutstandingHgEvents(
    std::vector<HgEvent>& outstandingEvents,
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto mountHandle = lookupMount(mountPoint);

  auto backingStore = mountHandle.getObjectStore().getBackingStore();
  std::shared_ptr<SaplingBackingStore> saplingBackingStore =
      castToSaplingBackingStore(
          backingStore, mountHandle.getEdenMount().getPath());
  const auto hgEvents = saplingBackingStore->getOutstandingHgEvents();

  auto processInfoCache =
      mountHandle.getEdenMount().getServerState()->getProcessInfoCache();
  for (const auto& event : hgEvents) {
    HgEvent thriftEvent;
    convertHgImportTraceEventToHgEvent(event, *processInfoCache, thriftEvent);
    outstandingEvents.emplace_back(thriftEvent);
  }
}

void EdenServiceHandler::debugStartRecordingActivity(
    ActivityRecorderResult& result,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> outputDir) {
  AbsolutePathPiece path;
  try {
    path = absolutePathFromThrift(*outputDir);
  } catch (const std::exception&) {
    throw newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "path for output directory is invalid");
  }

  auto mountHandle = lookupMount(mountPoint);
  auto lockedPtr = mountHandle.getEdenMount().getActivityRecorder().wlock();
  // bool check on the wrapped pointer as lockedPtr is truthy as long
  // as we have the lock
  if (!lockedPtr->get()) {
    auto recorder =
        server_->makeActivityRecorder(mountHandle.getEdenMountPtr());
    lockedPtr->swap(recorder);
  }
  uint64_t unique = lockedPtr->get()->addSubscriber(path);
  // unique_ref is signed but overflow is very unlikely because unique is UNIX
  // timestamp in seconds.
  result.unique() = unique;
}

void EdenServiceHandler::debugStopRecordingActivity(
    ActivityRecorderResult& result,
    std::unique_ptr<std::string> mountPoint,
    int64_t unique) {
  auto mountHandle = lookupMount(mountPoint);
  auto lockedPtr = mountHandle.getEdenMount().getActivityRecorder().wlock();
  auto* activityRecorder = lockedPtr->get();
  if (!activityRecorder) {
    return;
  }

  auto outputPath = activityRecorder->removeSubscriber(unique);
  if (outputPath.has_value()) {
    result.unique() = unique;
    result.path() = outputPath.value();
  }

  if (activityRecorder->getSubscribers().empty()) {
    lockedPtr->reset();
  }
}

void EdenServiceHandler::debugListActivityRecordings(
    ListActivityRecordingsResult& result,
    std::unique_ptr<std::string> mountPoint) {
  auto mountHandle = lookupMount(mountPoint);
  auto lockedPtr = mountHandle.getEdenMount().getActivityRecorder().rlock();
  auto* activityRecorder = lockedPtr->get();
  if (!activityRecorder) {
    return;
  }

  std::vector<ActivityRecorderResult> recordings;
  auto subscribers = activityRecorder->getSubscribers();
  recordings.reserve(subscribers.size());
  for (auto const& subscriber : subscribers) {
    ActivityRecorderResult recording;
    recording.unique() = std::get<0>(subscriber);
    recording.path() = std::get<1>(subscriber);
    recordings.push_back(std::move(recording));
  }
  result.recordings() = recordings;
}

void EdenServiceHandler::debugGetInodePath(
    InodePathDebugInfo& info,
    std::unique_ptr<std::string> mountPoint,
    int64_t inodeNumber) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto inodeNum = static_cast<InodeNumber>(inodeNumber);
  auto mountHandle = lookupMount(mountPoint);
  auto inodeMap = mountHandle.getEdenMount().getInodeMap();

  auto relativePath = inodeMap->getPathForInode(inodeNum);
  // Check if the inode is loaded
  info.loaded() = inodeMap->lookupLoadedInode(inodeNum) != nullptr;
  // If getPathForInode returned none then the inode is unlinked
  info.linked() = relativePath != std::nullopt;
  info.path() = relativePath ? relativePath->asString() : "";
}

void EdenServiceHandler::clearFetchCounts() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  for (auto& handle : server_->getMountPoints()) {
    handle.getObjectStore().clearFetchCounts();
  }
}

void EdenServiceHandler::clearFetchCountsByMount(
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto mount = lookupMount(mountPoint);
  mount.getObjectStore().clearFetchCounts();
}

void EdenServiceHandler::startRecordingBackingStoreFetch() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  for (auto& backingStore : server_->getBackingStores()) {
    backingStore->startRecordingFetch();
  }
}

void EdenServiceHandler::stopRecordingBackingStoreFetch(
    GetFetchedFilesResult& results) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  for (const auto& backingStore : server_->getBackingStores()) {
    auto filePaths = backingStore->stopRecordingFetch();

    std::shared_ptr<SaplingBackingStore> saplingBackingStore{nullptr};

    // If FilteredFS is enabled, we'll see a FilteredBackingStore first
    auto filteredBackingStore =
        std::dynamic_pointer_cast<FilteredBackingStore>(backingStore);
    if (filteredBackingStore) {
      // FilteredBackingStore -> SaplingBackingStore
      saplingBackingStore = std::dynamic_pointer_cast<SaplingBackingStore>(
          filteredBackingStore->getBackingStore());
    } else {
      // BackingStore -> SaplingBackingStore
      saplingBackingStore =
          std::dynamic_pointer_cast<SaplingBackingStore>(backingStore);
    }

    // recording is only implemented for SaplingBackingStore at the moment
    if (saplingBackingStore) {
      (*results.fetchedFilePaths())["SaplingBackingStore"].insert(
          filePaths.begin(), filePaths.end());
    }
  }
} // namespace eden

void EdenServiceHandler::getAccessCounts(
    GetAccessCountsResult& result,
    int64_t duration) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  result.cmdsByPid() =
      server_->getServerState()->getProcessInfoCache()->getAllProcessNames();

  auto seconds = std::chrono::seconds{duration};

  for (auto& handle : server_->getMountPoints()) {
    auto& mount = handle.getEdenMount();
    auto& mountStr = mount.getPath().value();
    auto& pal = mount.getProcessAccessLog();

    auto& pidFetches = mount.getObjectStore()->getPidFetches();

    MountAccesses& ma = result.accessesByMount()[mountStr];
    for (auto& [pid, accessCounts] : pal.getAccessCounts(seconds)) {
      ma.accessCountsByPid()[pid] = accessCounts;
    }

    auto pidFetchesLockedPtr = pidFetches.rlock();
    for (auto& [pid, fetchCount] : *pidFetchesLockedPtr) {
      ma.fetchCountsByPid()[pid.get()] = fetchCount;
    }
  }
}

void EdenServiceHandler::clearAndCompactLocalStore() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1);
  server_->getLocalStore()->clearCachesAndCompactAll();
}

void EdenServiceHandler::debugClearLocalStoreCaches() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1);
  server_->getLocalStore()->clearCaches();
}

void EdenServiceHandler::debugCompactLocalStorage() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1);
  server_->getLocalStore()->compactStorage();
}

// TODO(T119221752): add more BackingStore subclasses to this command. We
// currently only support SaplingBackingStores
int64_t EdenServiceHandler::debugDropAllPendingRequests() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1);
  auto stores = server_->getSaplingBackingStores();
  int64_t numDropped = 0;
  for (auto& store : stores) {
    numDropped += store->dropAllPendingRequestsFromQueue();
  }
  return numDropped;
}

int64_t EdenServiceHandler::unloadInodeForPath(
    [[maybe_unused]] unique_ptr<string> mountPoint,
    [[maybe_unused]] std::unique_ptr<std::string> path,
    [[maybe_unused]] std::unique_ptr<TimeSpec> age) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1, *mountPoint, *path);
  auto mountHandle = lookupMount(mountPoint);

  TreeInodePtr inode =
      inodeFromUserPath(
          mountHandle.getEdenMount(), *path, helper->getFetchContext())
          .asTreePtr();
  auto cutoff = std::chrono::system_clock::now() -
      std::chrono::seconds(*age->seconds()) -
      std::chrono::nanoseconds(*age->nanoSeconds());
  auto cutoff_ts = folly::to<timespec>(cutoff);
  return inode->unloadChildrenLastAccessedBefore(cutoff_ts);
#else
  NOT_IMPLEMENTED();
#endif
}

folly::SemiFuture<std::unique_ptr<DebugInvalidateResponse>>
EdenServiceHandler::semifuture_debugInvalidateNonMaterialized(
    std::unique_ptr<DebugInvalidateRequest> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1, *params->mount()->mountPoint());
  auto mountHandle = lookupMount(params->mount()->mountPoint());
  auto& fetchContext = helper->getFetchContext();

  // TODO: We may need to restrict 0s age as that can lead to
  // weird behavior where files are invalidated while being read causing the
  // read to fail.

  auto cutoff = std::chrono::system_clock::time_point::max();
  if (*params->age()->seconds() != 0) {
    cutoff = std::chrono::system_clock::now() -
        std::chrono::seconds(*params->age()->seconds());
  }

  ImmediateFuture<folly::Unit> backgroundFuture{std::in_place};
  if (*params->background()) {
    backgroundFuture = makeNotReadyImmediateFuture();
  }

  auto invalFut =
      std::move(backgroundFuture)
          .thenValue([mountHandle, sync = *params->sync()](auto&&) {
            return waitForPendingWrites(mountHandle.getEdenMount(), sync);
          })
          .thenValue(
              [mountHandle, path = *params->path(), &fetchContext](auto&&) {
                return inodeFromUserPath(
                           mountHandle.getEdenMount(), path, fetchContext)
                    .asTreePtr();
              })
          .thenValue([this, mountHandle, cutoff, &fetchContext](
                         TreeInodePtr inode) mutable {
            return server_->garbageCollectWorkingCopy(
                mountHandle.getEdenMount(), inode, cutoff, fetchContext);
          })
          .thenValue([](uint64_t numInvalidated) {
            auto ret = std::make_unique<DebugInvalidateResponse>();
            ret->numInvalidated() = numInvalidated;
            return ret;
          })
          .ensure([helper = std::move(helper), mountHandle] {});

  if (!*params->background()) {
    return std::move(invalFut).semi();
  } else {
    folly::futures::detachOn(
        server_->getServerState()->getThreadPool().get(),
        std::move(invalFut).semi());
    return std::make_unique<DebugInvalidateResponse>();
  }
}

folly::SemiFuture<std::unique_ptr<GetFileContentResponse>>
EdenServiceHandler::semifuture_getFileContent(
    std::unique_ptr<GetFileContentRequest> request) {
  // Read from request
  auto sync = request->sync();
  auto mountPoint = request->mount()->mountPoint();
  auto filePath = request->filePath();

  // Set up log helper
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, getSyncTimeout(*sync), *filePath);

  // Prepare params for querying
  auto mountHandle = lookupMount(mountPoint);
  auto path = RelativePathPiece(*filePath);
  auto& fetchContext = helper->getFetchContext();

  // Ensure Eden has its internal state updated.
  // See SyncBehavior struct in eden.thrift for details.
  auto fut = waitForPendingWrites(mountHandle.getEdenMount(), *request->sync());

  return wrapImmediateFuture(
             std::move(helper),
             std::move(fut)
                 .thenValue([mountHandle,
                             path = path.copy(),
                             fetchContext = fetchContext.copy()](auto&&) {
                   auto& edenMount = mountHandle.getEdenMount();
                   return edenMount.getVirtualInode(path, fetchContext);
                 })
                 .thenValue([mountHandle,
                             fetchContext = fetchContext.copy()](auto&& inode) {
                   auto& objectStore = mountHandle.getObjectStorePtr();
                   return inode.getBlob(objectStore, fetchContext);
                 })
                 .thenTry([path = path.copy()](auto&& result) {
                   ScmBlobOrError blobOrError;
                   if (result.hasException()) {
                     blobOrError.error() = newEdenError(result.exception());
                   } else {
                     // Return error if the binary size exceeds 2GB limit.
                     // Enforced by CompactProtocolWriter in the Thrift
                     // https://github.com/facebook/fbthrift/blob/main/thrift/lib/cpp2/protocol/CompactProtocol-inl.h
                     const auto blobSize = result.value().size();
                     if (blobSize > std::numeric_limits<int32_t>::max()) {
                       blobOrError.error() = newEdenError(
                           EFBIG,
                           EdenErrorType::POSIX_ERROR,
                           "Thrift size limit (2GB) exceeded by file: ",
                           path);
                     } else {
                       blobOrError.blob() = std::move(result.value());
                     }
                   }
                   auto response = std::make_unique<GetFileContentResponse>();
                   response->blob() = std::move(blobOrError);
                   return response;
                 }))
      .semi();
}

void EdenServiceHandler::listRedirections(
    ListRedirectionsResponse& response,
    std::unique_ptr<ListRedirectionsRequest> request) {
  auto mountId = request->mount();
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountId);

  const auto& configDir = server_->getEdenDir();
  const auto& edenEtcDir =
      server_->getServerState()->getEdenConfig()->getSystemConfigDir();

  auto redirsFFI = list_redirections(
      absolutePathFromThrift(*mountId->mountPoint()).stringWithoutUNC(),
      configDir.stringWithoutUNC(),
      edenEtcDir.stringWithoutUNC());

  std::vector<Redirection> redirs(redirsFFI.size());
  std::transform(
      redirsFFI.begin(), redirsFFI.end(), redirs.begin(), [](auto&& redirFFI) {
        return redirectionFromFFI(std::move(redirFFI));
      });

  response.redirections() = std::move(redirs);
}

void EdenServiceHandler::getStatInfo(
    InternalStats& result,
    std::unique_ptr<GetStatInfoParams> params) {
  int64_t statsMask = *params->statsMask();
  // return all stats when mask not provided
  // TODO: remove when no old clients exists
  if (0 == statsMask) {
    statsMask = ~0;
  }

  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  if (statsMask & eden_constants::STATS_MOUNTS_STATS_) {
    auto mountList = server_->getMountPoints();
    std::map<PathString, MountInodeInfo> mountPointInfo = {};
    std::map<PathString, JournalInfo> mountPointJournalInfo = {};
    for (auto& handle : mountList) {
      auto& mount = handle.getEdenMount();
      auto inodeMap = mount.getInodeMap();
      // Set LoadedInde Count and unloaded Inode count for the mountPoint.
      MountInodeInfo mountInodeInfo;
      auto counts = inodeMap->getInodeCounts();
      mountInodeInfo.unloadedInodeCount() = counts.unloadedInodeCount;
      mountInodeInfo.loadedFileCount() = counts.fileCount;
      mountInodeInfo.loadedTreeCount() = counts.treeCount;

      JournalInfo journalThrift;
      if (auto journalStats = mount.getJournal().getStats()) {
        journalThrift.entryCount() = journalStats->entryCount;
        journalThrift.durationSeconds() = journalStats->getDurationInSeconds();
      } else {
        journalThrift.entryCount() = 0;
        journalThrift.durationSeconds() = 0;
      }
      journalThrift.memoryUsage() = mount.getJournal().estimateMemoryUsage();

      auto mountPath = absolutePathToThrift(mount.getPath());
      mountPointJournalInfo[mountPath] = journalThrift;

      mountPointInfo[mountPath] = mountInodeInfo;
    }
    result.mountPointInfo() = mountPointInfo;
    result.mountPointJournalInfo() = mountPointJournalInfo;
  }

  auto counters = fb303::ServiceData::get()->getCounters();
  if (statsMask & eden_constants::STATS_COUNTERS_) {
    // Get the counters and set number of inodes unloaded by periodic unload
    // job.
    result.counters() = counters;
    size_t periodicUnloadCount{0};
    for (auto& handle : server_->getMountPoints()) {
      auto& mount = handle.getEdenMount();
      periodicUnloadCount +=
          counters[mount.getCounterName(CounterName::PERIODIC_INODE_UNLOAD)];
    }

    result.periodicUnloadCount() = periodicUnloadCount;
  }

  if (statsMask & eden_constants::STATS_PRIVATE_BYTES_) {
    auto privateDirtyBytes = facebook::eden::proc_util::calculatePrivateBytes();
    if (privateDirtyBytes) {
      result.privateBytes() = privateDirtyBytes.value();
    }
  }

  if (statsMask & eden_constants::STATS_RSS_BYTES_) {
    auto memoryStats = facebook::eden::proc_util::readMemoryStats();
    if (memoryStats) {
      result.vmRSSBytes() = memoryStats->resident;
    }
  }

  if (statsMask & eden_constants::STATS_SMAPS_) {
    // Note: this will be removed in a subsequent commit.
    // We now report periodically via ServiceData
    std::string smaps;
    if (folly::readFile("/proc/self/smaps", smaps)) {
      result.smaps() = std::move(smaps);
    }
  }

  if (statsMask & eden_constants::STATS_CACHE_STATS_) {
    const auto blobCacheStats = server_->getBlobCache()->getStats(counters);
    result.blobCacheStats() = CacheStats{};
    result.blobCacheStats()->entryCount() = blobCacheStats.objectCount;
    result.blobCacheStats()->totalSizeInBytes() =
        blobCacheStats.totalSizeInBytes;
    result.blobCacheStats()->hitCount() = blobCacheStats.hitCount;
    result.blobCacheStats()->missCount() = blobCacheStats.missCount;
    result.blobCacheStats()->evictionCount() = blobCacheStats.evictionCount;
    result.blobCacheStats()->dropCount() = blobCacheStats.dropCount;

    const auto treeCacheStats = server_->getTreeCache()->getStats(counters);
    result.treeCacheStats() = CacheStats{};
    result.treeCacheStats()->entryCount() = treeCacheStats.objectCount;
    result.treeCacheStats()->totalSizeInBytes() =
        treeCacheStats.totalSizeInBytes;
    result.treeCacheStats()->hitCount() = treeCacheStats.hitCount;
    result.treeCacheStats()->missCount() = treeCacheStats.missCount;
    result.treeCacheStats()->evictionCount() = treeCacheStats.evictionCount;
    result.treeCacheStats()->dropCount() = treeCacheStats.dropCount;
  }
}

void EdenServiceHandler::flushStatsNow() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  server_->flushStatsNow();
}

folly::SemiFuture<Unit>
EdenServiceHandler::semifuture_invalidateKernelInodeCache(
    [[maybe_unused]] std::unique_ptr<std::string> mountPoint,
    [[maybe_unused]] std::unique_ptr<std::string> path) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint, *path);
  auto mountHandle = lookupMount(mountPoint);
#ifndef _WIN32
  InodePtr inode = inodeFromUserPath(
      mountHandle.getEdenMount(), *path, helper->getFetchContext());

  if (auto* fuseChannel = mountHandle.getEdenMount().getFuseChannel()) {
    // Invalidate cached pages and attributes
    fuseChannel->invalidateInode(inode->getNodeId(), 0, 0);

    const auto treePtr = inode.asTreePtrOrNull();

    // Invalidate all parent/child relationships potentially cached.
    if (treePtr != nullptr) {
      const auto& dir = treePtr->getContents().rlock();
      for (const auto& entry : dir->entries) {
        fuseChannel->invalidateEntry(inode->getNodeId(), entry.first);
      }
    }

    // Wait for all of the invalidations to complete
    return fuseChannel->completeInvalidations().semi();
  }

  if (auto* nfsChannel = mountHandle.getEdenMount().getNfsdChannel()) {
    inode->forceMetadataUpdate();
    auto& fetchContext = helper->getFetchContext();
    auto rawInodePtr = inode.get();
    return wrapImmediateFuture(
               std::move(helper),
               rawInodePtr->stat(fetchContext)
                   .thenValue(
                       [nfsChannel,
                        canonicalMountPoint =
                            absolutePathFromThrift(*mountPoint),
                        inode = std::move(inode),
                        path = std::move(path),
                        mountHandle,
                        fetchContext =
                            fetchContext.copy()](struct stat&& stat) mutable
                       -> ImmediateFuture<folly::Unit> {
                         nfsChannel->invalidate(
                             canonicalMountPoint + RelativePath{*path},
                             stat.st_mode);
                         const auto treePtr = inode.asTreePtrOrNull();
                         // Invalidate all children as well. There isn't
                         // really a way to invalidate the entry cache for nfs
                         // so we settle for invalidating the children
                         // themselves.
                         if (treePtr != nullptr) {
                           const auto& dir = treePtr->getContents().rlock();
                           std::vector<ImmediateFuture<folly::Unit>>
                               childInvalidations{};
                           for (const auto& entry : dir->entries) {
                             auto childPath = RelativePath{*path} + entry.first;
                             auto childInode = inodeFromUserPath(
                                 mountHandle.getEdenMount(),
                                 childPath.asString(),
                                 fetchContext);
                             childInode->forceMetadataUpdate();
                             childInvalidations.push_back(
                                 childInode->stat(fetchContext)
                                     .thenValue(
                                         [nfsChannel,
                                          canonicalMountPoint,
                                          childPath](struct stat&& stat) {
                                           nfsChannel->invalidate(
                                               canonicalMountPoint + childPath,
                                               stat.st_mode);
                                           return folly::Unit();
                                         }));
                           }
                           return collectAll(std::move(childInvalidations))
                               .unit();
                         }
                         return folly::unit;
                       })
                   .thenTry([nfsChannel](folly::Try<folly::Unit> res) {
                     return nfsChannel->completeInvalidations().thenTry(
                         [res = std::move(res)](auto&&) mutable {
                           return res;
                         });
                   }))
        .semi();
  }
#else
  auto toInvalidate = relpathFromUserPath(*path);

  XLOGF(
      WARN,
      "Manually invalidating \"{}\". This is unsupported and may lead to strange behavior.",
      toInvalidate);
  if (auto* prjfsChannel = mountHandle.getEdenMount().getPrjfsChannel()) {
    return makeImmediateFutureWith(
               [&] { return prjfsChannel->removeCachedFile(toInvalidate); })
        .semi();
  }
#endif // !_WIN32

  return EDEN_BUG_FUTURE(folly::Unit) << "Unsupported Channel type.";
}

void EdenServiceHandler::enableTracing() {
  XLOG(INFO, "Enabling tracing");
  eden::enableTracing();
}
void EdenServiceHandler::disableTracing() {
  XLOG(INFO, "Disabling tracing");
  eden::disableTracing();
}

void EdenServiceHandler::getTracePoints(std::vector<TracePoint>& result) {
  auto compactTracePoints = getAllTracepoints();
  for (auto& point : compactTracePoints) {
    TracePoint tp;
    tp.timestamp() = point.timestamp.count();
    tp.traceId() = point.traceId;
    tp.blockId() = point.blockId;
    tp.parentBlockId() = point.parentBlockId;
    if (point.name) {
      tp.name() = std::string(point.name);
    }
    if (point.start) {
      tp.event() = TracePointEvent::START;
    } else if (point.stop) {
      tp.event() = TracePointEvent::STOP;
    }
    result.emplace_back(std::move(tp));
  }
}

void EdenServiceHandler::getRetroactiveThriftRequestEvents(
    GetRetroactiveThriftRequestEventsResult& result) {
  if (!thriftRequestActivityBuffer_.has_value()) {
    throw newEdenError(
        ENOTSUP,
        EdenErrorType::POSIX_ERROR,
        "ActivityBuffer not initialized in thrift server.");
  }

  std::vector<ThriftRequestEvent> thriftEvents;
  auto bufferEvents = thriftRequestActivityBuffer_->getAllEvents();
  thriftEvents.reserve(bufferEvents.size());
  for (auto const& event : bufferEvents) {
    ThriftRequestEvent thriftEvent;
    convertThriftRequestTraceEventToThriftRequestEvent(event, thriftEvent);
    thriftEvents.emplace_back(std::move(thriftEvent));
  }

  result.events() = std::move(thriftEvents);
}

void EdenServiceHandler::getRetroactiveHgEvents(
    GetRetroactiveHgEventsResult& result,
    std::unique_ptr<GetRetroactiveHgEventsParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());
  auto backingStore = mountHandle.getObjectStore().getBackingStore();
  std::shared_ptr<SaplingBackingStore> saplingBackingStore =
      castToSaplingBackingStore(
          backingStore, mountHandle.getEdenMount().getPath());

  std::vector<HgEvent> thriftEvents;
  auto bufferEvents = saplingBackingStore->getActivityBuffer().getAllEvents();
  thriftEvents.reserve(bufferEvents.size());
  for (auto const& event : bufferEvents) {
    HgEvent thriftEvent{};
    convertHgImportTraceEventToHgEvent(
        event, *server_->getServerState()->getProcessInfoCache(), thriftEvent);
    thriftEvents.emplace_back(std::move(thriftEvent));
  }

  result.events() = std::move(thriftEvents);
}

void EdenServiceHandler::getRetroactiveInodeEvents(
    GetRetroactiveInodeEventsResult& result,
    std::unique_ptr<GetRetroactiveInodeEventsParams> params) {
  auto mountHandle = lookupMount(params->mountPoint());

  if (!mountHandle.getEdenMount().getActivityBuffer().has_value()) {
    throw newEdenError(
        ENOTSUP,
        EdenErrorType::POSIX_ERROR,
        "ActivityBuffer not initialized in EdenFS mount.");
  }

  std::vector<InodeEvent> thriftEvents;
  auto bufferEvents =
      mountHandle.getEdenMount().getActivityBuffer()->getAllEvents();
  thriftEvents.reserve(bufferEvents.size());
  for (auto const& event : bufferEvents) {
    InodeEvent thriftEvent{};
    ConvertInodeTraceEventToThriftInodeEvent(event, thriftEvent);
    thriftEvent.path() = event.getPath();
    thriftEvents.emplace_back(std::move(thriftEvent));
  }

  result.events() = std::move(thriftEvents);
}

namespace {
std::optional<folly::exception_wrapper> getFaultError(
    apache::thrift::optional_field_ref<std::string&> errorType,
    apache::thrift::optional_field_ref<std::string&> errorMessage) {
  if (!errorType.has_value() && !errorMessage.has_value()) {
    return std::nullopt;
  }

  auto createException =
      [](StringPiece type, const std::string& msg) -> folly::exception_wrapper {
    if (type == "runtime_error") {
      return std::runtime_error(msg);
    } else if (type.startsWith("errno:")) {
      auto errnum = folly::to<int>(type.subpiece(6));
      return std::system_error(errnum, std::generic_category(), msg);
    } else if (type == "quiet") {
      return QuietFault(msg);
    }
    // If we want to support other error types in the future they should
    // be added here.
    throw newEdenError(
        EdenErrorType::GENERIC_ERROR, "unknown error type ", type);
  };

  return createException(
      errorType.value_or("runtime_error"),
      errorMessage.value_or("injected error"));
}
} // namespace

void EdenServiceHandler::injectFault(unique_ptr<FaultDefinition> fault) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);
  auto& injector = server_->getServerState()->getFaultInjector();
  if (*fault->block()) {
    injector.injectBlock(
        *fault->keyClass(), *fault->keyValueRegex(), *fault->count());
    return;
  }
  if (*fault->kill()) {
    injector.injectKill(
        *fault->keyClass(), *fault->keyValueRegex(), *fault->count());
    return;
  }

  auto error = getFaultError(fault->errorType(), fault->errorMessage());
  std::chrono::milliseconds delay(*fault->delayMilliseconds());
  if (error.has_value()) {
    if (delay.count() > 0) {
      injector.injectDelayedError(
          *fault->keyClass(),
          *fault->keyValueRegex(),
          delay,
          error.value(),
          *fault->count());
    } else {
      injector.injectError(
          *fault->keyClass(),
          *fault->keyValueRegex(),
          error.value(),
          *fault->count());
    }
  } else {
    if (delay.count() > 0) {
      injector.injectDelay(
          *fault->keyClass(), *fault->keyValueRegex(), delay, *fault->count());
    } else {
      injector.injectNoop(
          *fault->keyClass(), *fault->keyValueRegex(), *fault->count());
    }
  }
}

bool EdenServiceHandler::removeFault(unique_ptr<RemoveFaultArg> fault) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);
  auto& injector = server_->getServerState()->getFaultInjector();
  return injector.removeFault(*fault->keyClass(), *fault->keyValueRegex());
}

int64_t EdenServiceHandler::unblockFault(unique_ptr<UnblockFaultArg> info) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);
  auto& injector = server_->getServerState()->getFaultInjector();
  auto error = getFaultError(info->errorType(), info->errorMessage());

  if (!info->keyClass().has_value()) {
    if (info->keyValueRegex().has_value()) {
      throw newEdenError(
          EINVAL,
          EdenErrorType::ARGUMENT_ERROR,
          "cannot specify a key value regex without a key class");
    }
    if (error.has_value()) {
      return injector.unblockAllWithError(error.value());
    } else {
      return injector.unblockAll();
    }
  }

  const auto& keyClass = info->keyClass().value();
  std::string keyValueRegex = info->keyValueRegex().value_or(".*");
  if (error.has_value()) {
    return injector.unblockWithError(keyClass, keyValueRegex, error.value());
  } else {
    return injector.unblock(keyClass, keyValueRegex);
  }
}

void EdenServiceHandler::getBlockedFaults(
    GetBlockedFaultsResponse& out,
    std::unique_ptr<GetBlockedFaultsRequest> request) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);
  auto& injector = server_->getServerState()->getFaultInjector();
  auto result = injector.getBlockedFaults(*request->keyclass());

  out.keyValues() = std::move(result);
}

void EdenServiceHandler::reloadConfig() {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO);
  server_->reloadConfig();
}

void EdenServiceHandler::fillDaemonInfo(DaemonInfo& info) {
  fb303::cpp2::fb303_status status = [&] {
    switch (server_->getStatus()) {
      case EdenServer::RunState::STARTING:
        return facebook::fb303::cpp2::fb303_status::STARTING;
      case EdenServer::RunState::RUNNING:
        return facebook::fb303::cpp2::fb303_status::ALIVE;
      case EdenServer::RunState::SHUTTING_DOWN:
        return facebook::fb303::cpp2::fb303_status::STOPPING;
    }
    EDEN_BUG() << "unexpected EdenServer status "
               << enumValue(server_->getStatus());
  }();

  info.pid() = ProcessId::current().get();
  info.commandLine() = originalCommandLine_;
  info.status() = status;

  auto now = std::chrono::steady_clock::now();
  std::chrono::duration<float> uptime = now - server_->getStartTime();
  info.uptime() = uptime.count();
}

void EdenServiceHandler::getDaemonInfo(DaemonInfo& result) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG4);
  fillDaemonInfo(result);
}

apache::thrift::ResponseAndServerStream<DaemonInfo, std::string>
EdenServiceHandler::streamStartStatus() {
  DaemonInfo result;
  fillDaemonInfo(result);

  if (result.status() != facebook::fb303::cpp2::fb303_status::STARTING) {
    return {
        result,
        apache::thrift::ServerStream<EdenStartStatusUpdate>::createEmpty()};
  }
  try {
    auto serverStream = server_->createStartupStatusThriftStream();
    return {std::move(result), std::move(serverStream)};
  } catch (EdenError& error) {
    if (error.errorType() == EdenErrorType::POSIX_ERROR &&
        error.errorCode() == EALREADY) {
      // We raced with eden start completing. Let's re-collect the status and
      // return as if EdenFS has completed. The EdenFS status should be set
      // before the startup logger completes, so at this point the status
      // should be something other than starting. Client should not
      // necessarily rely on this though.
      fillDaemonInfo(result);
      return {
          result,
          apache::thrift::ServerStream<EdenStartStatusUpdate>::createEmpty()};
    }
    throw;
  }
}

void EdenServiceHandler::checkPrivHelper(PrivHelperInfo& result) {
  auto privhelper = server_->getServerState()->getPrivHelper();
  result.connected() = privhelper->checkConnection();
  result.pid() = privhelper->getPid();
}

int64_t EdenServiceHandler::getPid() {
  return ProcessId::current().get();
}

void EdenServiceHandler::getCheckoutProgressInfo(
    CheckoutProgressInfoResponse& ret,
    unique_ptr<CheckoutProgressInfoRequest> params) {
  auto mountPath = absolutePathFromThrift(*params->mountPoint());
  auto mountHandle = server_->getMount(mountPath);
  auto& mount = mountHandle.getEdenMount();
  auto checkoutProgress = mount.getCheckoutProgress();
  if (checkoutProgress.has_value()) {
    CheckoutProgressInfo progressInfoRet;
    auto counts = mount.getInodeMap()->getInodeCounts();
    auto totalInodes =
        counts.unloadedInodeCount + counts.fileCount + counts.treeCount;

    progressInfoRet.totalInodes() = totalInodes;

    progressInfoRet.updatedInodes() = std::move(checkoutProgress.value());
    ret.checkoutProgressInfo() = std::move(progressInfoRet);
  } else {
    ret.set_noProgress();
  }
}

void EdenServiceHandler::initiateShutdown(std::unique_ptr<std::string> reason) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO);
  XLOGF(INFO, "initiateShutdown requested, reason: {}", *reason);
  server_->stop();
}

void EdenServiceHandler::getConfig(
    EdenConfigData& result,
    unique_ptr<GetConfigParams> params) {
  auto state = server_->getServerState();
  auto config = state->getEdenConfig(*params->reload());

  result = config->toThriftConfigData();
}

OptionalProcessId EdenServiceHandler::getAndRegisterClientPid() {
#ifndef _WIN32
  // The Cpp2RequestContext for a thrift request is kept in a thread local
  // on the thread which the request originates. This means this must be run
  // on the Thread in which a thrift request originates.
  auto connectionContext = getRequestContext();
  // if connectionContext will be a null pointer in an async method, so we
  // need to check for this
  if (connectionContext) {
    if (auto peerCreds = connectionContext->getConnectionContext()
                             ->getPeerEffectiveCreds()) {
      pid_t clientPid = peerCreds->pid;
      server_->getServerState()->getProcessInfoCache()->add(clientPid);
      return ProcessId(clientPid);
    }
  }
  return std::nullopt;
#else
  // Unix domain sockets on Windows don't support peer credentials.
  return std::nullopt;
#endif
}

} // namespace facebook::eden
