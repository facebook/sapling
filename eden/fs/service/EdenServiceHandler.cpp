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

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  auto rootInode = std::make_shared<TreeInode>(
      objectStore->getTree(snapshotID),
      mountPoint.get(),
      FUSE_ROOT_ID,
      FUSE_ROOT_ID,
      objectStore,
      overlay);
  mountPoint->setRootInode(rootInode);

  // TODO(mbolin): Use the result of config.getBindMounts() to perform the
  // appropriate bind mounts for the client.
  server_->mount(std::move(mountPoint));
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

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  for (const auto& mountPoint : server_->getMountPoints()) {
    MountInfo info;
    info.mountPoint = mountPoint->getPath().stringPiece().str();
    // TODO: Fill in info.edenClientPath.
    // I'll add that in a future diff, once we have a custom MountPoint
    // subclass that isn't in the low-level fusell namespace.
    results.push_back(info);
  }
}

void EdenServiceHandler::checkOutRevision(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash) {
  AbsolutePathPiece mountPointForClient(*mountPoint);

  auto mount = server_->getMountPoint(*mountPoint);
  if (!mount) {
    throw EdenError("requested mount point is not known to this eden instance");
  }

  auto dispatcher = mount->getDispatcher();
  auto root = std::dynamic_pointer_cast<TreeInode>(
      dispatcher->getDirInode(FUSE_ROOT_ID));
  CHECK_NOTNULL(root.get());

  root->performCheckout(*hash, dispatcher, mount);
}
}
} // facebook::eden
