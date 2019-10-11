/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "PrivHelperTestServer.h"

#include <boost/filesystem.hpp>
#include <folly/Conv.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <system_error>
#include "eden/fs/utils/SystemError.h"

using folly::File;
using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

PrivHelperTestServer::PrivHelperTestServer() {}

void PrivHelperTestServer::init(folly::File&& socket, uid_t uid, gid_t gid) {
  // folly::init() has already been called before the unit tests start,
  // so just call initPartial() rather than init(), to avoid calling
  // folly::init() twce.
  initPartial(std::move(socket), uid, gid);
}

// FUSE mounts.

File PrivHelperTestServer::fuseMount(const char* mountPath) {
  // Create a single file named "mounted" and write "mounted" into it.
  auto pathToNewFile = getPathToMountMarker(mountPath);
  File f(pathToNewFile, O_RDWR | O_CREAT | O_TRUNC);
  StringPiece data{"mounted"};
  folly::writeFull(f.fd(), data.data(), data.size());
  return f;
}

void PrivHelperTestServer::fuseUnmount(const char* mountPath) {
  // Replace the file contents with "unmounted".
  folly::writeFile(
      StringPiece{"unmounted"}, getPathToMountMarker(mountPath).c_str());

  // Implicitly unmount all bind mounts
  auto mountPrefix = folly::to<std::string>(mountPath, "/");
  for (auto& path : allBindMounts_) {
    if (folly::StringPiece(path).startsWith(mountPrefix)) {
      folly::writeFile(StringPiece{"bind-unmounted"}, path.c_str());
    }
  }
}

bool PrivHelperTestServer::isMounted(folly::StringPiece mountPath) const {
  return checkIfMarkerFileHasContents(
      getPathToMountMarker(mountPath), "mounted");
}

string PrivHelperTestServer::getPathToMountMarker(StringPiece mountPath) const {
  return mountPath.str() + "/mounted";
}

// Bind mounts.

void PrivHelperTestServer::bindMount(
    const char* /*clientPath*/,
    const char* mountPath) {
  // Create a single file named "bind-mounted" and write "bind-mounted" into it.

  // Normally, the caller to the PrivHelper (in practice, EdenServer) is
  // responsible for creating the directory before requesting the bind mount.
  boost::filesystem::create_directories(mountPath);

  auto fileInMountPath = getPathToBindMountMarker(mountPath);
  folly::writeFile(StringPiece{"bind-mounted"}, fileInMountPath.c_str());
  allBindMounts_.push_back(fileInMountPath);
}

void PrivHelperTestServer::bindUnmount(const char* mountPath) {
  // Replace the file contents with "bind-unmounted".
  folly::writeFile(
      StringPiece{"bind-unmounted"},
      getPathToBindMountMarker(mountPath).c_str());
}

bool PrivHelperTestServer::isBindMounted(folly::StringPiece mountPath) const {
  return checkIfMarkerFileHasContents(
      getPathToBindMountMarker(mountPath), "bind-mounted");
}

string PrivHelperTestServer::getPathToBindMountMarker(
    StringPiece mountPath) const {
  return mountPath.str() + "/bind-mounted";
}

// General helpers.

bool PrivHelperTestServer::checkIfMarkerFileHasContents(
    const string pathToMarkerFile,
    const string contents) const {
  try {
    string data;
    folly::readFile(pathToMarkerFile.c_str(), data, 256);
    return data == contents;
  } catch (const std::system_error& ex) {
    if (isEnoent(ex)) {
      // Looks like this was never mounted
      return false;
    }
    throw;
  }
}

} // namespace eden
} // namespace facebook
