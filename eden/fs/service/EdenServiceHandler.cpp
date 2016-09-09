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
#include "EdenServer.h"
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/TreeEntryFileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/utils/PathFuncs.h"

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

  auto overlayPath = config->getOverlayPath();
  auto overlay = std::make_shared<Overlay>(overlayPath);
  auto backingStore =
      server_->getBackingStore(config->getRepoType(), config->getRepoSource());
  auto objectStore =
      make_unique<ObjectStore>(server_->getLocalStore(), backingStore);
  auto rootTree = objectStore->getTreeForCommit(snapshotID);
  auto edenMount =
      std::make_shared<EdenMount>(mountPoint, std::move(objectStore), overlay);

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  auto rootInode = std::make_shared<TreeInode>(
      edenMount.get(), std::move(rootTree), FUSE_ROOT_ID, FUSE_ROOT_ID);
  mountPoint->setRootInode(rootInode);

  // TODO(mbolin): Use the result of config.getBindMounts() to perform the
  // appropriate bind mounts for the client.
  server_->mount(std::move(edenMount), std::move(config));
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  try {
    mountImpl(*info);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw EdenError(folly::exceptionStr(ex).toStdString());
  }
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  try {
    server_->unmount(*mountPoint);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw EdenError(folly::exceptionStr(ex).toStdString());
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
    EdenError err(e.what());
    err.set_errorCode(e.code().value());
    SHA1Result out;
    out.set_error(err);
    return out;
  }
}

SHA1Result EdenServiceHandler::getSHA1ForPath(
    const string& mountPoint,
    const string& path) {
  SHA1Result out;

  if (path.empty()) {
    out.set_error(EdenError("path cannot be the empty string"));
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
        out.set_error(EdenError(folly::to<string>(
            "Wrong FileInode type: ", currentPiece.stringPiece())));
        return out;
      }

      auto entry = fileInode->getEntry();
      if (!S_ISREG(entry->mode)) {
        out.set_error(EdenError(folly::to<string>(
            "Not an ordinary file: ", currentPiece.stringPiece())));
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

void EdenServiceHandler::shutdown() {
  server_->stop();
}
}
} // facebook::eden
