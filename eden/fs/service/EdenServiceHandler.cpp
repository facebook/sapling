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

#include <folly/FileUtil.h>
#include <folly/String.h>
#include "EdenServer.h"
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/utils/PathFuncs.h"

using std::string;

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mountImpl(const MountInfo& info) {
  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePathPiece{info.edenClientPath});
  auto snapshotID = config->getSnapshotID();

  auto mountPoint =
      std::make_shared<fusell::MountPoint>(AbsolutePathPiece{info.mountPoint});

  auto overlayPath = config->getOverlayPath();
  auto overlay = std::make_shared<Overlay>(overlayPath);
  auto objectStore = server_->getLocalStore();
  auto edenMount =
      std::make_shared<EdenMount>(mountPoint, objectStore, overlay);

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  auto rootInode = std::make_shared<TreeInode>(
      edenMount.get(),
      objectStore->getTree(snapshotID),
      FUSE_ROOT_ID,
      FUSE_ROOT_ID);
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
}
} // facebook::eden
