/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenServiceHandler.h"
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/container/Access.h>
#include <folly/experimental/logging/Logger.h>
#include <folly/experimental/logging/LoggerDB.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <folly/stop_watch.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/Differ.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/GlobNode.h"
#include "eden/fs/service/StreamingSubscriber.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"

using folly::Future;
using folly::makeFuture;
using folly::Optional;
using folly::StringPiece;
using std::make_unique;
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
} // namespace

#define TLOG(logger, level, file, line) \
  FB_LOG_RAW(logger, level, file, line) \
      << "[" << folly::RequestContext::get() << "] "

namespace /* anonymous namespace for helper functions */ {

// Helper class to log where the request completes in Future
class ThriftLogHelper {
 public:
  template <typename... Args>
  ThriftLogHelper(
      folly::LogLevel level,
      folly::StringPiece itcFunctionName,
      folly::StringPiece itcFileName,
      uint32_t itcLineNumber,
      const Args&... rest)
      : itcFunctionName_(itcFunctionName),
        itcFileName_(itcFileName),
        itcLineNumber_(itcLineNumber),
        level_(level),
        itcLogger_("eden.thrift." + itcFunctionName_.str()) {
    TLOG(itcLogger_, level_, itcFileName_, itcLineNumber_)
        << itcFunctionName_ << "(" << toDelimWrapper(rest...) << ")";
  }

  ~ThriftLogHelper() {
    if (wrapperExecuted_) {
      // Logging of future creation at folly::LogLevel::DBG3.
      TLOG(itcLogger_, folly::LogLevel::DBG3, itcFileName_, itcLineNumber_)
          << itcFunctionName_ << "() created future "
          << itcTimer_.elapsed().count() << "ms";
    } else {
      // If this object was not used for future creation
      // log the elaped time here.
      TLOG(itcLogger_, level_, itcFileName_, itcLineNumber_)
          << itcFunctionName_ << "() took " << itcTimer_.elapsed().count()
          << "ms";
    }
  }

  template <typename ReturnType>
  Future<ReturnType> wrapFuture(folly::Future<ReturnType>&& f) {
    wrapperExecuted_ = true;
    return f.then([timer = itcTimer_,
                   logger = this->itcLogger_,
                   funcName = itcFunctionName_,
                   level = level_,
                   filename = itcFileName_,
                   linenumber = itcLineNumber_](ReturnType&& ret) {
      // Logging completion time for the request
      // The line number points to where the object was originally created
      TLOG(logger, level, filename, linenumber)
          << funcName << "() took " << timer.elapsed().count() << "ms";
      return std::forward<ReturnType>(ret);
    });
  }

 private:
  folly::StringPiece itcFunctionName_;
  folly::StringPiece itcFileName_;
  uint32_t itcLineNumber_;
  folly::LogLevel level_;
  folly::Logger itcLogger_;
  folly::stop_watch<std::chrono::milliseconds> itcTimer_ = {};
  bool wrapperExecuted_ = false;
};

} // namespace

// INSTRUMENT_THRIFT_CALL returns a unique pointer to
// ThriftLogHelper object. The returned pointer can be used to call wrapFuture()
// to attach a log message on the completion of the Future.

// When not attached to Future it will log the completion of the operation and
// time taken to complete it.

#define INSTRUMENT_THRIFT_CALL(level, ...)                          \
  ([&](folly::StringPiece functionName,                             \
       folly::StringPiece fileName,                                 \
       uint32_t lineNumber) {                                       \
    return (std::make_unique<ThriftLogHelper>(                      \
        level, functionName, fileName, lineNumber, ##__VA_ARGS__)); \
  }(__func__, __FILE__, __LINE__))

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG4);
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  auto helper =
      INSTRUMENT_THRIFT_CALL(folly::LogLevel::INFO, info->get_mountPoint());
  try {
    auto initialConfig = ClientConfig::loadFromClientDirectory(
        AbsolutePathPiece{info->mountPoint},
        AbsolutePathPiece{info->edenClientPath});
    server_->mount(std::move(initialConfig)).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::INFO, *mountPoint);
  try {
    server_->unmount(*mountPoint).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  for (const auto& edenMount : server_->getMountPoints()) {
    MountInfo info;
    info.mountPoint = edenMount->getPath().stringPiece().str();
    // TODO: Fill in info.edenClientPath.
    // I'll add that in a future diff, once we have a custom MountPoint
    // subclass that isn't in the low-level fusell namespace.
    results.push_back(info);
  }
}

void EdenServiceHandler::checkOutRevision(
    std::vector<CheckoutConflict>& results,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash,
    CheckoutMode checkoutMode) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG1,
      *mountPoint,
      hashFromThrift(*hash).toString(),
      folly::get_default(
          _CheckoutMode_VALUES_TO_NAMES, checkoutMode, "(unknown)"));
  auto hashObj = hashFromThrift(*hash);

  auto edenMount = server_->getMount(*mountPoint);
  auto checkoutFuture = edenMount->checkout(hashObj, checkoutMode);
  results = checkoutFuture.get();
}

void EdenServiceHandler::resetParentCommits(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<WorkingDirectoryParents> parents) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG1,
      *mountPoint,
      hashFromThrift(parents->parent1).toString());
  ParentCommits edenParents;
  edenParents.parent1() = hashFromThrift(parents->parent1);
  if (parents->__isset.parent2) {
    edenParents.parent2() = hashFromThrift(parents->parent2);
  }
  auto edenMount = server_->getMount(*mountPoint);
  edenMount->resetParents(edenParents);
}

void EdenServiceHandler::getSHA1(
    vector<SHA1Result>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> paths) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG3,
      *mountPoint,
      "[" + folly::join(", ", *paths.get()) + "]");

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
  // Calls getSHA1ForPath() and traps all immediate exceptions and converts
  // them in to a Future result.
  try {
    return getSHA1ForPath(mountPoint, path);
  } catch (const std::system_error& e) {
    return makeFuture<Hash>(newEdenError(e));
  }
}

Future<Hash> EdenServiceHandler::getSHA1ForPath(
    StringPiece mountPoint,
    StringPiece path) {
  if (path.empty()) {
    return makeFuture<Hash>(
        newEdenError(EINVAL, "path cannot be the empty string"));
  }

  auto edenMount = server_->getMount(mountPoint);
  auto relativePath = RelativePathPiece{path};
  return edenMount->getInode(relativePath).then([](const InodePtr& inode) {
    auto fileInode = inode.asFilePtr();
    if (!S_ISREG(fileInode->getMode())) {
      // We intentionally want to refuse to compute the SHA1 of symlinks
      return makeFuture<Hash>(
          InodeError(EINVAL, fileInode, "file is a symlink"));
    }
    return fileInode->getSha1();
  });
}

void EdenServiceHandler::getBindMounts(
    std::vector<string>& out,
    std::unique_ptr<string> mountPointPtr) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3, *mountPointPtr);
  auto mountPoint = *mountPointPtr.get();
  auto mountPointPath = AbsolutePathPiece{mountPoint};
  auto edenMount = server_->getMount(mountPoint);

  for (auto& bindMount : edenMount->getBindMounts()) {
    out.emplace_back(mountPointPath.relativize(bindMount.pathInMountDir)
                         .stringPiece()
                         .str());
  }
}

void EdenServiceHandler::getCurrentJournalPosition(
    JournalPosition& out,
    std::unique_ptr<std::string> mountPoint) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto latest = edenMount->getJournal().getLatest();

  out.mountGeneration = edenMount->getMountGeneration();
  out.sequenceNumber = latest->toSequence;
  out.snapshotHash = thriftHash(latest->toHash);
}

void EdenServiceHandler::async_tm_subscribe(
    std::unique_ptr<apache::thrift::StreamingHandlerCallback<
        std::unique_ptr<JournalPosition>>> callback,
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);

  // StreamingSubscriber manages the subscription lifetime and releases itself
  // as appropriate.
  StreamingSubscriber::subscribe(std::move(callback), std::move(edenMount));
}

void EdenServiceHandler::getFilesChangedSince(
    FileDelta& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<JournalPosition> fromPosition) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG2, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto delta = edenMount->getJournal().getLatest();

  if (fromPosition->mountGeneration != edenMount->getMountGeneration()) {
    throw newEdenError(
        ERANGE,
        "fromPosition.mountGeneration does not match the current "
        "mountGeneration.  "
        "You need to compute a new basis for delta queries.");
  }

  out.toPosition.sequenceNumber = delta->toSequence;
  out.toPosition.snapshotHash = thriftHash(delta->toHash);
  out.toPosition.mountGeneration = edenMount->getMountGeneration();

  out.fromPosition = out.toPosition;

  // The +1 is because the core merge stops at the item prior to
  // its limitSequence parameter and we want the changes *since*
  // the provided sequence number.
  auto merged = delta->merge(fromPosition->sequenceNumber + 1, true);
  if (merged) {
    out.fromPosition.sequenceNumber = merged->fromSequence;
    out.fromPosition.snapshotHash = thriftHash(merged->fromHash);
    out.fromPosition.mountGeneration = out.toPosition.mountGeneration;

    for (auto& path : merged->changedFilesInOverlay) {
      out.changedPaths.emplace_back(path.stringPiece().str());
    }

    for (auto& path : merged->createdFilesInOverlay) {
      out.createdPaths.emplace_back(path.stringPiece().str());
    }

    for (auto& path : merged->removedFilesInOverlay) {
      out.removedPaths.emplace_back(path.stringPiece().str());
    }

    for (auto& path : merged->uncleanPaths) {
      out.uncleanPaths.emplace_back(path.stringPiece().str());
    }
  }
}

void EdenServiceHandler::getFileInformation(
    std::vector<FileInformationOrError>& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG3,
      *mountPoint,
      "[" + folly::join(", ", *paths.get()) + "]");
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  for (auto& path : *paths) {
    FileInformationOrError result;

    try {
      auto relativePath = RelativePathPiece{path};
      auto inodeBase = edenMount->getInodeBlocking(relativePath);

      // we've reached the item of interest.
      auto attr = inodeBase->getattr().get();
      FileInformation info;
      info.size = attr.st.st_size;
      info.mtime.seconds = attr.st.st_mtim.tv_sec;
      info.mtime.nanoSeconds = attr.st.st_mtim.tv_nsec;
      info.mode = attr.st.st_mode;

      result.set_info(info);
      out.emplace_back(std::move(result));

    } catch (const std::system_error& e) {
      result.set_error(newEdenError(e));
      out.emplace_back(std::move(result));
    }
  }
}

void EdenServiceHandler::glob(
    vector<string>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> globs) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG3,
      *mountPoint,
      "[" + folly::join(", ", *globs.get()) + "]");
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  // Compile the list of globs into a tree
  GlobNode globRoot;
  for (auto& globString : *globs) {
    globRoot.parse(globString);
  }

  // and evaluate it against the root
  auto matches = globRoot.evaluate(RelativePathPiece(), rootInode).get();
  for (auto& fileName : matches) {
    out.emplace_back(fileName.stringPiece().toString());
  }
}

void EdenServiceHandler::getManifestEntry(
    ManifestEntry& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePath) {
  auto helper =
      INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3, *mountPoint, *relativePath);
  auto mount = server_->getMount(*mountPoint);
  auto filename = RelativePathPiece{*relativePath};
  auto mode = isInManifestAsFile(mount.get(), filename);
  if (mode.hasValue()) {
    out.mode = mode.value();
  } else {
    NoValueForKeyError error;
    error.set_key(*relativePath);
    throw error;
  }
}

// TODO(mbolin): Make this a method of ObjectStore and make it Future-based.
folly::Optional<mode_t> EdenServiceHandler::isInManifestAsFile(
    const EdenMount* mount,
    const RelativePathPiece filename) {
  auto tree = mount->getRootTree();
  auto parentDirectory = filename.dirname();
  auto objectStore = mount->getObjectStore();
  for (auto piece : parentDirectory.components()) {
    auto entry = tree->getEntryPtr(piece);
    if (entry != nullptr && entry->isTree()) {
      tree = objectStore->getTree(entry->getHash()).get();
    } else {
      return folly::none;
    }
  }

  if (tree != nullptr) {
    auto entry = tree->getEntryPtr(filename.basename());
    if (entry != nullptr && !entry->isTree()) {
      return modeFromTreeEntryType(entry->getType());
    }
  }

  return folly::none;
}

folly::Future<std::unique_ptr<ScmStatus>>
EdenServiceHandler::future_getScmStatus(
    std::unique_ptr<std::string> mountPoint,
    bool listIgnored) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG2,
      *mountPoint,
      folly::format("listIgnored={}", listIgnored ? "true" : "false"));

  auto mount = server_->getMount(*mountPoint);
  return helper->wrapFuture(diffMountForStatus(mount.get(), listIgnored));
}

folly::Future<std::unique_ptr<ScmStatus>>
EdenServiceHandler::future_getScmStatusBetweenRevisions(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> oldHash,
    std::unique_ptr<std::string> newHash) {
  auto helper = INSTRUMENT_THRIFT_CALL(
      folly::LogLevel::DBG2,
      *mountPoint,
      folly::format("oldHash={}, newHash={}", *oldHash, *newHash));
  auto mount = server_->getMount(*mountPoint);
  return helper->wrapFuture(
      diffRevisions(mount.get(), Hash{*oldHash}, Hash{*newHash}));
}

void EdenServiceHandler::debugGetScmTree(
    vector<ScmTreeEntry>& entries,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  std::shared_ptr<const Tree> tree;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    tree = localStore->getTree(id);
  } else {
    tree = store->getTree(id).get();
  }

  if (!tree) {
    throw newEdenError("no tree found for id ", *idStr);
  }

  for (const auto& entry : tree->getTreeEntries()) {
    entries.emplace_back();
    auto& out = entries.back();
    out.name = entry.getName().stringPiece().str();
    out.mode = modeFromTreeEntryType(entry.getType());
    out.id = thriftHash(entry.getHash());
  }
}

void EdenServiceHandler::debugGetScmBlob(
    string& data,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  std::shared_ptr<const Blob> blob;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    blob = localStore->getBlob(id);
  } else {
    blob = store->getBlob(id).get();
  }

  if (!blob) {
    throw newEdenError("no blob found for id ", *idStr);
  }
  auto dataBuf = blob->getContents().cloneCoalescedAsValue();
  data.assign(reinterpret_cast<const char*>(dataBuf.data()), dataBuf.length());
}

void EdenServiceHandler::debugGetScmBlobMetadata(
    ScmBlobMetadata& result,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto edenMount = server_->getMount(*mountPoint);
  auto id = hashFromThrift(*idStr);

  Optional<BlobMetadata> metadata;
  auto store = edenMount->getObjectStore();
  if (localStoreOnly) {
    auto localStore = store->getLocalStore();
    metadata = localStore->getBlobMetadata(id);
  } else {
    metadata = store->getBlobMetadata(id).get();
  }

  if (!metadata.hasValue()) {
    throw newEdenError("no blob metadata found for id ", *idStr);
  }
  result.size = metadata->size;
  result.contentsSha1 = thriftHash(metadata->sha1);
}

void EdenServiceHandler::debugInodeStatus(
    vector<TreeInodeDebugInfo>& inodeInfo,
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto edenMount = server_->getMount(*mountPoint);

  TreeInodePtr inode;
  if (path->empty()) {
    inode = edenMount->getRootInode();
  } else {
    inode = edenMount->getInode(RelativePathPiece{*path}).get().asTreePtr();
  }

  inode->getDebugStatus(inodeInfo);
}

void EdenServiceHandler::debugGetInodePath(
    InodePathDebugInfo& info,
    std::unique_ptr<std::string> mountPoint,
    int64_t inodeNumber) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto inodeNum = static_cast<InodeNumber>(inodeNumber);
  auto inodeMap = server_->getMount(*mountPoint)->getInodeMap();

  folly::Optional<RelativePath> relativePath =
      inodeMap->getPathForInode(inodeNum);
  // Check if the inode is loaded
  info.loaded = inodeMap->lookupLoadedInode(inodeNum) != nullptr;
  // If getPathForInode returned folly::none then the inode is unlinked
  info.linked = relativePath != folly::none;

  info.path = relativePath ? relativePath->stringPiece().str() : "";
}

void EdenServiceHandler::debugSetLogLevel(
    SetLogLevelResult& result,
    std::unique_ptr<std::string> category,
    std::unique_ptr<std::string> level) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG1);
  // TODO: This is a temporary hack until Adam's upcoming log config parser
  // is ready.
  bool inherit = true;
  if (level->length() && '!' == level->back()) {
    *level = level->substr(0, level->length() - 1);
    inherit = false;
  }

  auto& db = folly::LoggerDB::get();
  result.categoryCreated = !db.getCategoryOrNull(*category);
  folly::Logger(*category).getCategory()->setLevel(
      folly::stringToLogLevel(*level), inherit);
}

int64_t EdenServiceHandler::unloadInodeForPath(
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path,
    std::unique_ptr<TimeSpec> age) {
  auto helper =
      INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG1, *mountPoint, *path);
  auto edenMount = server_->getMount(*mountPoint);

  TreeInodePtr inode;
  if (path->empty()) {
    inode = edenMount->getRootInode();
  } else {
    inode = edenMount->getInode(RelativePathPiece{*path}).get().asTreePtr();
  }
  auto cutoff = std::chrono::system_clock::now() -
      std::chrono::seconds(age->seconds) -
      std::chrono::nanoseconds(age->nanoSeconds);
  auto cutoff_ts = folly::to<timespec>(cutoff);
  return inode->unloadChildrenLastAccessedBefore(cutoff_ts);
}

void EdenServiceHandler::getStatInfo(InternalStats& result) {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  auto mountList = server_->getMountPoints();
  for (auto& mount : mountList) {
    auto inodeMap = mount->getInodeMap();
    // Set LoadedInde Count and unloaded Inode count for the mountPoint.
    MountInodeInfo mountInodeInfo;
    auto counts = inodeMap->getLoadedInodeCounts();
    mountInodeInfo.loadedInodeCount = counts.fileCount + counts.treeCount;
    mountInodeInfo.unloadedInodeCount = inodeMap->getUnloadedInodeCount();
    mountInodeInfo.loadedFileCount = counts.fileCount;
    mountInodeInfo.loadedTreeCount = counts.treeCount;

    // TODO: Currently getting Materialization status of an inode using
    // getDebugStatus which walks through entire Tree of inodes, in future we
    // can add some mechanism to get materialized inode count without walking
    // through the entire tree.
    vector<TreeInodeDebugInfo> debugInfoStatus;
    auto root = mount->getRootInode();
    root->getDebugStatus(debugInfoStatus);
    uint64_t materializedCount = 0;
    for (auto& entry : debugInfoStatus) {
      if (entry.materialized) {
        materializedCount++;
      }
    }
    mountInodeInfo.materializedInodeCount = materializedCount;
    result.mountPointInfo[mount->getPath().stringPiece().str()] =
        mountInodeInfo;
  }
  // Get the counters and set number of inodes unloaded by periodic unload job.
  result.counters = stats::ServiceData::get()->getCounters();
  result.periodicUnloadCount =
      result.counters[kPeriodicUnloadCounterKey.toString()];

  // TODO: Linux-only
  std::string smaps;
  if (folly::readFile("/proc/self/smaps", smaps)) {
    result.smaps = std::move(smaps);
  }
}

void EdenServiceHandler::flushStatsNow() {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::DBG3);
  server_->flushStatsNow();
}

void EdenServiceHandler::invalidateKernelInodeCache(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> path) {
  auto edenMount = server_->getMount(*mountPoint);
  InodePtr inode;
  if (path->empty()) {
    inode = edenMount->getRootInode();
  } else {
    inode = edenMount->getInode(RelativePathPiece{*path}).get();
  }
  auto* fuseChannel = edenMount->getFuseChannel();

  // Invalidate cached pages and attributes
  fuseChannel->invalidateInode(inode->getNodeId(), 0, 0);

  const auto treePtr = inode.asTreePtrOrNull();

  // invalidate all parent/child relationships potentially cached.
  if (treePtr != nullptr) {
    const auto& dir = treePtr->getContents().rlock();
    for (const auto& entry : dir->entries) {
      fuseChannel->invalidateEntry(inode->getNodeId(), entry.first);
    }
  }
}

void EdenServiceHandler::shutdown() {
  auto helper = INSTRUMENT_THRIFT_CALL(folly::LogLevel::INFO);
  server_->stop();
}
} // namespace eden
} // namespace facebook
