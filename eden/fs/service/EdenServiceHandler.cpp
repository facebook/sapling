/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenServiceHandler.h"

#include <boost/polymorphic_cast.hpp>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/Subprocess.h>
#include <unordered_set>
#include "EdenError.h"
#include "EdenServer.h"
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/EdenMountHandler.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

using std::shared_ptr;
using std::string;
using std::unique_ptr;
using folly::make_unique;
using folly::StringPiece;

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  try {
    mountImpl(*info);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::mountImpl(const MountInfo& info) {
  server_->reloadConfig();
  auto initialConfig = ClientConfig::loadFromClientDirectory(
      AbsolutePathPiece{info.mountPoint},
      AbsolutePathPiece{info.edenClientPath},
      server_->getConfig().get());

  auto repoType = initialConfig->getRepoType();
  auto backingStore =
      server_->getBackingStore(repoType, initialConfig->getRepoSource());
  auto objectStore =
      make_unique<ObjectStore>(server_->getLocalStore(), backingStore);
  auto edenMount = std::make_shared<EdenMount>(
      std::move(initialConfig), std::move(objectStore));
  // We gave ownership of initialConfig to the EdenMount.
  // Get a pointer to it that we can use for the remainder of this function.
  auto* config = edenMount->getConfig();

  // TODO(mbolin): Use the result of config.getBindMounts() to perform the
  // appropriate bind mounts for the client.
  server_->mount(edenMount);

  auto cloneSuccessPath = config->getCloneSuccessPath();
  bool isInitialMount = access(cloneSuccessPath.c_str(), F_OK) != 0;
  if (isInitialMount) {
    auto repoHooks = config->getRepoHooks();
    auto postCloneScript = repoHooks + RelativePathPiece("post-clone");
    auto repoSource = config->getRepoSource();

    LOG(INFO) << "Running post-clone hook '" << postCloneScript << "' for "
              << info.mountPoint;
    try {
      // TODO(mbolin): It would be preferable to pass the name of the repository
      // as defined in ~/.edenrc so that the script can derive the repoType and
      // repoSource from that. Then the hook would only take two args.
      folly::Subprocess proc(
          {postCloneScript.c_str(), repoType, info.mountPoint, repoSource},
          folly::Subprocess::pipeStdin());
      proc.closeParentFd(STDIN_FILENO);
      proc.waitChecked();
    } catch (const folly::SubprocessSpawnError& ex) {
      // If this failed because postCloneScript does not exist, then ignore the
      // error because we are tolerant of the case where /etc/eden/hooks does
      // not exist, by design.
      if (ex.errnoValue() != ENOENT) {
        // TODO(13448173): If clone fails, then we should roll back the mount.
        throw;
      } else {
        VLOG(1) << "Did not run post-clone hook '" << postCloneScript
                << "' because it was not found.";
      }
    }
    LOG(INFO) << "Finished post-clone hook '" << postCloneScript << "' for "
              << info.mountPoint;
  }

  // The equivalent of `touch` to signal that clone completed successfully.
  folly::writeFile(string(), cloneSuccessPath.c_str());
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
    server_->unmount(*mountPoint);
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

void EdenServiceHandler::checkOutRevision(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash) {
  Hash hashObj(*hash);
  AbsolutePathPiece mountPointForClient(*mountPoint);

  auto edenMount = server_->getMount(*mountPoint);
  if (!edenMount) {
    throw EdenError("requested mount point is not known to this eden instance");
  }

  auto root = edenMount->getRootInode();
  CHECK_NOTNULL(root.get());

  root->performCheckout(hashObj);
}

void EdenServiceHandler::getSHA1(
    std::vector<SHA1Result>& out,
    std::unique_ptr<string> mountPoint,
    std::unique_ptr<std::vector<string>> paths) {
  // TODO(t12747617): Parallelize these requests.
  for (auto& path : *paths.get()) {
    out.push_back(getSHA1ForPathDefensively(*mountPoint.get(), path));
  }
}

SHA1Result EdenServiceHandler::getSHA1ForPathDefensively(
    const string& mountPoint,
    const string& path) {
  // Calls getSHA1ForPath() and traps all system_errors and returns the error
  // variant of the SHA1Result union type rather than letting the exception
  // bubble up.
  try {
    return getSHA1ForPath(mountPoint, path);
  } catch (const std::system_error& e) {
    SHA1Result out;
    out.set_error(newEdenError(e));
    return out;
  }
}

SHA1Result EdenServiceHandler::getSHA1ForPath(
    const string& mountPoint,
    const string& path) {
  SHA1Result out;

  if (path.empty()) {
    out.set_error(newEdenError(EINVAL, "path cannot be the empty string"));
    return out;
  }

  auto edenMount = server_->getMount(mountPoint);
  auto relativePath = RelativePathPiece{path};
  auto inodeDispatcher = edenMount->getDispatcher();
  shared_ptr<fusell::DirInode> parent = edenMount->getRootInode();

  auto it = relativePath.paths().begin();
  while (true) {
    shared_ptr<fusell::InodeBase> inodeBase;
    inodeBase =
        inodeDispatcher
            ->lookupInodeBase(parent->getNodeId(), it.piece().basename())
            .get();

    auto inodeNumber = inodeBase->getNodeId();
    auto currentPiece = it.piece();
    it++;
    if (it == relativePath.paths().end()) {
      // inodeNumber must correspond to the last path component, which we expect
      // to correspond to a file.
      auto fileInode = std::dynamic_pointer_cast<FileInode>(
          inodeDispatcher->getFileInode(inodeNumber));

      if (!fileInode) {
        out.set_error(newEdenError(
            EISDIR, "Wrong FileInode type: {}", currentPiece.stringPiece()));
        return out;
      }

      auto entry = fileInode->getEntry();
      if (!S_ISREG(entry->mode)) {
        out.set_error(newEdenError(
            EISDIR, "Not an ordinary file: {}", currentPiece.stringPiece()));
        return out;
      }

      auto hash = fileInode->getSHA1().get();
      out.set_sha1(StringPiece(hash.getBytes()).str());
      return out;
    } else {
      parent = inodeDispatcher->getDirInode(inodeNumber);
    }
  }
}

void EdenServiceHandler::getMaterializedEntries(
    MaterializedResult& out,
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);
  if (!edenMount) {
    throw newEdenError(ENODEV, "no such mount point \"{}\"", *mountPoint);
  }

  return getMaterializedEntriesForMount(edenMount.get(), out);
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
  out.snapshotHash = StringPiece(latest->toHash.getBytes()).str();
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

  std::unordered_set<RelativePath> changedFiles;

  out.toPosition.sequenceNumber = delta->toSequence;
  out.toPosition.snapshotHash = StringPiece(delta->toHash.getBytes()).str();
  out.toPosition.mountGeneration = edenMount->getMountGeneration();

  out.fromPosition = out.toPosition;

  while (delta) {
    if (delta->toSequence <= fromPosition->sequenceNumber) {
      // We've reached the end of the interesting section
      break;
    }

    changedFiles.insert(
        delta->changedFilesInOverlay.begin(),
        delta->changedFilesInOverlay.end());

    out.fromPosition.sequenceNumber = delta->fromSequence;
    out.fromPosition.snapshotHash =
        StringPiece(delta->fromHash.getBytes()).str();
    out.fromPosition.mountGeneration = edenMount->getMountGeneration();

    delta = delta->previous;
  }

  for (auto& path : changedFiles) {
    out.paths.emplace_back(path.stringPiece().str());
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
      auto inodeBase =
          edenMount->getMountPoint()->getInodeBaseForPath(relativePath);

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

// TODO(mbolin): Use ThriftHgStatusCode exclusively instead of HgStatusCode.
ThriftHgStatusCode thriftStatusCodeForStatusCode(HgStatusCode statusCode) {
  switch (statusCode) {
    case HgStatusCode::CLEAN:
      return ThriftHgStatusCode::CLEAN;
    case HgStatusCode::MODIFIED:
      return ThriftHgStatusCode::MODIFIED;
    case HgStatusCode::ADDED:
      return ThriftHgStatusCode::ADDED;
    case HgStatusCode::REMOVED:
      return ThriftHgStatusCode::REMOVED;
    case HgStatusCode::MISSING:
      return ThriftHgStatusCode::MISSING;
    case HgStatusCode::NOT_TRACKED:
      return ThriftHgStatusCode::NOT_TRACKED;
    case HgStatusCode::IGNORED:
      return ThriftHgStatusCode::IGNORED;
  }
  throw std::runtime_error(
      folly::to<std::string>("Unrecognized value: ", statusCode));
}

void EdenServiceHandler::scmGetStatus(
    ThriftHgStatus& out,
    std::unique_ptr<std::string> mountPoint) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  auto status = dirstate->getStatus();
  auto& entries = out.entries;
  for (auto& pair : *status->list()) {
    auto statusCode = pair.second;
    entries[pair.first.stringPiece().str()] =
        thriftStatusCodeForStatusCode(statusCode);
  }
}

void EdenServiceHandler::scmAdd(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> path) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  dirstate->add(RelativePathPiece{*path});
}

void EdenServiceHandler::scmRemove(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> path,
    bool force) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  dirstate->remove(RelativePathPiece{*path}, force);
}

void EdenServiceHandler::shutdown() {
  server_->stop();
}
}
} // facebook::eden
