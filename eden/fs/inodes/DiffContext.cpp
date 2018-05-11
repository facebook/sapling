/*
 *  Copyright (c) 2004-present, Facebook, Inc.
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

#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook {
namespace eden {

constexpr folly::StringPiece DiffContext::kSystemWideIgnoreFileName;

DiffContext::DiffContext(
    InodeDiffCallback* cb,
    bool listIgnored,
    const ObjectStore* os,
    const UserInfo& userInfo)
    : callback{cb}, store{os}, listIgnored{listIgnored} {
  initOwnedIgnores(
      tryIngestFile(AbsolutePathPiece{kSystemWideIgnoreFileName}),
      tryIngestFile(constructUserIgnoreFileName(userInfo)));
}

DiffContext::DiffContext(
    InodeDiffCallback* cb,
    bool listIgnored,
    const ObjectStore* os,
    folly::StringPiece systemWideIgnoreFileContents,
    folly::StringPiece userIgnoreFileContents)
    : callback{cb}, store{os}, listIgnored{listIgnored} {
  // Load the system-wide ignore settings and user-specific
  // ignore settings into rootIgnore_.
  initOwnedIgnores(systemWideIgnoreFileContents, userIgnoreFileContents);
}

AbsolutePath DiffContext::constructUserIgnoreFileName(
    const UserInfo& userInfo) {
  return userInfo.getHomeDirectory() + ".gitignore"_pc;
}

std::string DiffContext::tryIngestFile(AbsolutePathPiece fileName) {
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

void DiffContext::initOwnedIgnores(
    folly::StringPiece systemWideIgnoreFileContents,
    folly::StringPiece userIgnoreFileContents) {
  pushFrameIfAvailable(systemWideIgnoreFileContents);
  pushFrameIfAvailable(userIgnoreFileContents);
}

void DiffContext::pushFrameIfAvailable(folly::StringPiece ignoreFileContents) {
  if (folly::trimWhitespace(ignoreFileContents).size() > 0) {
    ownedIgnores_.push_back(std::make_unique<GitIgnoreStack>(
        getToplevelIgnore(), ignoreFileContents));
  }
}

} // namespace eden
} // namespace facebook
