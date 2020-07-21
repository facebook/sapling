/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServiceHandler.h"

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
#include <optional>

#ifdef _WIN32
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/win/utils/stub.h" // @manual
#else
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/ProcessNameCache.h"
#endif // _WIN32

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeLoader.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/ThriftPermissionChecker.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/Diff.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/ProcUtil.h"
#include "eden/fs/utils/StatTimes.h"

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Try;
using folly::Unit;
using std::string;
using std::unique_ptr;
using std::vector;

namespace {
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

// Helper class to log where the request completes in Future
class ThriftLogHelper {
 public:
  FOLLY_PUSH_WARNING
  FOLLY_CLANG_DISABLE_WARNING("-Wunused-member-function")
  // Older versions of MSVC (19.13.26129.0) don't perform copy elision
  // as required by C++17, and require a move constructor to be defined for this
  // class.
  ThriftLogHelper(ThriftLogHelper&&) = default;
  // However, this class is not move-assignable.
  ThriftLogHelper& operator=(ThriftLogHelper&&) = delete;
  FOLLY_POP_WARNING

  template <typename... Args>
  ThriftLogHelper(
      const folly::Logger& logger,
      folly::LogLevel level,
      folly::StringPiece itcFunctionName,
      folly::StringPiece itcFileName,
      uint32_t itcLineNumber)
      : itcFunctionName_(itcFunctionName),
        itcFileName_(itcFileName),
        itcLineNumber_(itcLineNumber),
        level_(level),
        itcLogger_(logger) {}

  ~ThriftLogHelper() {
    if (wrapperExecuted_) {
      // Logging of future creation at folly::LogLevel::DBG3.
      TLOG(itcLogger_, folly::LogLevel::DBG3, itcFileName_, itcLineNumber_)
          << folly::format(
                 "{}() created future {:,} " EDEN_MICRO,
                 itcFunctionName_,
                 itcTimer_.elapsed().count());
    } else {
      // If this object was not used for future creation
      // log the elaped time here.
      TLOG(itcLogger_, level_, itcFileName_, itcLineNumber_) << folly::format(
          "{}() took {:,} " EDEN_MICRO,
          itcFunctionName_,
          itcTimer_.elapsed().count());
    }
  }

  template <typename ReturnType>
  Future<ReturnType> wrapFuture(folly::Future<ReturnType>&& f) {
    wrapperExecuted_ = true;
    return std::move(f).thenTry(
        [timer = itcTimer_,
         logger = this->itcLogger_,
         funcName = itcFunctionName_,
         level = level_,
         filename = itcFileName_,
         linenumber = itcLineNumber_](folly::Try<ReturnType>&& ret) {
          // Logging completion time for the request
          // The line number points to where the object was originally created
          TLOG(logger, level, filename, linenumber) << folly::format(
              "{}() took {:,} " EDEN_MICRO, funcName, timer.elapsed().count());
          return std::forward<folly::Try<ReturnType>>(ret);
        });
  }

 private:
  folly::StringPiece itcFunctionName_;
  folly::StringPiece itcFileName_;
  uint32_t itcLineNumber_;
  folly::LogLevel level_;
  folly::Logger itcLogger_;
  folly::stop_watch<std::chrono::microseconds> itcTimer_ = {};
  bool wrapperExecuted_ = false;
};

#undef EDEN_MICRO

facebook::eden::InodePtr inodeFromUserPath(
    facebook::eden::EdenMount& mount,
    StringPiece rootRelativePath) {
  if (rootRelativePath.empty() || rootRelativePath == ".") {
    return mount.getRootInode();
  } else {
    return mount.getInode(facebook::eden::RelativePathPiece{rootRelativePath})
        .get();
  }
}
} // namespace

// INSTRUMENT_THRIFT_CALL returns a unique pointer to
// ThriftLogHelper object. The returned pointer can be used to call wrapFuture()
// to attach a log message on the completion of the Future.

// When not attached to Future it will log the completion of the operation and
// time taken to complete it.

#define INSTRUMENT_THRIFT_CALL(level, ...)                                   \
  ([&](folly::StringPiece functionName,                                      \
       folly::StringPiece fileName,                                          \
       uint32_t lineNumber) {                                                \
    static folly::Logger logger("eden.thrift." + functionName.str());        \
    TLOG(logger, folly::LogLevel::level, fileName, lineNumber)               \
        << functionName << "(" << toDelimWrapper(__VA_ARGS__) << ")";        \
    return ThriftLogHelper(                                                  \
        logger, folly::LogLevel::level, functionName, fileName, lineNumber); \
  }(__func__, __FILE__, __LINE__))

// INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME works in the same way as
// INSTRUMENT_THRIFT_CALL but takes the function name as a parameter in case of
// using inside of a lambda (in which case __func__ is "()")

#define INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(level, functionName, ...)  \
  ([&](folly::StringPiece fileName, uint32_t lineNumber) {                   \
    static folly::Logger logger(                                             \
        "eden.thrift." + folly::to<string>(functionName));                   \
    TLOG(logger, folly::LogLevel::level, fileName, lineNumber)               \
        << functionName << "(" << toDelimWrapper(__VA_ARGS__) << ")";        \
    return ThriftLogHelper(                                                  \
        logger, folly::LogLevel::level, functionName, fileName, lineNumber); \
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
    *info.mountPoint_ref() = edenMount->getPath().value();
    *info.edenClientPath_ref() =
        edenMount->getConfig()->getClientDirectory().value();
    *info.state_ref() = edenMount->getState();
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
  auto checkoutFuture = edenMount->checkout(hashObj, checkoutMode);
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
    futures.emplace_back(getSHA1ForPathDefensively(*mountPoint, path));
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
    StringPiece path) noexcept {
  return folly::makeFutureWith(
      [&] { return getSHA1ForPath(mountPoint, path); });
}

Future<Hash> EdenServiceHandler::getSHA1ForPath(
    StringPiece mountPoint,
    StringPiece path) {
  if (path.empty()) {
    return makeFuture<Hash>(newEdenError(
        EINVAL,
        EdenErrorType::ARGUMENT_ERROR,
        "path cannot be the empty string"));
  }

  auto edenMount = server_->getMount(mountPoint);
  auto relativePath = RelativePathPiece{path};
  return edenMount->getInode(relativePath).thenValue([](const InodePtr& inode) {
    auto fileInode = inode.asFilePtr();
    if (!S_ISREG(fileInode->getMode())) {
      // We intentionally want to refuse to compute the SHA1 of symlinks
      return makeFuture<Hash>(
          InodeError(EINVAL, fileInode, "file is a symlink"));
    }
    return fileInode->getSha1(ObjectFetchContext::getNullContext());
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
    *out.sequenceNumber_ref() = latest->sequenceID;
    *out.snapshotHash_ref() = thriftHash(latest->toHash);
  } else {
    *out.sequenceNumber_ref() = 0;
    *out.snapshotHash_ref() = thriftHash(kZeroHash);
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

  // This is called each time the journal is updated
  auto onJournalChange = [weakMount, stream = std::move(stream)]() mutable {
    auto mount = weakMount.lock();
    if (mount) {
      auto& journal = mount->getJournal();
      JournalPosition pos;

      auto latest = journal.getLatest();
      if (latest) {
        *pos.sequenceNumber_ref() = latest->sequenceID;
        *pos.snapshotHash_ref() = StringPiece(latest->toHash.getBytes()).str();
      } else {
        *pos.sequenceNumber_ref() = 0;
        *pos.snapshotHash_ref() = StringPiece(kZeroHash.getBytes()).str();
      }
      *pos.mountGeneration_ref() = mount->getMountGeneration();
      stream->publisher.next(pos);
    }
  };

  // Register onJournalChange with the journal subsystem, and assign
  // the subscriber id into the handle so that the callbacks can consume it.
  handle->emplace(
      edenMount->getJournal().registerSubscriber(std::move(onJournalChange)));

  return std::move(streamAndPublisher.first);
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
  *out.toPosition_ref()->sequenceNumber_ref() =
      *fromPosition->sequenceNumber_ref();
  *out.toPosition_ref()->snapshotHash_ref() = *fromPosition->snapshotHash_ref();
  *out.toPosition_ref()->mountGeneration_ref() =
      edenMount->getMountGeneration();

  *out.fromPosition_ref() = *out.toPosition_ref();

  if (summed) {
    if (summed->isTruncated) {
      throw newEdenError(
          EDOM,
          EdenErrorType::JOURNAL_TRUNCATED,
          "Journal entry range has been truncated.");
    }
    *out.toPosition_ref()->sequenceNumber_ref() = summed->toSequence;
    *out.toPosition_ref()->snapshotHash_ref() = thriftHash(summed->toHash);
    *out.toPosition_ref()->mountGeneration_ref() =
        edenMount->getMountGeneration();

    *out.fromPosition_ref()->sequenceNumber_ref() = summed->fromSequence;
    *out.fromPosition_ref()->snapshotHash_ref() = thriftHash(summed->fromHash);
    *out.fromPosition_ref()->mountGeneration_ref() =
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

  *out.allDeltas_ref() = edenMount->getJournal().getDebugRawJournalInfo(
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

  // TODO: applyToInodes currently forces allocation of inodes for all specified
  // paths. It's possible to resolve this request directly from source control
  // data. In the future, this should be changed to avoid allocating inodes when
  // possible.

  return collectAll(applyToInodes(
                        rootInode,
                        *paths,
                        [](InodePtr inode) {
                          return inode
                              ->stat(ObjectFetchContext::getNullContext())
                              .thenValue([](struct stat st) {
                                FileInformation info;
                                *info.size_ref() = st.st_size;
                                auto ts = stMtime(st);
                                *info.mtime_ref()->seconds_ref() = ts.tv_sec;
                                *info.mtime_ref()->nanoSeconds_ref() =
                                    ts.tv_nsec;
                                *info.mode_ref() = st.st_mode;

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
      });
}

void EdenServiceHandler::glob(
    vector<string>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> globs) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, toLogArg(*globs));
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  // TODO: Track and report object fetches required for this glob.
  auto& context = ObjectFetchContext::getNullContext();

  try {
    // Compile the list of globs into a tree
    GlobNode globRoot(/*includeDotfiles=*/true);
    for (auto& globString : *globs) {
      globRoot.parse(globString);
    }

    // and evaluate it against the root
    auto matches = globRoot
                       .evaluate(
                           edenMount->getObjectStore(),
                           context,
                           RelativePathPiece(),
                           rootInode,
                           /*fileBlobsToPrefetch=*/nullptr)
                       .get();
    for (auto& fileName : matches) {
      out.emplace_back(fileName.name.stringPiece().toString());
    }
  } catch (const std::system_error& exc) {
    throw newEdenError(exc);
  }
}

folly::Future<std::unique_ptr<Glob>> EdenServiceHandler::future_globFiles(
    std::unique_ptr<GlobParams> params) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      DBG3,
      *params->mountPoint_ref(),
      toLogArg(*params->globs_ref()),
      *params->includeDotfiles_ref());
  auto edenMount = server_->getMount(*params->mountPoint_ref());
  auto rootInode = edenMount->getRootInode();

  // TODO: Track and report object fetches required for this glob.
  auto& context = ObjectFetchContext::getNullContext();

  // Compile the list of globs into a tree
  auto globRoot = std::make_shared<GlobNode>(*params->includeDotfiles_ref());
  try {
    for (auto& globString : *params->globs_ref()) {
      globRoot->parse(globString);
    }
  } catch (const std::system_error& exc) {
    throw newEdenError(exc);
  }

  auto fileBlobsToPrefetch = *params->prefetchFiles_ref()
      ? std::make_shared<folly::Synchronized<std::vector<Hash>>>()
      : nullptr;

  // and evaluate it against the root
  return helper.wrapFuture(
      globRoot
          ->evaluate(
              edenMount->getObjectStore(),
              context,
              RelativePathPiece(),
              rootInode,
              fileBlobsToPrefetch)
          .thenValue([edenMount,
                      wantDtype = *params->wantDtype_ref(),
                      fileBlobsToPrefetch,
                      suppressFileList = *params->suppressFileList_ref()](
                         std::vector<GlobNode::GlobResult>&& results) {
            auto out = std::make_unique<Glob>();

            if (!suppressFileList) {
              std::unordered_set<RelativePathPiece> seenPaths;
              for (auto& entry : results) {
                auto ret = seenPaths.insert(entry.name);
                if (ret.second) {
                  out->matchingFiles_ref()->emplace_back(
                      entry.name.stringPiece().toString());

                  if (wantDtype) {
                    out->dtypes_ref()->emplace_back(
                        static_cast<OsDtype>(entry.dtype));
                  }
                }
              }
            }
            if (fileBlobsToPrefetch) {
              // TODO: It would be worth tracking and logging glob fetches,
              // since they're often used by watchman.
              auto& context = ObjectFetchContext::getNullContext();

              std::vector<folly::Future<folly::Unit>> futures;

              auto store = edenMount->getObjectStore();
              auto blobs = fileBlobsToPrefetch->rlock();
              std::vector<Hash> batch;

              for (auto& hash : *blobs) {
                if (batch.size() >= 20480) {
                  futures.emplace_back(store->prefetchBlobs(batch, context));
                  batch.clear();
                }
                batch.emplace_back(hash);
              }
              if (!batch.empty()) {
                futures.emplace_back(store->prefetchBlobs(batch, context));
              }

              return folly::collectUnsafe(futures).thenValue(
                  [glob = std::move(out)](auto&&) mutable {
                    return makeFuture(std::move(glob));
                  });
            }
            return makeFuture(std::move(out));
          })
          .ensure([globRoot]() {
            // keep globRoot alive until the end
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
  auto tree = mount->getRootTree().get();
  auto parentDirectory = filename.dirname();
  auto objectStore = mount->getObjectStore();
  for (auto piece : parentDirectory.components()) {
    auto entry = tree->getEntryPtr(piece);
    if (entry != nullptr && entry->isTree()) {
      tree =
          objectStore
              ->getTree(entry->getHash(), ObjectFetchContext::getNullContext())
              .get();
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
  folly::makeFutureWith([&, func = __func__] {
    auto helper = INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(
        DBG2,
        func,
        *params->mountPoint_ref(),
        folly::to<string>("commitHash=", logHash(*params->commit_ref())),
        folly::to<string>("listIgnored=", *params->listIgnored_ref()));

    auto mount = server_->getMount(*params->mountPoint_ref());
    auto hash = hashFromThrift(*params->commit_ref());
    const auto& enforceParents = server_->getServerState()
                                     ->getReloadableConfig()
                                     .getEdenConfig()
                                     ->enforceParents.getValue();
    return helper.wrapFuture(
        mount->diff(hash, *params->listIgnored_ref(), enforceParents, request)
            .thenValue([this, mount](std::unique_ptr<ScmStatus>&& status) {
              auto result = std::make_unique<GetScmStatusResult>();
              *result->status_ref() = std::move(*status);
              *result->version_ref() = server_->getVersion();
              return result;
            }));
  })
      .thenTry([cb = std::move(callback)](
                   folly::Try<std::unique_ptr<GetScmStatusResult>>&&
                       result) mutable {
        apache::thrift::HandlerCallback<std::unique_ptr<GetScmStatusResult>>::
            completeInThread(std::move(cb), std::move(result));
      });
}

void EdenServiceHandler::async_tm_getScmStatus(
    unique_ptr<apache::thrift::HandlerCallback<unique_ptr<ScmStatus>>> callback,
    unique_ptr<string> mountPoint,
    bool listIgnored,
    unique_ptr<string> commitHash) {
  auto* request = callback->getRequest();
  folly::makeFutureWith([&, func = __func__] {
    auto helper = INSTRUMENT_THRIFT_CALL_WITH_FUNCTION_NAME(
        DBG2,
        func,
        *mountPoint,
        folly::to<string>("listIgnored=", listIgnored ? "true" : "false"),
        folly::to<string>("commitHash=", logHash(*commitHash)));

    // Unlike getScmStatusV2(), this older getScmStatus() call does not enforce
    // that the caller specified the current commit.  In the future we might
    // want to enforce that even for this call, if we confirm that all existing
    // callers of this method can deal with the error.
    auto mount = server_->getMount(*mountPoint);
    auto hash = hashFromThrift(*commitHash);
    return helper.wrapFuture(mount->diff(
        hash, listIgnored, /*enforceCurrentParent=*/false, request));
  })
      .thenTry([cb = std::move(callback)](
                   folly::Try<std::unique_ptr<ScmStatus>>&& result) mutable {
        apache::thrift::HandlerCallback<std::unique_ptr<ScmStatus>>::
            completeInThread(std::move(cb), std::move(result));
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
  return helper.wrapFuture(
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

  std::shared_ptr<const Tree> tree;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    tree = localStore->getTree(id).get();
  } else {
    tree = store->getTree(id, ObjectFetchContext::getNullContext()).get();
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
    *out.name_ref() = entry.getName().stringPiece().str();
    *out.mode_ref() = modeFromTreeEntryType(entry.getType());
    *out.id_ref() = thriftHash(entry.getHash());
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

  std::shared_ptr<const Blob> blob;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    blob = localStore->getBlob(id).get();
  } else {
    blob = store->getBlob(id, ObjectFetchContext::getNullContext()).get();
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

  std::optional<BlobMetadata> metadata;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    metadata = localStore->getBlobMetadata(id).get();
  } else {
    auto sha1 =
        store->getBlobSha1(id, ObjectFetchContext::getNullContext()).get();
    auto size =
        store->getBlobSize(id, ObjectFetchContext::getNullContext()).get();
    metadata.emplace(sha1, size);
  }

  if (!metadata.has_value()) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no blob metadata found for id ",
        id.toString());
  }
  *result.size_ref() = metadata->size;
  *result.contentsSha1_ref() = thriftHash(metadata->sha1);
}

void EdenServiceHandler::debugInodeStatus(
    vector<TreeInodeDebugInfo>& inodeInfo,
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path) {
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);
  auto edenMount = server_->getMount(*mountPoint);

  auto inode = inodeFromUserPath(*edenMount, *path).asTreePtr();
  inode->getDebugStatus(inodeInfo);
}

void EdenServiceHandler::debugOutstandingFuseCalls(
    std::vector<FuseCall>& outstandingCalls,
    std::unique_ptr<std::string> mountPoint) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG2);

  auto edenMount = server_->getMount(*mountPoint);
  auto* fuseChannel = edenMount->getFuseChannel();
  std::vector<fuse_in_header> fuseOutstandingCalls =
      fuseChannel->getOutstandingRequests();
  FuseCall fuseCall;

  for (const auto& call : fuseOutstandingCalls) {
    // Convert from fuse_in_header to fuseCall
    // Conversion is done here to avoid building a dependency between
    // FuseChannel and thrift

    *fuseCall.len_ref() = call.len;
    *fuseCall.opcode_ref() = call.opcode;
    *fuseCall.unique_ref() = call.unique;
    *fuseCall.nodeid_ref() = call.nodeid;
    *fuseCall.uid_ref() = call.uid;
    *fuseCall.gid_ref() = call.gid;
    *fuseCall.pid_ref() = call.pid;

    outstandingCalls.push_back(fuseCall);
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
  *info.loaded_ref() = inodeMap->lookupLoadedInode(inodeNum) != nullptr;
  // If getPathForInode returned none then the inode is unlinked
  *info.linked_ref() = relativePath != std::nullopt;
  *info.path_ref() = relativePath ? relativePath->stringPiece().str() : "";
}

void EdenServiceHandler::getAccessCounts(
    GetAccessCountsResult& result,
    int64_t duration) {
#ifndef _WIN32
  auto helper = INSTRUMENT_THRIFT_CALL(DBG3);

  *result.cmdsByPid_ref() =
      server_->getServerState()->getProcessNameCache()->getAllProcessNames();

  auto seconds = std::chrono::seconds{duration};

  for (auto& mount : server_->getMountPoints()) {
    auto& mountStr = mount->getPath().value();
    auto& pal = mount->getFuseChannel()->getProcessAccessLog();

    auto& pidFetches = mount->getObjectStore()->getPidFetches();

    MountAccesses& ma = result.accessesByMount_ref()[mountStr];
    for (auto& [pid, accessCounts] : pal.getAccessCounts(seconds)) {
      ma.accessCountsByPid_ref()[pid] = accessCounts;
    }

    for (auto& [pid, fetchCount] : *pidFetches.rlock()) {
      ma.fetchCountsByPid_ref()[pid] = fetchCount;
    }
  }
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
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
    *mountInodeInfo.unloadedInodeCount_ref() = counts.unloadedInodeCount;
    *mountInodeInfo.loadedFileCount_ref() = counts.fileCount;
    *mountInodeInfo.loadedTreeCount_ref() = counts.treeCount;

    JournalInfo journalThrift;
    if (auto journalStats = mount->getJournal().getStats()) {
      *journalThrift.entryCount_ref() = journalStats->entryCount;
      *journalThrift.durationSeconds_ref() =
          journalStats->getDurationInSeconds();
    } else {
      *journalThrift.entryCount_ref() = 0;
      *journalThrift.durationSeconds_ref() = 0;
    }
    *journalThrift.memoryUsage_ref() =
        mount->getJournal().estimateMemoryUsage();
    result.mountPointJournalInfo_ref()[mount->getPath().stringPiece().str()] =
        journalThrift;

    result.mountPointInfo_ref()[mount->getPath().stringPiece().str()] =
        mountInodeInfo;
  }
  // Get the counters and set number of inodes unloaded by periodic unload job.
  *result.counters_ref() = fb303::ServiceData::get()->getCounters();
  *result.periodicUnloadCount_ref() =
      result.counters_ref()[kPeriodicUnloadCounterKey.toString()];

  auto privateDirtyBytes = facebook::eden::proc_util::calculatePrivateBytes();
  if (privateDirtyBytes) {
    *result.privateBytes_ref() = privateDirtyBytes.value();
  }

  auto memoryStats = facebook::eden::proc_util::readMemoryStats();
  if (memoryStats) {
    *result.vmRSSBytes_ref() = memoryStats->resident;
  }

  // Note: this will be removed in a subsequent commit.
  // We now report periodically via ServiceData
  std::string smaps;
  if (folly::readFile("/proc/self/smaps", smaps)) {
    *result.smaps_ref() = std::move(smaps);
  }

  const auto blobCacheStats = server_->getBlobCache()->getStats();
  *result.blobCacheStats_ref()->entryCount_ref() = blobCacheStats.blobCount;
  *result.blobCacheStats_ref()->totalSizeInBytes_ref() =
      blobCacheStats.totalSizeInBytes;
  *result.blobCacheStats_ref()->hitCount_ref() = blobCacheStats.hitCount;
  *result.blobCacheStats_ref()->missCount_ref() = blobCacheStats.missCount;
  *result.blobCacheStats_ref()->evictionCount_ref() =
      blobCacheStats.evictionCount;
  *result.blobCacheStats_ref()->dropCount_ref() = blobCacheStats.dropCount;
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

#ifndef _WIN32
  float uptime = UnixClock::getElapsedTimeInNs(
      server_->getStartTime(), UnixClock().getRealtime());
  result.uptime_ref() = uptime;
#endif // !_WIN32
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

} // namespace eden
} // namespace facebook
