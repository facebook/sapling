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
using folly::Optional;
using folly::StringPiece;
using folly::makeFuture;
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

#define TLOG(level) \
  FB_LOG(_itcLogger, level) << "[" << folly::RequestContext::get() << "] "

// This macro must be used on a line by itself at the start of a Thrift endpoint
// method. Log calls in each method should use TLOG() instead of XLOG(LEVEL).
//
// Using TLOG() throughout your method will ensure the messages for the
// Thrift endpoint can be controlled via an endpoint-specific log category.
// Note this will also log the duration of the Thrift call.
#define INSTRUMENT_THRIFT_CALL(level, ...)                                   \
  /* This is needed because __func__ has a different value in SCOPE_EXIT. */ \
  static folly::StringPiece _itcFunctionName{__func__};                      \
  static folly::Logger _itcLogger("eden.thrift." + _itcFunctionName.str());  \
  auto _itcTimer = folly::stop_watch<std::chrono::milliseconds>{};           \
  {                                                                          \
    TLOG(level) << _itcFunctionName << "(" << toDelimWrapper(__VA_ARGS__)    \
                << ")";                                                      \
  }                                                                          \
  SCOPE_EXIT {                                                               \
    TLOG(level) << _itcFunctionName << "() took "                            \
                << _itcTimer.elapsed().count() << "ms";                      \
  }

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  INSTRUMENT_THRIFT_CALL(DBG4);
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  INSTRUMENT_THRIFT_CALL(INFO, info->get_mountPoint());
  try {
    server_->mount(*info).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  INSTRUMENT_THRIFT_CALL(INFO, *mountPoint);
  try {
    server_->unmount(*mountPoint).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  INSTRUMENT_THRIFT_CALL(DBG3);
  for (const auto& edenMount : server_->getMountPoints()) {
    MountInfo info;
    info.mountPoint = edenMount->getPath().stringPiece().str();
    // TODO: Fill in info.edenClientPath.
    // I'll add that in a future diff, once we have a custom MountPoint
    // subclass that isn't in the low-level fusell namespace.
    results.push_back(info);
  }
}

void EdenServiceHandler::getParentCommits(
    WorkingDirectoryParents& result,
    std::unique_ptr<std::string> mountPoint) {
  INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto parents = edenMount->getParentCommits();
  result.set_parent1(thriftHash(parents.parent1()));
  if (parents.parent2().hasValue()) {
    result.set_parent2(thriftHash(parents.parent2().value()));
  }
}

void EdenServiceHandler::checkOutRevision(
    std::vector<CheckoutConflict>& results,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash,
    bool force) {
  INSTRUMENT_THRIFT_CALL(
      DBG1,
      *mountPoint,
      hashFromThrift(*hash).toString(),
      folly::format("force={}", force ? "true" : "false"));
  auto hashObj = hashFromThrift(*hash);

  auto edenMount = server_->getMount(*mountPoint);
  auto checkoutFuture = edenMount->checkout(hashObj, force);
  results = checkoutFuture.get();
}

void EdenServiceHandler::resetParentCommits(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<WorkingDirectoryParents> parents) {
  INSTRUMENT_THRIFT_CALL(
      DBG1, *mountPoint, hashFromThrift(parents->parent1).toString());
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
  INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, "[" + folly::join(", ", *paths.get()) + "]");

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
  INSTRUMENT_THRIFT_CALL(DBG3, *mountPointPtr);
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
  INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto latest = edenMount->getJournal().rlock()->getLatest();

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
  INSTRUMENT_THRIFT_CALL(DBG2, *mountPoint);
  auto edenMount = server_->getMount(*mountPoint);
  auto delta = edenMount->getJournal().rlock()->getLatest();

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
  INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, "[" + folly::join(", ", *paths.get()) + "]");
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
  INSTRUMENT_THRIFT_CALL(
      DBG3, *mountPoint, "[" + folly::join(", ", *globs.get()) + "]");
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
  INSTRUMENT_THRIFT_CALL(DBG3, *mountPoint, *relativePath);
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
    if (entry != nullptr && entry->getFileType() == FileType::DIRECTORY) {
      tree = objectStore->getTree(entry->getHash()).get();
    } else {
      return folly::none;
    }
  }

  if (tree != nullptr) {
    auto entry = tree->getEntryPtr(filename.basename());
    if (entry != nullptr && entry->getFileType() != FileType::DIRECTORY) {
      return entry->getMode();
    }
  }

  return folly::none;
}

folly::Future<std::unique_ptr<ScmStatus>>
EdenServiceHandler::future_getScmStatus(
    std::unique_ptr<std::string> mountPoint,
    bool listIgnored) {
  INSTRUMENT_THRIFT_CALL(
      DBG2,
      *mountPoint,
      folly::format("listIgnored={}", listIgnored ? "true" : "false"));
  auto mount = server_->getMount(*mountPoint);
  return diffMountForStatus(mount.get(), listIgnored);
}

void EdenServiceHandler::debugGetScmTree(
    vector<ScmTreeEntry>& entries,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  INSTRUMENT_THRIFT_CALL(DBG3);
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
    out.mode = entry.getMode();
    out.id = thriftHash(entry.getHash());
  }
}

void EdenServiceHandler::debugGetScmBlob(
    string& data,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
  INSTRUMENT_THRIFT_CALL(DBG3);
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
  INSTRUMENT_THRIFT_CALL(DBG3);
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
  INSTRUMENT_THRIFT_CALL(DBG3);
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
  INSTRUMENT_THRIFT_CALL(DBG3);
  auto inodeNum = static_cast<fuse_ino_t>(inodeNumber);
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
  INSTRUMENT_THRIFT_CALL(DBG1);
  // TODO: This is a temporary hack until Adam's upcoming log config parser
  // is ready.
  bool inherit = true;
  if (level->length() && '!' == level->back()) {
    *level = level->substr(0, level->length() - 1);
    inherit = false;
  }

  auto db = folly::LoggerDB::get();
  result.categoryCreated = !db->getCategoryOrNull(*category);
  folly::Logger(*category).getCategory()->setLevel(
      folly::stringToLogLevel(*level), inherit);
}

int64_t EdenServiceHandler::unloadInodeForPath(
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path,
    std::unique_ptr<TimeSpec> age) {
  INSTRUMENT_THRIFT_CALL(DBG1, *mountPoint, *path);
  auto edenMount = server_->getMount(*mountPoint);

  TreeInodePtr inode;
  if (path->empty()) {
    inode = edenMount->getRootInode();
  } else {
    inode = edenMount->getInode(RelativePathPiece{*path}).get().asTreePtr();
  }
  // Convert age to std::chrono::nanoseconds.
  std::chrono::seconds sec(age->seconds);
  std::chrono::nanoseconds nsec(age->nanoSeconds);
  return inode->unloadChildrenNow(sec + nsec);
}

void EdenServiceHandler::getStatInfo(InternalStats& result) {
  INSTRUMENT_THRIFT_CALL(DBG3);
  auto mountList = server_->getMountPoints();
  for (auto& mount : mountList) {
    // Set LoadedInde Count and unloaded Inode count for the mountPoint.
    MountInodeInfo mountInodeInfo;
    mountInodeInfo.loadedInodeCount = stats::ServiceData::get()->getCounter(
        mount->getCounterName(CounterName::LOADED));
    mountInodeInfo.unloadedInodeCount = stats::ServiceData::get()->getCounter(
        mount->getCounterName(CounterName::UNLOADED));

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
}

void EdenServiceHandler::flushStatsNow() {
  INSTRUMENT_THRIFT_CALL(DBG3);
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
  INSTRUMENT_THRIFT_CALL(INFO);
  server_->stop();
}
} // namespace eden
} // namespace facebook
