/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServiceHandler.h"

#include <algorithm>
#include <optional>
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/utils/ProcessNameCache.h"

#include <fb303/ServiceData.h>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Portability.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/container/Access.h>
#include <folly/futures/Future.h>
#include <folly/logging/Logger.h>
#include <folly/logging/LoggerDB.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <folly/system/Shell.h>
#include <thrift/lib/cpp/util/EnumUtils.h>

#ifndef _WIN32
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#endif // _WIN32

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeLoader.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Traverse.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/ThriftPermissionChecker.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/gen-cpp2/eden_constants.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/service/gen-cpp2/streamingeden_constants.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/Diff.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/PathLoader.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/ProcUtil.h"
#include "eden/fs/utils/ProcessNameCache.h"
#include "eden/fs/utils/StatTimes.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Try;
using folly::Unit;
using std::string;
using std::unique_ptr;
using std::vector;

namespace {
using namespace facebook::eden;

/*
 * We need a version of folly::toDelim() that accepts zero, one, or many
 * arguments so it can be used with __VA_ARGS__ in the INSTRUMENT_THRIFT_CALL()
 * macro, so we create an overloaded method, toDelimWrapper(), to achieve that
 * effect.
 */
constexpr StringPiece toDelimWrapper() {
  return "";
}

std::string toDelimWrapper(StringPiece value) {
  return value.str();
}

template <class... Args>
std::string toDelimWrapper(StringPiece arg1, const Args&... rest) {
  std::string result;
  folly::toAppendDelimFit(", ", arg1, rest..., &result);
  return result;
}

using facebook::eden::Hash;
std::string logHash(StringPiece thriftArg) {
  if (thriftArg.size() == Hash::RAW_SIZE) {
    return Hash{folly::ByteRange{thriftArg}}.toString();
  } else if (thriftArg.size() == Hash::RAW_SIZE * 2) {
    return Hash{thriftArg}.toString();
  } else {
    return folly::hexlify(thriftArg);
  }
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
    return "[" + folly::join(", ", args) + "]";
  } else {
    return folly::to<string>(
        "[",
        folly::join(", ", args.begin(), args.begin() + limit),
        ", and ",
        args.size() - limit,
        " more]");
  }
}
} // namespace

#define TLOG(logger, level, file, line)     \
  FB_LOG_RAW(logger, level, file, line, "") \
      << "[" << folly::RequestContext::get() << "] "

namespace /* anonymous namespace for helper functions */ {

#define EDEN_MICRO u8"\u00B5s"

class ThriftFetchContext : public ObjectFetchContext {
 public:
  explicit ThriftFetchContext(
      std::optional<pid_t> pid,
      folly::StringPiece endpoint)
      : pid_(pid), endpoint_(endpoint) {}
  explicit ThriftFetchContext(
      std::optional<pid_t> pid,
      folly::StringPiece endpoint,
      bool prefetchMetadata)
      : pid_(pid), endpoint_(endpoint), prefetchMetadata_(prefetchMetadata) {}

  std::optional<pid_t> getClientPid() const override {
    return pid_;
  }

  Cause getCause() const override {
    return ObjectFetchContext::Cause::Thrift;
  }

  std::optional<folly::StringPiece> getCauseDetail() const override {
    return endpoint_;
  }

  bool prefetchMetadata() const override {
    return prefetchMetadata_;
  }

  void setPrefetchMetadata(bool prefetchMetadata) {
    prefetchMetadata_ = prefetchMetadata;
  }

 private:
  std::optional<pid_t> pid_;
  folly::StringPiece endpoint_;
  bool prefetchMetadata_ = false;
};

// Helper class to log where the request completes in Future
class ThriftLogHelper {
 public:
  FOLLY_PUSH_WARNING
  FOLLY_CLANG_DISABLE_WARNING("-Wunused-member-function")
#ifdef _MSC_VER
  // Older versions of MSVC (19.13.26129.0) don't perform copy elision
  // as required by C++17, and require a move constructor to be defined for this
  // class.
  ThriftLogHelper(ThriftLogHelper&&) = default;
#else
  ThriftLogHelper(ThriftLogHelper&&) = delete;
#endif
  // However, this class is not move-assignable.
  ThriftLogHelper& operator=(ThriftLogHelper&&) = delete;
  FOLLY_POP_WARNING

  template <typename... Args>
  ThriftLogHelper(
      const folly::Logger& logger,
      folly::LogLevel level,
      folly::StringPiece itcFunctionName,
      folly::StringPiece itcFileName,
      uint32_t itcLineNumber,
      std::optional<pid_t> pid)
      : itcFunctionName_(itcFunctionName),
        itcFileName_(itcFileName),
        itcLineNumber_(itcLineNumber),
        level_(level),
        itcLogger_(logger),
        fetchContext_{pid, itcFunctionName} {}

  ~ThriftLogHelper() {
    // Logging completion time for the request
    // The line number points to where the object was originally created
    TLOG(itcLogger_, level_, itcFileName_, itcLineNumber_) << folly::format(
        "{}() took {:,} " EDEN_MICRO,
        itcFunctionName_,
        itcTimer_.elapsed().count());
  }

  ThriftFetchContext& getFetchContext() {
    return fetchContext_;
  }

  folly::StringPiece getFunctionName() {
    return itcFunctionName_;
  }

 private:
  folly::StringPiece itcFunctionName_;
  folly::StringPiece itcFileName_;
  uint32_t itcLineNumber_;
  folly::LogLevel level_;
  folly::Logger itcLogger_;
  folly::stop_watch<std::chrono::microseconds> itcTimer_ = {};
  ThriftFetchContext fetchContext_;
};

template <typename ReturnType>
Future<ReturnType> wrapFuture(
    std::unique_ptr<ThriftLogHelper> logHelper,
    folly::Future<ReturnType>&& f) {
  return std::move(f).ensure([logHelper = std::move(logHelper)]() {});
}

template <typename ReturnType>
folly::SemiFuture<ReturnType> wrapSemiFuture(
    std::unique_ptr<ThriftLogHelper> logHelper,
    folly::SemiFuture<ReturnType>&& f) {
  return std::move(f).defer(
      [logHelper = std::move(logHelper)](folly::Try<ReturnType>&& ret) {
        return std::forward<folly::Try<ReturnType>>(ret);
      });
}

#undef EDEN_MICRO

facebook::eden::InodePtr inodeFromUserPath(
    facebook::eden::EdenMount& mount,
    StringPiece rootRelativePath) {
  if (rootRelativePath.empty() || rootRelativePath == ".") {
    return mount.getRootInode();
  } else {
    static auto context =
        ObjectFetchContext::getNullContextWithCauseDetail("inodeFromUserPath");
    return mount
        .getInode(facebook::eden::RelativePathPiece{rootRelativePath}, *context)
        .get();
  }
}
} // namespace

// INSTRUMENT_THRIFT_CALL returns a unique pointer to
// ThriftLogHelper object. The returned pointer can be used to call wrapFuture()
// to attach a log message on the completion of the Future.

// When not attached to Future it will log the completion of the operation and
// time taken to complete it.
#define INSTRUMENT_THRIFT_CALL(level, ...)                            \
  ([&](folly::StringPiece functionName,                               \
       folly::StringPiece fileName,                                   \
       uint32_t lineNumber) {                                         \
    static folly::Logger logger("eden.thrift." + functionName.str()); \
    TLOG(logger, folly::LogLevel::level, fileName, lineNumber)        \
        << functionName << "(" << toDelimWrapper(__VA_ARGS__) << ")"; \
    return std::make_unique<ThriftLogHelper>(                         \
        logger,                                                       \
        folly::LogLevel::level,                                       \
        functionName,                                                 \
        fileName,                                                     \
        lineNumber,                                                   \
        getAndRegisterClientPid());                                   \
  }(__func__, __FILE__, __LINE__))

// INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME works in the same way as
// INSTRUMENT_THRIFT_CALL but takes the function name as a parameter in case of
// using inside of a lambda (in which case __func__ is "()")

#define INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(                    \
    level, functionName, pid, ...)                                    \
  ([&](folly::StringPiece fileName, uint32_t lineNumber) {            \
    static folly::Logger logger(                                      \
        "eden.thrift." + folly::to<string>(functionName));            \
    TLOG(logger, folly::LogLevel::level, fileName, lineNumber)        \
        << functionName << "(" << toDelimWrapper(__VA_ARGS__) << ")"; \
    return std::make_unique<ThriftLogHelper>(                         \
        logger,                                                       \
        folly::LogLevel::level,                                       \
        functionName,                                                 \
        fileName,                                                     \
        lineNumber,                                                   \
        pid);                                                         \
  }(__FILE__, __LINE__))

namespace facebook {
namespace eden {

const char* const kServiceName = "EdenFS";

EdenServiceHandler::EdenServiceHandler(
    std::vector<std::string> originalCommandLine,
    EdenServer* server)
    : BaseService{kServiceName},
      originalCommandLine_{std::move(originalCommandLine)},
      server_{server} {
  struct HistConfig {
    int64_t bucketSize{250};
    int64_t min{0};
    int64_t max{25000};
  };

  static constexpr std::pair<StringPiece, HistConfig> customMethodConfigs[] = {
      {"listMounts", {20, 0, 1000}},
      {"resetParentCommits", {20, 0, 1000}},
      {"getCurrentJournalPosition", {20, 0, 1000}},
      {"flushStatsNow", {20, 0, 1000}},
      {"reloadConfig", {200, 0, 10000}},
  };

  apache::thrift::metadata::ThriftServiceMetadataResponse metadataResponse;
  getProcessor()->getServiceMetadata(metadataResponse);
  auto& edenService =
      metadataResponse.metadata_ref()->services_ref()->at("eden.EdenService");
  for (auto& function : *edenService.functions_ref()) {
    HistConfig hc;
    for (auto& [name, customHistConfig] : customMethodConfigs) {
      if (*function.name_ref() == name) {
        hc = customHistConfig;
        break;
      }
    }
    // For now, only register EdenService methods, but we could traverse up
    // parent services too.
    static constexpr StringPiece prefix = "EdenService.";
    exportThriftFuncHist(
        folly::to<std::string>(prefix, *function.name_ref()),
        facebook::fb303::PROCESS,
        folly::small_vector<int>({50, 90, 99}), // percentiles to record
        hc.bucketSize,
        hc.min,
        hc.max);
  }
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

facebook::fb303::cpp2::fb303_status EdenServiceHandler::getStatus() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG4);
  auto status = server_->getStatus();
  switch (status) {
    case EdenServer::RunState::STARTING:
      return facebook::fb303::cpp2::fb303_status::STARTING;
    case EdenServer::RunState::RUNNING:
      return facebook::fb303::cpp2::fb303_status::ALIVE;
    case EdenServer::RunState::SHUTTING_DOWN:
      return facebook::fb303::cpp2::fb303_status::STOPPING;
  }
  EDEN_BUG() << "unexpected EdenServer status " << enumValue(status);
}

void EdenServiceHandler::mount(std::unique_ptr<MountArgument> argument) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO, argument->get_mountPoint());
  try {
    auto initialConfig = CheckoutConfig::loadFromClientDirectory(
        AbsolutePathPiece{*argument->mountPoint_ref()},
        AbsolutePathPiece{*argument->edenClientPath_ref()});

    server_->mount(std::move(initialConfig), *argument->readOnly_ref()).get();
  } catch (const EdenError& ex) {
    XLOG(ERR) << "Error: " << ex.what();
    throw;
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error: " << ex.what();
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO, *mountPoint);
  try {
    server_->unmount(*mountPoint).get();
  } catch (const EdenError&) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  for (const auto& edenMount : server_->getAllMountPoints()) {
    MountInfo info;
    info.mountPoint_ref() = edenMount->getPath().value();
    info.edenClientPath_ref() =
        edenMount->getConfig()->getClientDirectory().value();
    info.state_ref() = edenMount->getState();
    info.backingRepoPath_ref() = edenMount->getConfig()->getRepoSource();
    results.push_back(info);
  }
}

void EdenServiceHandler::checkOutRevision(
    std::vector<CheckoutConflict>& results,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash,
    CheckoutMode checkoutMode) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG1,
      *mountPoint,
      logHash(*hash),
      apache::thrift::util::enumName(checkoutMode, "(unknown)"));
  auto hashObj = hashFromThrift(*hash);

  auto edenMount = server_->getMount(*mountPoint);
  auto checkoutFuture = edenMount->checkout(
      hashObj,
      helper->getFetchContext().getClientPid(),
      helper->getFunctionName(),
      checkoutMode);
  results = std::move(std::move(checkoutFuture).get().conflicts);
}

void EdenServiceHandler::resetParentCommits(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<WorkingDirectoryParents> parents) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG1, *mountPoint, logHash(*parents->parent1_ref()));
  ParentCommits edenParents;
  edenParents.parent1() = hashFromThrift(*parents->parent1_ref());
  if (parents->parent2_ref()) {
    edenParents.parent2() =
        hashFromThrift(parents->parent2_ref().value_unchecked());
  }
  auto edenMount = server_->getMount(*mountPoint);
  edenMount->resetParents(edenParents);
}

void EdenServiceHandler::getSHA1(
    vector<SHA1Result>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> paths) {
  TraceBlock block("getSHA1");
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, toLogArg(*paths));
  vector<Future<Hash>> futures;
  for (const auto& path : *paths) {
    futures.emplace_back(getSHA1ForPathDefensively(
        *mountPoint, path, helper->getFetchContext()));
  }

  auto results = folly::collectAll(std::move(futures)).get();
  for (auto& result : results) {
    out.emplace_back();
    SHA1Result& sha1Result = out.back();
    if (result.hasValue()) {
      sha1Result.set_sha1(thriftHash(result.value()));
    } else {
      sha1Result.set_error(newEdenError(result.exception()));
    }
  }
}

Future<Hash> EdenServiceHandler::getSHA1ForPathDefensively(
    StringPiece mountPoint,
    StringPiece path,
    ObjectFetchContext& fetchContext) noexcept {
  return folly::makeFutureWith(
      [&] { return getSHA1ForPath(mountPoint, path, fetchContext); });
}

Future<Hash> EdenServiceHandler::getSHA1ForPath(
    StringPiece mountPoint,
    StringPiece path,
    ObjectFetchContext& fetchContext) {
  if (path.empty()) {
    return makeFuture<Hash>(newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "path cannot be the empty string"));
  }

  auto edenMount = server_->getMount(mountPoint);
  auto relativePath = RelativePathPiece{path};
  return edenMount->getInode(relativePath, fetchContext)
      .thenValue([&fetchContext](const InodePtr& inode) {
        auto fileInode = inode.asFilePtr();
        if (!S_ISREG(fileInode->getMode())) {
          // We intentionally want to refuse to compute the SHA1 of symlinks
          return makeFuture<Hash>(
              InodeError(EINVAL, fileInode, "file is a symlink"));
        }
        return fileInode->getSha1(fetchContext);
      });
}

void EdenServiceHandler::getBindMounts(
    std::vector<std::string>&,
    std::unique_ptr<std::string>) {
  // This deprecated method is only here until buck has swung through a
  // migration
}

void EdenServiceHandler::addBindMount(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> repoPath,
    std::unique_ptr<std::string> targetPath) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);

  edenMount
      ->addBindMount(
          RelativePathPiece{*repoPath}, AbsolutePathPiece{*targetPath})
      .get();
#else
  NOT_IMPLEMENTED();
#endif
}

void EdenServiceHandler::removeBindMount(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> repoPath) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);

  edenMount->removeBindMount(RelativePathPiece{*repoPath}).get();
#else
  NOT_IMPLEMENTED();
#endif
}

void EdenServiceHandler::getCurrentJournalPosition(
    JournalPosition& out,
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto latest = edenMount->getJournal().getLatest();

  *out.mountGeneration_ref() = edenMount->getMountGeneration();
  if (latest) {
    out.sequenceNumber_ref() = latest->sequenceID;
    out.snapshotHash_ref() = thriftHash(latest->toHash);
  } else {
    out.sequenceNumber_ref() = 0;
    out.snapshotHash_ref() = thriftHash(kZeroHash);
  }
}

apache::thrift::ServerStream<JournalPosition>
EdenServiceHandler::subscribeStreamTemporary(
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);

  // We need a weak ref on the mount because the thrift stream plumbing
  // may outlive the mount point
  std::weak_ptr<EdenMount> weakMount(edenMount);

  // We'll need to pass the subscriber id to both the disconnect
  // and change callbacks.  We can't know the id until after we've
  // created them both, so we need to share an optional id between them.
  auto handle = std::make_shared<std::optional<Journal::SubscriberId>>();
  auto disconnected = std::make_shared<std::atomic<bool>>(false);

  // This is called when the subscription channel is torn down
  auto onDisconnect = [weakMount, handle, disconnected] {
    XLOG(ERR) << "streaming client disconnected";
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
  handle->emplace(edenMount->getJournal().registerSubscriber(
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
  times.timestamp_ref() =
      duration_cast<nanoseconds>(event.systemTime.time_since_epoch()).count();
  times.monotonic_time_ns_ref() =
      duration_cast<nanoseconds>(event.monotonicTime.time_since_epoch())
          .count();
  return times;
}

RequestInfo thriftRequestInfo(pid_t pid, ProcessNameCache& processNameCache) {
  RequestInfo info;
  info.pid_ref() = pid;
  info.processName_ref().from_optional(processNameCache.getProcessName(pid));
  return info;
}
} // namespace

#ifndef _WIN32

namespace {
FuseCall populateFuseCall(
    uint64_t unique,
    const FuseTraceEvent::RequestHeader& request,
    ProcessNameCache& processNameCache) {
  FuseCall fc;
  fc.opcode_ref() = request.opcode;
  fc.unique_ref() = unique;
  fc.nodeid_ref() = request.nodeid;
  fc.uid_ref() = request.uid;
  fc.gid_ref() = request.gid;
  fc.pid_ref() = request.pid;

  fc.opcodeName_ref() = fuseOpcodeName(request.opcode);
  fc.processName_ref().from_optional(
      processNameCache.getProcessName(request.pid));
  return fc;
}

/**
 * Returns true if event should not be traced.
 */
bool isEventMasked(int64_t eventCategoryMask, const FuseTraceEvent& event) {
  using AccessType = ProcessAccessLog::AccessType;
  switch (fuseOpcodeAccessType(event.getRequest().opcode)) {
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

} // namespace

apache::thrift::ServerStream<FsEvent> EdenServiceHandler::traceFsEvents(
    std::unique_ptr<std::string> mountPoint,
    int64_t eventCategoryMask) {
  auto edenMount = server_->getMount(*mountPoint);

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
    TraceSubscriptionHandle<FuseTraceEvent> subHandle;
  };

  auto context = std::make_shared<Context>();
  context->argHandle = edenMount->getFuseChannel()->traceDetailedArguments();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<FsEvent>::createPublisher([context] {
        // on disconnect, release context and the TraceSubscriptionHandle
      });

  struct PublisherOwner {
    explicit PublisherOwner(
        apache::thrift::ServerStreamPublisher<FsEvent> publisher)
        : owner(true), publisher{std::move(publisher)} {}

    PublisherOwner(PublisherOwner&& that) noexcept
        : owner{std::exchange(that.owner, false)},
          publisher{std::move(that.publisher)} {}

    PublisherOwner& operator=(PublisherOwner&&) = delete;

    // Destroying a publisher without calling complete() aborts the process, so
    // ensure complete() is called when the TraceBus deletes the subscriber (as
    // occurs during unmount).
    ~PublisherOwner() {
      if (owner) {
        std::move(publisher).complete();
      }
    }

    bool owner;
    apache::thrift::ServerStreamPublisher<FsEvent> publisher;
  };

  context->subHandle =
      edenMount->getFuseChannel()->getTraceBus().subscribeFunction(
          folly::to<std::string>("strace-", edenMount->getPath().basename()),
          [owner = PublisherOwner{std::move(publisher)},
           serverState = server_->getServerState(),
           eventCategoryMask](const FuseTraceEvent& event) {
            if (isEventMasked(eventCategoryMask, event)) {
              return;
            }

            FsEvent te;
            auto times = thriftTraceEventTimes(event);
            te.times_ref() = times;

            // Legacy timestamp fields.
            te.timestamp_ref() = *times.timestamp_ref();
            te.monotonic_time_ns_ref() = *times.monotonic_time_ns_ref();

            te.fuseRequest_ref() = populateFuseCall(
                event.getUnique(),
                event.getRequest(),
                *serverState->getProcessNameCache());

            switch (event.getType()) {
              case FuseTraceEvent::START:
                te.type_ref() = FsEventType::START;
                if (auto& arguments = event.getArguments()) {
                  te.arguments_ref() = *arguments;
                }
                break;
              case FuseTraceEvent::FINISH:
                te.type_ref() = FsEventType::FINISH;
                te.result_ref().from_optional(event.getResponseCode());
                break;
            }

            te.requestInfo_ref() = thriftRequestInfo(
                event.getRequest().pid, *serverState->getProcessNameCache());

            owner.publisher.next(te);
          });

  return std::move(serverStream);
}

#endif // _WIN32

apache::thrift::ServerStream<HgEvent> EdenServiceHandler::traceHgEvents(
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);
  auto hgBackingStore = std::dynamic_pointer_cast<HgQueuedBackingStore>(
      edenMount->getObjectStore()->getBackingStore());
  if (!hgBackingStore) {
    throw std::runtime_error("mount must use hg backing store");
  }

  struct Context {
    TraceSubscriptionHandle<HgImportTraceEvent> subHandle;
  };

  auto context = std::make_shared<Context>();

  auto [serverStream, publisher] =
      apache::thrift::ServerStream<HgEvent>::createPublisher([context] {
        // on disconnect, release context and the TraceSubscriptionHandle
      });

  struct PublisherOwner {
    explicit PublisherOwner(
        apache::thrift::ServerStreamPublisher<HgEvent> publisher)
        : owner(true), publisher{std::move(publisher)} {}

    PublisherOwner(PublisherOwner&& that) noexcept
        : owner{std::exchange(that.owner, false)},
          publisher{std::move(that.publisher)} {}

    PublisherOwner& operator=(PublisherOwner&&) = delete;

    // Destroying a publisher without calling complete() aborts the process, so
    // ensure complete() is called when the TraceBus deletes the subscriber (as
    // occurs during unmount).
    ~PublisherOwner() {
      if (owner) {
        std::move(publisher).complete();
      }
    }

    bool owner;
    apache::thrift::ServerStreamPublisher<HgEvent> publisher;
  };

  context->subHandle = hgBackingStore->getTraceBus().subscribeFunction(
      folly::to<std::string>("hgtrace-", edenMount->getPath().basename()),
      [owner = PublisherOwner{std::move(publisher)},
       serverState =
           server_->getServerState()](const HgImportTraceEvent& event) {
        HgEvent te;
        te.times_ref() = thriftTraceEventTimes(event);
        switch (event.eventType) {
          case HgImportTraceEvent::QUEUE:
            te.eventType_ref() = HgEventType::QUEUE;
            break;
          case HgImportTraceEvent::START:
            te.eventType_ref() = HgEventType::START;
            break;
          case HgImportTraceEvent::FINISH:
            te.eventType_ref() = HgEventType::FINISH;
            break;
        }

        switch (event.resourceType) {
          case HgImportTraceEvent::BLOB:
            te.resourceType_ref() = HgResourceType::BLOB;
            break;
          case HgImportTraceEvent::TREE:
            te.resourceType_ref() = HgResourceType::TREE;
            break;
        }

        te.unique_ref() = event.unique;

        te.manifestNodeId_ref() = event.manifestNodeId.toString();
        te.path_ref() = event.getPath();

        // TODO: trace requesting pid
        // te.requestInfo_ref() = thriftRequestInfo(pid);

        owner.publisher.next(te);
      });

  return std::move(serverStream);
}

void EdenServiceHandler::getFilesChangedSince(
    FileDelta& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<JournalPosition> fromPosition) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);

  if (*fromPosition->mountGeneration_ref() !=
      static_cast<ssize_t>(edenMount->getMountGeneration())) {
    throw newEdenError(
        ERANGE,
        EdenErrorType::MOUNT_GENERATION_CHANGED,
        "fromPosition.mountGeneration does not match the current "
        "mountGeneration.  "
        "You need to compute a new basis for delta queries.");
  }

  // The +1 is because the core merge stops at the item prior to
  // its limitSequence parameter and we want the changes *since*
  // the provided sequence number.
  auto summed = edenMount->getJournal().accumulateRange(
      *fromPosition->sequenceNumber_ref() + 1);

  // We set the default toPosition to be where we where if summed is null
  out.toPosition_ref()->sequenceNumber_ref() =
      *fromPosition->sequenceNumber_ref();
  out.toPosition_ref()->snapshotHash_ref() = *fromPosition->snapshotHash_ref();
  out.toPosition_ref()->mountGeneration_ref() = edenMount->getMountGeneration();

  out.fromPosition_ref() = *out.toPosition_ref();

  if (summed) {
    if (summed->isTruncated) {
      throw newEdenError(
          EDOM,
          EdenErrorType::JOURNAL_TRUNCATED,
          "Journal entry range has been truncated.");
    }

    out.toPosition_ref()->sequenceNumber_ref() = summed->toSequence;
    out.toPosition_ref()->snapshotHash_ref() =
        thriftHash(summed->snapshotTransitions.back());
    out.toPosition_ref()->mountGeneration_ref() =
        edenMount->getMountGeneration();

    out.fromPosition_ref()->sequenceNumber_ref() = summed->fromSequence;
    out.fromPosition_ref()->snapshotHash_ref() =
        thriftHash(summed->snapshotTransitions.front());
    out.fromPosition_ref()->mountGeneration_ref() =
        *out.toPosition_ref()->mountGeneration_ref();

    for (const auto& entry : summed->changedFilesInOverlay) {
      auto& path = entry.first;
      auto& changeInfo = entry.second;
      if (changeInfo.isNew()) {
        out.createdPaths_ref()->emplace_back(path.stringPiece().str());
      } else {
        out.changedPaths_ref()->emplace_back(path.stringPiece().str());
      }
    }

    for (auto& path : summed->uncleanPaths) {
      out.uncleanPaths_ref()->emplace_back(path.stringPiece().str());
    }

    out.snapshotTransitions_ref()->reserve(summed->snapshotTransitions.size());
    for (auto& hash : summed->snapshotTransitions) {
      out.snapshotTransitions_ref()->push_back(thriftHash(hash));
    }
  }
}

void EdenServiceHandler::setJournalMemoryLimit(
    std::unique_ptr<PathString> mountPoint,
    int64_t limit) {
  if (!mountPoint) {
    throw newEdenError(
        EINVAL, EdenErrorType::ARGUMENT_ERROR, "mount point must not be null");
  }
  auto edenMount = server_->getMount(*mountPoint);
  if (limit < 0) {
    throw newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "memory limit must be non-negative");
  }
  edenMount->getJournal().setMemoryLimit(static_cast<size_t>(limit));
}

int64_t EdenServiceHandler::getJournalMemoryLimit(
    std::unique_ptr<PathString> mountPoint) {
  if (!mountPoint) {
    throw newEdenError(
        EINVAL, EdenErrorType::ARGUMENT_ERROR, "mount point must not be null");
  }
  auto edenMount = server_->getMount(*mountPoint);
  return static_cast<int64_t>(edenMount->getJournal().getMemoryLimit());
}

void EdenServiceHandler::flushJournal(std::unique_ptr<PathString> mountPoint) {
  if (!mountPoint) {
    throw newEdenError(
        EINVAL, EdenErrorType::ARGUMENT_ERROR, "mount point must not be null");
  }
  auto edenMount = server_->getMount(*mountPoint);
  edenMount->getJournal().flush();
}

void EdenServiceHandler::debugGetRawJournal(
    DebugGetRawJournalResponse& out,
    std::unique_ptr<DebugGetRawJournalParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *params->mountPoint_ref());
  auto edenMount = server_->getMount(*params->mountPoint_ref());
  auto mountGeneration = static_cast<ssize_t>(edenMount->getMountGeneration());

  std::optional<size_t> limitopt = std::nullopt;
  if (auto limit = params->limit_ref()) {
    limitopt = static_cast<size_t>(*limit);
  }

  out.allDeltas_ref() = edenMount->getJournal().getDebugRawJournalInfo(
      *params->fromSequenceNumber_ref(), limitopt, mountGeneration);
}

folly::SemiFuture<std::unique_ptr<std::vector<EntryInformationOrError>>>
EdenServiceHandler::semifuture_getEntryInformation(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, toLogArg(*paths));
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  // TODO: applyToInodes currently forces allocation of inodes for all specified
  // paths. It's possible to resolve this request directly from source control
  // data. In the future, this should be changed to avoid allocating inodes when
  // possible.

  return collectAll(applyToInodes(
                        rootInode,
                        *paths,
                        [](InodePtr inode) { return inode->getType(); }))
      .deferValue([](vector<Try<dtype_t>> done) {
        auto out = std::make_unique<vector<EntryInformationOrError>>();
        out->reserve(done.size());
        for (auto& item : done) {
          EntryInformationOrError result;
          if (item.hasException()) {
            result.set_error(newEdenError(item.exception()));
          } else {
            EntryInformation info;
            info.dtype_ref() = static_cast<Dtype>(item.value());
            result.set_info(info);
          }
          out->emplace_back(std::move(result));
        }
        return out;
      });
}

folly::SemiFuture<std::unique_ptr<std::vector<FileInformationOrError>>>
EdenServiceHandler::semifuture_getFileInformation(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, toLogArg(*paths));
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();
  auto& fetchContext = helper->getFetchContext();
  // TODO: applyToInodes currently forces allocation of inodes for all specified
  // paths. It's possible to resolve this request directly from source control
  // data. In the future, this should be changed to avoid allocating inodes when
  // possible.
  return wrapSemiFuture(
      std::move(helper),
      collectAll(
          applyToInodes(
              rootInode,
              *paths,
              [&fetchContext](InodePtr inode) {
                return inode->stat(fetchContext).thenValue([](struct stat st) {
                  FileInformation info;
                  info.size_ref() = st.st_size;
                  auto ts = stMtime(st);
                  info.mtime_ref()->seconds_ref() = ts.tv_sec;
                  info.mtime_ref()->nanoSeconds_ref() = ts.tv_nsec;
                  info.mode_ref() = st.st_mode;

                  FileInformationOrError result;
                  result.set_info(info);

                  return result;
                });
              }))
          .deferValue([](vector<Try<FileInformationOrError>>&& done) {
            auto out = std::make_unique<vector<FileInformationOrError>>();
            out->reserve(done.size());
            for (auto& item : done) {
              if (item.hasException()) {
                FileInformationOrError result;
                result.set_error(newEdenError(item.exception()));
                out->emplace_back(std::move(result));
              } else {
                out->emplace_back(item.value());
              }
            }
            return out;
          }));
}

folly::Future<std::unique_ptr<Glob>> EdenServiceHandler::future_globFiles(
    std::unique_ptr<GlobParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint_ref(),
      toLogArg(*params->globs_ref()),
      *params->includeDotfiles_ref());
  auto edenMount = server_->getMount(*params->mountPoint_ref());

  // Compile the list of globs into a tree
  auto globRoot = std::make_shared<GlobNode>(*params->includeDotfiles_ref());
  try {
    for (auto& globString : *params->globs_ref()) {
      try {
        globRoot->parse(globString);
      } catch (const std::domain_error& exc) {
        throw newEdenError(
            EdenErrorType::ARGUMENT_ERROR,
            "Invalid glob (",
            exc.what(),
            "): ",
            globString);
      }
    }
  } catch (const std::system_error& exc) {
    throw newEdenError(exc);
  }

  auto fileBlobsToPrefetch = *params->prefetchFiles_ref()
      ? std::make_shared<folly::Synchronized<std::vector<Hash>>>()
      : nullptr;

  auto& fetchContext = helper->getFetchContext();
  fetchContext.setPrefetchMetadata(*params->prefetchMetadata_ref());

  // These hashes must outlive the GlobResult created by evaluate as the
  // GlobResults will hold on to references to these hashes
  auto originHashes = std::make_unique<std::vector<Hash>>();

  // Globs will be evaluated against the specified commits or the current commit
  // if none are specified. The results will be collected here.
  std::vector<folly::Future<std::vector<GlobNode::GlobResult>>> globResults{};

  RelativePath searchRoot{*params->searchRoot_ref()};

  auto rootHashes = params->revisions_ref();
  if (!rootHashes->empty()) {
    // Note that we MUST reserve here, otherwise while emplacing we might
    // invalidate the earlier commitHash refrences
    globResults.reserve(rootHashes->size());
    originHashes->reserve(rootHashes->size());
    for (auto& rootHash : *rootHashes) {
      const Hash& originHash =
          originHashes->emplace_back(hashFromThrift(rootHash));

      globResults.emplace_back(
          edenMount->getObjectStore()
              ->getTreeForCommit(originHash, fetchContext)
              .thenValue([edenMount,
                          globRoot,
                          &fetchContext,
                          fileBlobsToPrefetch,
                          searchRoot](std::shared_ptr<const Tree>&& rootTree) {
                return resolveTree(
                    *edenMount->getObjectStore(),
                    fetchContext,
                    std::move(rootTree),
                    searchRoot);
              })
              .thenValue([edenMount,
                          globRoot,
                          &fetchContext,
                          fileBlobsToPrefetch,
                          &originHash](std::shared_ptr<const Tree>&& tree) {
                return globRoot->evaluate(
                    edenMount->getObjectStore(),
                    fetchContext,
                    RelativePathPiece(),
                    tree,
                    fileBlobsToPrefetch,
                    originHash);
              }));
    }
  } else {
    const Hash& originHash =
        originHashes->emplace_back(edenMount->getParentCommits().parent1());
    globResults.emplace_back(
        edenMount->getInode(searchRoot, helper->getFetchContext())
            .thenValue([helper = helper.get(),
                        globRoot,
                        edenMount,
                        fileBlobsToPrefetch,
                        &originHash](InodePtr inode) {
              return globRoot->evaluate(
                  edenMount->getObjectStore(),
                  helper->getFetchContext(),
                  RelativePathPiece(),
                  inode.asTreePtr(),
                  fileBlobsToPrefetch,
                  originHash);
            }));
  }

  return wrapFuture(
      std::move(helper),
      folly::collectAll(std::move(globResults))
          .via(server_->getServerState()->getThreadPool().get())
          .thenValue([fileBlobsToPrefetch,
                      suppressFileList = *params->suppressFileList_ref()](
                         std::vector<folly::Try<
                             std::vector<GlobNode::GlobResult>>>&& rawResults) {
            // deduplicate and combine all the globResults.
            std::vector<GlobNode::GlobResult> combinedResults{};
            if (!suppressFileList) {
              size_t totalResults{};
              for (auto& maybeResults : rawResults) {
                if (maybeResults.hasException()) {
                  return folly::makeFuture<std::vector<GlobNode::GlobResult>>(
                      maybeResults.exception());
                }
                auto& results = maybeResults.value();
                std::sort(results.begin(), results.end());
                auto resultsNewEnd =
                    std::unique(results.begin(), results.end());
                results.erase(resultsNewEnd, results.end());
                totalResults += results.size();
              }
              combinedResults.reserve(totalResults);
              // note no need to check for errors here as we would have
              // returned if any of these were errors. Note that we also
              // do not need to de-duplicate between the vectors of
              // GlobResults because no two should share an originHash.
              for (auto& results : rawResults) {
                combinedResults.insert(
                    combinedResults.end(),
                    std::make_move_iterator(results.value().begin()),
                    std::make_move_iterator(results.value().end()));
              }
            }

            // fileBlobsToPrefetch is deduplicated as an optimization.
            // The BackingStore layer does not deduplicate fetches, so lets
            // avoid causing too many duplicates here.
            if (fileBlobsToPrefetch) {
              auto fileBlobsToPrefetchLocked = fileBlobsToPrefetch->wlock();
              std::sort(
                  fileBlobsToPrefetchLocked->begin(),
                  fileBlobsToPrefetchLocked->end());
              auto fileBlobsToPrefetchNewEnd = std::unique(
                  fileBlobsToPrefetchLocked->begin(),
                  fileBlobsToPrefetchLocked->end());
              fileBlobsToPrefetchLocked->erase(
                  fileBlobsToPrefetchNewEnd, fileBlobsToPrefetchLocked->end());
            }

            return folly::makeFuture<std::vector<GlobNode::GlobResult>>(
                std::move(combinedResults));
          })
          .thenValue([edenMount,
                      wantDtype = *params->wantDtype_ref(),
                      fileBlobsToPrefetch,
                      suppressFileList = *params->suppressFileList_ref(),
                      &fetchContext,
                      config = server_->getServerState()->getEdenConfig()](
                         std::vector<GlobNode::GlobResult>&& results) mutable {
            auto out = std::make_unique<Glob>();

            if (!suppressFileList) {
              // already deduplicated at this point, no need to de-dup
              for (auto& entry : results) {
                out->matchingFiles_ref()->emplace_back(
                    entry.name.stringPiece().toString());

                if (wantDtype) {
                  out->dtypes_ref()->emplace_back(
                      static_cast<OsDtype>(entry.dtype));
                }

                out->originHashes_ref()->emplace_back(
                    entry.originHash->getBytes());
              }
            }
            if (fileBlobsToPrefetch) {
              std::vector<folly::Future<folly::Unit>> futures;

              auto store = edenMount->getObjectStore();
              auto blobs = fileBlobsToPrefetch->rlock();
              std::vector<Hash> batch;
              bool useEdenNativeFetch =
                  config->useEdenNativePrefetch.getValue();

              for (auto& hash : *blobs) {
                if (!useEdenNativeFetch && batch.size() >= 20480) {
                  futures.emplace_back(
                      store->prefetchBlobs(batch, fetchContext));
                  batch.clear();
                }
                batch.emplace_back(hash);
              }
              if (!batch.empty()) {
                futures.emplace_back(store->prefetchBlobs(batch, fetchContext));
              }

              return folly::collectUnsafe(futures).thenValue(
                  [glob = std::move(out)](auto&&) mutable {
                    return makeFuture(std::move(glob));
                  });
            }
            return makeFuture(std::move(out));
          })
          .ensure([globRoot, originHashes = std::move(originHashes)]() {
            // keep globRoot and originHashes alive until the end
          }));
}

folly::Future<Unit> EdenServiceHandler::future_chown(
    std::unique_ptr<std::string> mountPoint,
    int32_t uid,
    int32_t gid) {
#ifndef _WIN32
  return server_->getMount(*mountPoint)->chown(uid, gid);
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServiceHandler::getManifestEntry(
    ManifestEntry& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePath) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, *relativePath);
  auto mount = server_->getMount(*mountPoint);
  auto filename = RelativePathPiece{*relativePath};
  auto mode = isInManifestAsFile(mount.get(), filename);
  if (mode.has_value()) {
    *out.mode_ref() = mode.value();
  } else {
    NoValueForKeyError error;
    error.key_ref() = *relativePath;
    throw error;
  }
}

// TODO(mbolin): Make this a method of ObjectStore and make it Future-based.
std::optional<mode_t> EdenServiceHandler::isInManifestAsFile(
    const EdenMount* mount,
    const RelativePathPiece filename) {
  static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
      "EdenServiceHandler::isInManifestAsFile");
  auto tree = mount->getRootTree().get();
  auto parentDirectory = filename.dirname();
  auto objectStore = mount->getObjectStore();
  for (auto piece : parentDirectory.components()) {
    auto entry = tree->getEntryPtr(piece);
    if (entry != nullptr && entry->isTree()) {
      tree = objectStore->getTree(entry->getHash(), *context).get();
    } else {
      return std::nullopt;
    }
  }

  if (tree != nullptr) {
    auto entry = tree->getEntryPtr(filename.basename());
    if (entry != nullptr && !entry->isTree()) {
      return modeFromTreeEntryType(entry->getType());
    }
  }

  return std::nullopt;
}

void EdenServiceHandler::async_tm_getScmStatusV2(
    unique_ptr<apache::thrift::HandlerCallback<unique_ptr<GetScmStatusResult>>>
        callback,
    unique_ptr<GetScmStatusParams> params) {
  auto* request = callback->getRequest();
  folly::makeFutureWith([&, func = __func__, pid = getAndRegisterClientPid()] {
    auto helper = INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(
        DBG2,
        func,
        pid,
        *params->mountPoint_ref(),
        folly::to<string>("commitHash=", logHash(*params->commit_ref())),
        folly::to<string>("listIgnored=", *params->listIgnored_ref()));

    auto mount = server_->getMount(*params->mountPoint_ref());
    auto hash = hashFromThrift(*params->commit_ref());
    const auto& enforceParents = server_->getServerState()
                                     ->getReloadableConfig()
                                     .getEdenConfig()
                                     ->enforceParents.getValue();
    return wrapFuture(
        std::move(helper),
        mount->diff(hash, *params->listIgnored_ref(), enforceParents, request)
            .thenValue([this, mount](std::unique_ptr<ScmStatus>&& status) {
              auto result = std::make_unique<GetScmStatusResult>();
              *result->status_ref() = std::move(*status);
              *result->version_ref() = server_->getVersion();
              return result;
            }));
  })
      .thenTry([cb = std::move(callback)](
                   folly::Try<std::unique_ptr<GetScmStatusResult>>&& result) {
        cb->complete(std::move(result));
      });
}

void EdenServiceHandler::async_tm_getScmStatus(
    unique_ptr<apache::thrift::HandlerCallback<unique_ptr<ScmStatus>>> callback,
    unique_ptr<string> mountPoint,
    bool listIgnored,
    unique_ptr<string> commitHash) {
  auto* request = callback->getRequest();
  folly::makeFutureWith([&, func = __func__, pid = getAndRegisterClientPid()] {
    auto helper = INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(
        DBG2,
        func,
        pid,
        *mountPoint,
        folly::to<string>("listIgnored=", listIgnored ? "true" : "false"),
        folly::to<string>("commitHash=", logHash(*commitHash)));

    // Unlike getScmStatusV2(), this older getScmStatus() call does not enforce
    // that the caller specified the current commit.  In the future we might
    // want to enforce that even for this call, if we confirm that all existing
    // callers of this method can deal with the error.
    auto mount = server_->getMount(*mountPoint);
    auto hash = hashFromThrift(*commitHash);
    return wrapFuture(
        std::move(helper),
        mount->diff(
            hash, listIgnored, /*enforceCurrentParent=*/false, request));
  })
      .thenTry([cb = std::move(callback)](
                   folly::Try<std::unique_ptr<ScmStatus>>&& result) {
        cb->complete(std::move(result));
      });
}

Future<unique_ptr<ScmStatus>>
EdenServiceHandler::future_getScmStatusBetweenRevisions(
    unique_ptr<string> mountPoint,
    unique_ptr<string> oldHash,
    unique_ptr<string> newHash) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG2,
      *mountPoint,
      folly::to<string>("oldHash=", logHash(*oldHash)),
      folly::to<string>("newHash=", logHash(*newHash)));
  auto id1 = hashFromThrift(*oldHash);
  auto id2 = hashFromThrift(*newHash);
  auto mount = server_->getMount(*mountPoint);
  return wrapFuture(
      std::move(helper),
      diffCommitsForStatus(mount->getObjectStore(), id1, id2));
}

void EdenServiceHandler::debugGetScmTree(
    vector<ScmTreeEntry>& entries,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, logHash(*idStr));
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
      "EdenServiceHandler::debugGetScmTree");
  std::shared_ptr<const Tree> tree;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    tree = localStore->getTree(id).get();
  } else {
    tree = store->getTree(id, *context).get();
  }

  if (!tree) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no tree found for id ",
        id.toString());
  }

  for (const auto& entry : tree->getTreeEntries()) {
    entries.emplace_back();
    auto& out = entries.back();
    out.name_ref() = entry.getName().stringPiece().str();
    out.mode_ref() = modeFromTreeEntryType(entry.getType());
    out.id_ref() = thriftHash(entry.getHash());
  }
}

void EdenServiceHandler::debugGetScmBlob(
    string& data,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, logHash(*idStr));
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
      "EdenServiceHandler::debugGetScmBlob");
  std::shared_ptr<const Blob> blob;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    blob = localStore->getBlob(id).get();
  } else {
    blob = store->getBlob(id, *context).get();
  }

  if (!blob) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no blob found for id ",
        id.toString());
  }
  auto dataBuf = blob->getContents().cloneCoalescedAsValue();
  data.assign(reinterpret_cast<const char*>(dataBuf.data()), dataBuf.length());
}

void EdenServiceHandler::debugGetScmBlobMetadata(
    ScmBlobMetadata& result,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, logHash(*idStr));
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
      "EdenServiceHandler::debugGetScmBlobMetadata");
  std::optional<BlobMetadata> metadata;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    metadata = localStore->getBlobMetadata(id).get();
  } else {
    auto sha1 = store->getBlobSha1(id, *context).get();
    auto size = store->getBlobSize(id, *context).get();
    metadata.emplace(sha1, size);
  }

  if (!metadata.has_value()) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no blob metadata found for id ",
        id.toString());
  }
  result.size_ref() = metadata->size;
  result.contentsSha1_ref() = thriftHash(metadata->sha1);
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
      const std::optional<Hash>& hash,
      uint64_t fsRefcount,
      const std::vector<ChildEntry>& entries) override {
#ifndef _WIN32
    auto* inodeMetadataTable = mount_->getInodeMetadataTable();
#endif

    TreeInodeDebugInfo info;
    info.inodeNumber_ref() = ino.get();
    info.path_ref() = path.stringPiece().str();
    info.materialized_ref() = !hash.has_value();
    if (hash.has_value()) {
      info.treeHash_ref() = thriftHash(hash.value());
    }
    info.refcount_ref() = fsRefcount;

    info.entries_ref()->reserve(entries.size());

    for (auto& entry : entries) {
      TreeInodeEntryDebugInfo entryInfo;
      entryInfo.name_ref() = entry.name.stringPiece().str();
      entryInfo.inodeNumber_ref() = entry.ino.get();

      // This could be enabled on Windows if InodeMetadataTable was removed.
#ifndef _WIN32
      if (auto metadata = (flags_ & eden_constants::DIS_COMPUTE_ACCURATE_MODE_)
              ? inodeMetadataTable->getOptional(entry.ino)
              : std::nullopt) {
        entryInfo.mode_ref() = metadata->mode;
      } else {
        entryInfo.mode_ref() = dtype_to_mode(entry.dtype);
      }
#else
      entryInfo.mode_ref() = dtype_to_mode(entry.dtype);
#endif

      entryInfo.loaded_ref() = entry.loadedChild != nullptr;
      entryInfo.materialized_ref() = !entry.hash.has_value();
      if (entry.hash.has_value()) {
        entryInfo.hash_ref() = thriftHash(entry.hash.value());
      }

      if ((flags_ & eden_constants::DIS_COMPUTE_BLOB_SIZES_) &&
          dtype_t::Dir != entry.dtype) {
        if (entry.hash.has_value()) {
          // schedule fetching size from ObjectStore::getBlobSize
          requestedSizes_.push_back(RequestedSize{
              results_.size(), info.entries_ref()->size(), entry.hash.value()});
        } else {
#ifndef _WIN32
          entryInfo.fileSize_ref() =
              mount_->getOverlayFileAccess()->getFileSize(
                  entry.ino, entry.loadedChild.get());
#else
          // This following code ends up doing a stat in the working directory.
          // This is safe to do as Windows works very differently from
          // Linux/macOS when dealing with materialized files. In this code, we
          // know that the file is materialized because we do not have a hash
          // for it, and every materialized file is present on disk and
          // reading/stating it is guaranteed to be done without EdenFS
          // involvement. If somehow EdenFS is wrong, and this ends up
          // triggering a recursive call into EdenFS, we are detecting this and
          // simply bailing out very early in the callback.
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

      info.entries_ref()->push_back(entryInfo);
    }

    results_.push_back(std::move(info));
  }

  bool shouldRecurse(const ChildEntry& entry) override {
    if ((flags_ & eden_constants::DIS_REQUIRE_LOADED_) && !entry.loadedChild) {
      return false;
    }
    if ((flags_ & eden_constants::DIS_REQUIRE_MATERIALIZED_) &&
        entry.hash.has_value()) {
      return false;
    }
    return true;
  }

  void fillBlobSizes(ObjectFetchContext& fetchContext) {
    std::vector<folly::Future<folly::Unit>> futures;
    futures.reserve(requestedSizes_.size());
    for (auto& request : requestedSizes_) {
      futures.push_back(mount_->getObjectStore()
                            ->getBlobSize(request.hash, fetchContext)
                            .thenValue([this, request](uint64_t blobSize) {
                              results_.at(request.resultIndex)
                                  .entries_ref()
                                  ->at(request.entryIndex)
                                  .fileSize_ref() = blobSize;
                            }));
    }
    folly::collectAll(futures).get();
  }

 private:
  struct RequestedSize {
    size_t resultIndex;
    size_t entryIndex;
    Hash hash;
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
    int64_t flags) {
  if (0 == flags) {
    flags = eden_constants::DIS_REQUIRE_LOADED_ |
        eden_constants::DIS_COMPUTE_BLOB_SIZES_;
  }

  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint, *path, flags);
  auto edenMount = server_->getMount(*mountPoint);

  auto inode = inodeFromUserPath(*edenMount, *path).asTreePtr();
  auto inodePath = inode->getPath().value();

  InodeStatusCallbacks callbacks{edenMount.get(), flags, inodeInfo};
  traverseObservedInodes(*inode, inodePath, callbacks);
  callbacks.fillBlobSizes(helper->getFetchContext());
}

void EdenServiceHandler::debugOutstandingFuseCalls(
    std::vector<FuseCall>& outstandingCalls,
    std::unique_ptr<std::string> mountPoint) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto edenMount = server_->getMount(*mountPoint);
  auto* fuseChannel = edenMount->getFuseChannel();

  for (const auto& call : fuseChannel->getOutstandingRequests()) {
    outstandingCalls.push_back(populateFuseCall(
        call.unique,
        call.request,
        *server_->getServerState()->getProcessNameCache()));
  }
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServiceHandler::debugGetInodePath(
    InodePathDebugInfo& info,
    std::unique_ptr<std::string> mountPoint,
    int64_t inodeNumber) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto inodeNum = static_cast<InodeNumber>(inodeNumber);
  auto inodeMap = server_->getMount(*mountPoint)->getInodeMap();

  auto relativePath = inodeMap->getPathForInode(inodeNum);
  // Check if the inode is loaded
  info.loaded_ref() = inodeMap->lookupLoadedInode(inodeNum) != nullptr;
  // If getPathForInode returned none then the inode is unlinked
  info.linked_ref() = relativePath != std::nullopt;
  info.path_ref() = relativePath ? relativePath->stringPiece().str() : "";
}

void EdenServiceHandler::clearFetchCounts() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  for (auto& mount : server_->getMountPoints()) {
    mount->getObjectStore()->clearFetchCounts();
  }
}

void EdenServiceHandler::clearFetchCountsByMount(
    std::unique_ptr<std::string> mountPath) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto mount = server_->getMount(*mountPath);
  mount->getObjectStore()->clearFetchCounts();
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
  for (const auto& backingStore : server_->getHgQueuedBackingStores()) {
    auto filePaths = backingStore->stopRecordingFetch();
    (*results.fetchedFilePaths_ref())["HgQueuedBackingStore"].insert(
        filePaths.begin(), filePaths.end());
  }
} // namespace eden

void EdenServiceHandler::getAccessCounts(
    GetAccessCountsResult& result,
    int64_t duration) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  result.cmdsByPid_ref() =
      server_->getServerState()->getProcessNameCache()->getAllProcessNames();

  auto seconds = std::chrono::seconds{duration};

  for (auto& mount : server_->getMountPoints()) {
    auto& mountStr = mount->getPath().value();
    auto& pal = mount->getProcessAccessLog();

    auto& pidFetches = mount->getObjectStore()->getPidFetches();

    MountAccesses& ma = result.accessesByMount_ref()[mountStr];
    for (auto& [pid, accessCounts] : pal.getAccessCounts(seconds)) {
      ma.accessCountsByPid_ref()[pid] = accessCounts;
    }

    for (auto& [pid, fetchCount] : *pidFetches.rlock()) {
      ma.fetchCountsByPid_ref()[pid] = fetchCount;
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

int64_t EdenServiceHandler::unloadInodeForPath(
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path,
    std::unique_ptr<TimeSpec> age) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG1, *mountPoint, *path);
  auto edenMount = server_->getMount(*mountPoint);

  TreeInodePtr inode = inodeFromUserPath(*edenMount, *path).asTreePtr();
  auto cutoff = std::chrono::system_clock::now() -
      std::chrono::seconds(*age->seconds_ref()) -
      std::chrono::nanoseconds(*age->nanoSeconds_ref());
  auto cutoff_ts = folly::to<timespec>(cutoff);
  return inode->unloadChildrenLastAccessedBefore(cutoff_ts);
#else
  NOT_IMPLEMENTED();
#endif
}

void EdenServiceHandler::getStatInfo(InternalStats& result) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto mountList = server_->getMountPoints();
  for (auto& mount : mountList) {
    auto inodeMap = mount->getInodeMap();
    // Set LoadedInde Count and unloaded Inode count for the mountPoint.
    MountInodeInfo mountInodeInfo;
    auto counts = inodeMap->getInodeCounts();
    mountInodeInfo.unloadedInodeCount_ref() = counts.unloadedInodeCount;
    mountInodeInfo.loadedFileCount_ref() = counts.fileCount;
    mountInodeInfo.loadedTreeCount_ref() = counts.treeCount;

    JournalInfo journalThrift;
    if (auto journalStats = mount->getJournal().getStats()) {
      journalThrift.entryCount_ref() = journalStats->entryCount;
      journalThrift.durationSeconds_ref() =
          journalStats->getDurationInSeconds();
    } else {
      journalThrift.entryCount_ref() = 0;
      journalThrift.durationSeconds_ref() = 0;
    }
    journalThrift.memoryUsage_ref() = mount->getJournal().estimateMemoryUsage();
    result.mountPointJournalInfo_ref()[mount->getPath().stringPiece().str()] =
        journalThrift;

    result.mountPointInfo_ref()[mount->getPath().stringPiece().str()] =
        mountInodeInfo;
  }
  // Get the counters and set number of inodes unloaded by periodic unload job.
  result.counters_ref() = fb303::ServiceData::get()->getCounters();
  result.periodicUnloadCount_ref() =
      result.counters_ref()[kPeriodicUnloadCounterKey.toString()];

  auto privateDirtyBytes = facebook::eden::proc_util::calculatePrivateBytes();
  if (privateDirtyBytes) {
    result.privateBytes_ref() = privateDirtyBytes.value();
  }

  auto memoryStats = facebook::eden::proc_util::readMemoryStats();
  if (memoryStats) {
    result.vmRSSBytes_ref() = memoryStats->resident;
  }

  // Note: this will be removed in a subsequent commit.
  // We now report periodically via ServiceData
  std::string smaps;
  if (folly::readFile("/proc/self/smaps", smaps)) {
    result.smaps_ref() = std::move(smaps);
  }

  const auto blobCacheStats = server_->getBlobCache()->getStats();
  result.blobCacheStats_ref()->entryCount_ref() = blobCacheStats.blobCount;
  result.blobCacheStats_ref()->totalSizeInBytes_ref() =
      blobCacheStats.totalSizeInBytes;
  result.blobCacheStats_ref()->hitCount_ref() = blobCacheStats.hitCount;
  result.blobCacheStats_ref()->missCount_ref() = blobCacheStats.missCount;
  result.blobCacheStats_ref()->evictionCount_ref() =
      blobCacheStats.evictionCount;
  result.blobCacheStats_ref()->dropCount_ref() = blobCacheStats.dropCount;
}

void EdenServiceHandler::flushStatsNow() {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  server_->flushStatsNow();
}

Future<Unit> EdenServiceHandler::future_invalidateKernelInodeCache(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> path) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint, *path);
  auto edenMount = server_->getMount(*mountPoint);
  InodePtr inode = inodeFromUserPath(*edenMount, *path);
  auto* fuseChannel = edenMount->getFuseChannel();

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
  return fuseChannel->flushInvalidations();
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServiceHandler::enableTracing() {
  XLOG(INFO) << "Enabling tracing";
  eden::enableTracing();
}
void EdenServiceHandler::disableTracing() {
  XLOG(INFO) << "Disabling tracing";
  eden::disableTracing();
}

void EdenServiceHandler::getTracePoints(std::vector<TracePoint>& result) {
  auto compactTracePoints = getAllTracepoints();
  for (auto& point : compactTracePoints) {
    TracePoint tp;
    tp.timestamp_ref() = point.timestamp.count();
    tp.traceId_ref() = point.traceId;
    tp.blockId_ref() = point.blockId;
    tp.parentBlockId_ref() = point.parentBlockId;
    if (point.name) {
      tp.name_ref() = std::string(point.name);
    }
    if (point.start) {
      tp.event_ref() = TracePointEvent::START;
    } else if (point.stop) {
      tp.event_ref() = TracePointEvent::STOP;
    }
    result.emplace_back(std::move(tp));
  }
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
  auto& injector = server_->getServerState()->getFaultInjector();
  if (*fault->block_ref()) {
    injector.injectBlock(
        *fault->keyClass_ref(),
        *fault->keyValueRegex_ref(),
        *fault->count_ref());
    return;
  }

  auto error = getFaultError(fault->errorType_ref(), fault->errorMessage_ref());
  std::chrono::milliseconds delay(*fault->delayMilliseconds_ref());
  if (error.has_value()) {
    if (delay.count() > 0) {
      injector.injectDelayedError(
          *fault->keyClass_ref(),
          *fault->keyValueRegex_ref(),
          delay,
          error.value(),
          *fault->count_ref());
    } else {
      injector.injectError(
          *fault->keyClass_ref(),
          *fault->keyValueRegex_ref(),
          error.value(),
          *fault->count_ref());
    }
  } else {
    if (delay.count() > 0) {
      injector.injectDelay(
          *fault->keyClass_ref(),
          *fault->keyValueRegex_ref(),
          delay,
          *fault->count_ref());
    } else {
      injector.injectNoop(
          *fault->keyClass_ref(),
          *fault->keyValueRegex_ref(),
          *fault->count_ref());
    }
  }
}

bool EdenServiceHandler::removeFault(unique_ptr<RemoveFaultArg> fault) {
  auto& injector = server_->getServerState()->getFaultInjector();
  return injector.removeFault(
      *fault->keyClass_ref(), *fault->keyValueRegex_ref());
}

int64_t EdenServiceHandler::unblockFault(unique_ptr<UnblockFaultArg> info) {
  auto& injector = server_->getServerState()->getFaultInjector();
  auto error = getFaultError(info->errorType_ref(), info->errorMessage_ref());

  if (!info->keyClass_ref().has_value()) {
    if (info->keyValueRegex_ref().has_value()) {
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

  const auto& keyClass = info->keyClass_ref().value();
  std::string keyValueRegex = info->keyValueRegex_ref().value_or(".*");
  if (error.has_value()) {
    return injector.unblockWithError(keyClass, keyValueRegex, error.value());
  } else {
    return injector.unblock(keyClass, keyValueRegex);
  }
}

void EdenServiceHandler::reloadConfig() {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO);
  server_->reloadConfig();
}

void EdenServiceHandler::getDaemonInfo(DaemonInfo& result) {
  *result.pid_ref() = getpid();
  *result.commandLine_ref() = originalCommandLine_;
  result.status_ref() = getStatus();

  auto now = std::chrono::steady_clock::now();
  std::chrono::duration<float> uptime = now - server_->getStartTime();
  result.uptime_ref() = uptime.count();
}

int64_t EdenServiceHandler::getPid() {
  return getpid();
}

void EdenServiceHandler::initiateShutdown(std::unique_ptr<std::string> reason) {
  auto helper = INSTRUMENT_THRIFT_CALL(INFO);
  XLOG(INFO) << "initiateShutdown requested, reason: " << *reason;
  server_->stop();
}

void EdenServiceHandler::getConfig(
    EdenConfigData& result,
    unique_ptr<GetConfigParams> params) {
  auto state = server_->getServerState();
  auto config = state->getEdenConfig(*params->reload_ref());

  result = config->toThriftConfigData();
}

std::optional<pid_t> EdenServiceHandler::getAndRegisterClientPid() {
#ifndef _WIN32
  // The Cpp2RequestContext for a thrift request is kept in a thread local
  // on the thread which the request originates. This means this must be run
  // on the Thread in which a thrift request originates.
  auto connectionContext = getRequestContext();
  // if connectionContext will be a null pointer in an async method, so we need
  // to check for this
  if (connectionContext) {
    pid_t clientPid =
        connectionContext->getConnectionContext()->getPeerEffectiveCreds()->pid;
    server_->getServerState()->getProcessNameCache()->add(clientPid);
    return clientPid;
  }
  return std::nullopt;
#else
  return std::nullopt;
#endif
}

} // namespace eden
} // namespace facebook
