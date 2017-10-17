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

#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <unordered_set>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/DirstatePersistence.h"
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

using std::make_unique;
using std::string;
using std::unique_ptr;
using std::vector;
using folly::Optional;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using facebook::eden::hgdirstate::DirstateNonnormalFileStatus;
using facebook::eden::hgdirstate::DirstateMergeState;
using facebook::eden::hgdirstate::DirstateTuple;
using facebook::eden::hgdirstate::_DirstateNonnormalFileStatus_VALUES_TO_NAMES;
using facebook::eden::hgdirstate::_DirstateMergeState_VALUES_TO_NAMES;

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  try {
    server_->mount(*info).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

/**
 * The path to the metadata for this mount is available at
 * ~/.eden/clients/CLIENT_HASH.
 */
AbsolutePath EdenServiceHandler::getPathToDirstateStorage(
    AbsolutePathPiece mountPointPath) {
  // We need to take the sha-1 of the utf-8 version of path.
  folly::ByteRange bytes(mountPointPath.stringPiece());
  auto sha1 = Hash::sha1(bytes);
  auto component = PathComponent(sha1.toString());

  return server_->getEdenDir() + PathComponent("clients") + component +
      PathComponent("dirstate");
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  try {
    server_->unmount(*mountPoint).get();
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
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
  auto hashObj = hashFromThrift(*hash);

  auto edenMount = server_->getMount(*mountPoint);
  auto checkoutFuture = edenMount->checkout(hashObj, force);
  results = checkoutFuture.get();
}

void EdenServiceHandler::resetParentCommits(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<WorkingDirectoryParents> parents) {
  ParentCommits edenParents;
  edenParents.parent1() = hashFromThrift(parents->parent1);
  if (parents->__isset.parent2) {
    edenParents.parent2() = hashFromThrift(parents->parent2);
  }
  auto edenMount = server_->getMount(*mountPoint);
  edenMount->resetParents(edenParents).get();
}

void EdenServiceHandler::getSHA1(
    vector<SHA1Result>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> paths) {
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
    return fileInode->getSHA1();
  });
}

void EdenServiceHandler::getBindMounts(
    std::vector<string>& out,
    std::unique_ptr<string> mountPointPtr) {
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
  auto delta = edenMount->getJournal().rlock()->getLatest();

  auto sub = std::make_shared<StreamingSubscriber>(
      std::move(callback), std::move(edenMount));
  // The subscribe call sets up a journal subscriber which captures
  // a reference to the `sub` shared_ptr.  This keeps it alive for
  // the duration of the subscription so that it doesn't get immediately
  // deleted when sub falls out of scope at the bottom of this method call.
  sub->subscribe();
}

void EdenServiceHandler::getFilesChangedSince(
    FileDelta& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<JournalPosition> fromPosition) {
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

void EdenServiceHandler::scmGetStatus(
    ThriftHgStatus& out,
    std::unique_ptr<std::string> mountPoint,
    bool listIgnored) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  out = dirstate->getStatus(listIgnored);
  XLOG(INFO) << "scmGetStatus() returning " << out;
}

void EdenServiceHandler::hgClearDirstate(
    std::unique_ptr<std::string> mountPoint) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  XLOG(DBG2) << "hgClearDirstate(" << *mountPoint << ")";
  dirstate->clear();
}

void EdenServiceHandler::hgGetDirstateTuple(
    DirstateTuple& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePath) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  auto filename = RelativePathPiece{*relativePath};
  try {
    out = dirstate->hgGetDirstateTuple(filename);
    // Print this before invoking hgGetDirstateTuple(), as it may throw.
    XLOG(DBG2) << "hgGetDirstateTuple(" << *relativePath << ") returning "
               << _DirstateNonnormalFileStatus_VALUES_TO_NAMES.at(
                      out.get_status())
               << " "
               << _DirstateMergeState_VALUES_TO_NAMES.at(out.get_mergeState());
  } catch (const std::out_of_range& e) {
    XLOG(DBG2) << "hgGetDirstateTuple(" << *relativePath << ") returns None";
    NoValueForKeyError error;
    error.set_key(*relativePath);
    throw error;
  }
}

void EdenServiceHandler::hgSetDirstateTuple(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePath,
    std::unique_ptr<DirstateTuple> tuple) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  XLOG(INFO) << "hgSetDirstateTuple(" << *relativePath << ") to "
             << _DirstateNonnormalFileStatus_VALUES_TO_NAMES.at(
                    tuple->get_status())
             << " "
             << _DirstateMergeState_VALUES_TO_NAMES.at(tuple->get_mergeState());

  auto filename = RelativePathPiece{*relativePath};
  dirstate->hgSetDirstateTuple(filename, tuple.get());
}

bool EdenServiceHandler::hgDeleteDirstateTuple(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePath) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  XLOG(DBG2) << "hgDeleteDirstateTuple(" << *relativePath << ")";
  auto filename = RelativePathPiece{*relativePath};
  return dirstate->hgDeleteDirstateTuple(filename);
};

void EdenServiceHandler::hgGetNonnormalFiles(
    std::vector<HgNonnormalFile>& out,
    std::unique_ptr<std::string> mountPoint) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  for (auto& pair : dirstate->hgGetNonnormalFiles()) {
    HgNonnormalFile nonnormal;
    nonnormal.set_relativePath(pair.first.stringPiece().str());
    nonnormal.tuple = pair.second;
    nonnormal.__isset.tuple = true;
    out.emplace_back(nonnormal);
  }
}

void EdenServiceHandler::hgCopyMapPut(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePathDest,
    std::unique_ptr<std::string> relativePathSource) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();
  XLOG(DBG2) << "hgCopyMapPut(" << *relativePathDest << ","
             << *relativePathSource << ")";

  dirstate->hgCopyMapPut(
      RelativePathPiece{*relativePathDest},
      RelativePathPiece{*relativePathSource});
}

void EdenServiceHandler::hgCopyMapGet(
    std::string& relativePathSource,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> relativePathDest) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  try {
    auto source = dirstate->hgCopyMapGet(RelativePathPiece{*relativePathDest});
    relativePathSource = source.stringPiece().str();
    XLOG(DBG2) << "hgCopyMapGet(" << *relativePathDest << ") returning "
               << relativePathSource;
  } catch (const std::out_of_range& e) {
    XLOG(DBG2) << "hgCopyMapGet(" << *relativePathDest << ") returns None";
    NoValueForKeyError error;
    error.set_key(*relativePathDest);
    throw error;
  }
}

void EdenServiceHandler::hgCopyMapGetAll(
    std::map<std::string, std::string>& copyMap,
    std::unique_ptr<std::string> mountPoint) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr)
      << "Failed to get dirstate for " << mountPoint.get();

  for (const auto& pair : dirstate->hgCopyMapGetAll()) {
    copyMap.emplace(pair.first.str(), pair.second.stringPiece().str());
  }
}

void EdenServiceHandler::debugGetScmTree(
    vector<ScmTreeEntry>& entries,
    unique_ptr<string> mountPoint,
    unique_ptr<string> idStr,
    bool localStoreOnly) {
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
    std::unique_ptr<std::string> category,
    std::unique_ptr<std::string> level) {
  auto levelValue = folly::stringToLogLevel(*level);
  folly::Logger(*category).getCategory()->setLevel(levelValue);
}

int64_t EdenServiceHandler::unloadInodeForPath(
    unique_ptr<string> mountPoint,
    std::unique_ptr<std::string> path,
    std::unique_ptr<TimeSpec> age) {
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
  server_->stop();
}
}
} // facebook::eden
