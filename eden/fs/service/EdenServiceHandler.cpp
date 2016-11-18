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
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeEntryFileInode.h"
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

void EdenServiceHandler::mountImpl(const MountInfo& info) {
  server_->reloadConfig();
  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePathPiece{info.mountPoint},
      AbsolutePathPiece{info.edenClientPath},
      server_->getConfig().get());
  auto snapshotID = config->getSnapshotID();

  auto mountPoint =
      std::make_shared<fusell::MountPoint>(AbsolutePathPiece{info.mountPoint});

  // Read some values before transferring the config.
  auto repoSource = config->getRepoSource();
  auto cloneSuccessPath = config->getCloneSuccessPath();
  auto repoHooks = config->getRepoHooks().copy();
  auto repoType = config->getRepoType();

  auto overlayPath = config->getOverlayPath();
  auto overlay = std::make_shared<Overlay>(overlayPath);
  auto backingStore =
      server_->getBackingStore(repoType, config->getRepoSource());
  auto objectStore =
      make_unique<ObjectStore>(server_->getLocalStore(), backingStore);
  auto rootTree = objectStore->getTreeForCommit(snapshotID);
  auto edenMount = std::make_shared<EdenMount>(
      mountPoint, std::move(objectStore), overlay, std::move(config));

  // Load the overlay, if present.
  auto rootOverlayDir = overlay->loadOverlayDir(RelativePathPiece());

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  if (rootOverlayDir) {
    mountPoint->setRootInode(std::make_shared<TreeInode>(
        edenMount.get(),
        std::move(rootOverlayDir.value()),
        nullptr,
        FUSE_ROOT_ID,
        FUSE_ROOT_ID));
  } else {
    mountPoint->setRootInode(std::make_shared<TreeInode>(
        edenMount.get(),
        std::move(rootTree),
        nullptr,
        FUSE_ROOT_ID,
        FUSE_ROOT_ID));
  }

  // Record the transition from no snapshot to the current snapshot in
  // the journal.  This also sets things up so that we can carry the
  // snapshot id forward through subsequent journal entries.
  auto delta = std::make_unique<JournalDelta>();
  delta->toHash = snapshotID;
  edenMount->getJournal().wlock()->addDelta(std::move(delta));

  // TODO(mbolin): Use the result of config.getBindMounts() to perform the
  // appropriate bind mounts for the client.
  server_->mount(std::move(edenMount));

  bool isInitialMount = access(cloneSuccessPath.c_str(), F_OK) != 0;
  if (isInitialMount) {
    auto postCloneScript = repoHooks + RelativePathPiece("post-clone");

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

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  try {
    mountImpl(*info);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
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

  auto mount = edenMount->getMountPoint();
  auto dispatcher = mount->getDispatcher();
  auto root = std::dynamic_pointer_cast<TreeInode>(
      dispatcher->getDirInode(FUSE_ROOT_ID));
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
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();
  auto parent = inodeDispatcher->getDirInode(FUSE_ROOT_ID);

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
      auto fileInode = std::dynamic_pointer_cast<TreeEntryFileInode>(
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
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();

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
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();

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
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();
  auto rootInode = inodeDispatcher->getInode(FUSE_ROOT_ID);

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

void EdenServiceHandler::shutdown() {
  server_->stop();
}
}
} // facebook::eden
