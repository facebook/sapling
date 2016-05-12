/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PrivHelperTestServer.h"

#include <folly/File.h>
#include <folly/FileUtil.h>
#include <system_error>

using folly::File;
using folly::StringPiece;

namespace facebook {
namespace eden {
namespace fusell {

PrivHelperTestServer::PrivHelperTestServer(StringPiece tmpDir)
    : tmpDir_(tmpDir.str()) {}

File PrivHelperTestServer::fuseMount(const char* mountPath) {
  // Just open a new file inside our temporary directory,
  // and write "mounted" into it.
  File f(getMountPath(mountPath).c_str(), O_RDWR | O_CREAT | O_TRUNC);
  StringPiece data{"mounted"};
  folly::writeFull(f.fd(), data.data(), data.size());
  return f;
}

void PrivHelperTestServer::fuseUnmount(const char* mountPath) {
  // Replace the file contents with "unmounted"
  File f(getMountPath(mountPath).c_str(), O_RDWR | O_CREAT | O_TRUNC);
  StringPiece data{"unmounted"};
  folly::writeFull(f.fd(), data.data(), data.size());
}

std::string PrivHelperTestServer::getMountPath(StringPiece mountPath) const {
  return tmpDir_ + "/" + mountPath.str();
}

bool PrivHelperTestServer::isMounted(folly::StringPiece mountPath) const {
  try {
    std::string data;
    folly::readFile(getMountPath(mountPath).c_str(), data, 256);
    return data == "mounted";
  } catch (const std::system_error& ex) {
    if (ex.code().category() == std::system_category() &&
        ex.code().value() == ENOENT) {
      // Looks like this was never mounted
      return false;
    }
    throw;
  }
}
}
}
}
