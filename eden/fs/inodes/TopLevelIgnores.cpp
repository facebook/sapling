/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook {
namespace eden {

constexpr folly::StringPiece TopLevelIgnores::kSystemWideIgnoreFileName;

TopLevelIgnores::TopLevelIgnores(const UserInfo& userInfo)
    : systemIgnoreStack_{nullptr,
                         tryIngestFile(
                             AbsolutePathPiece{kSystemWideIgnoreFileName})},
      userIgnoreStack_{&systemIgnoreStack_,
                       tryIngestFile(constructUserIgnoreFileName(userInfo))} {}

AbsolutePath TopLevelIgnores::constructUserIgnoreFileName(
    const UserInfo& userInfo) {
  return userInfo.getHomeDirectory() + ".gitignore"_pc;
}

std::string TopLevelIgnores::tryIngestFile(AbsolutePathPiece fileName) {
  std::string contents;
  try {
    auto in =
        folly::File(fileName.stringPiece()); // throws if file does not exist
    if (!folly::readFile(in.fd(), contents)) {
      contents.clear();
    }
  } catch (const std::system_error& ex) {
    if (!isEnoent(ex)) {
      XLOG(WARNING) << "error reading gitignore file " << fileName
                    << folly::exceptionStr(ex);
    }
  } catch (const std::exception& ex) {
    XLOG(WARNING) << "error reading gitignore file " << fileName
                  << folly::exceptionStr(ex);
  }
  return contents;
}

} // namespace eden
} // namespace facebook
