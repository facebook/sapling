/*
 *  Copyright (c) 2016-present, Facebook, Inc.
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

#include "eden/fs/model/git/GitIgnoreFileParser.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook {
namespace eden {

folly::Expected<GitIgnore, int> GitIgnoreFileParser::operator()(
    int fileDescriptor,
    AbsolutePathPiece filePath) const {
  GitIgnore gitIgnore;
  try {
    std::string fileContents;
    if (!folly::readFile(fileDescriptor, fileContents)) {
      return folly::makeUnexpected((int)errno);
    }
    if (folly::trimWhitespace(fileContents).size() > 0) {
      gitIgnore.loadFile(fileContents);
    }
  } catch (const std::system_error& ex) {
    int errNum{EIO};
    if (isErrnoError(ex)) {
      errNum = ex.code().value();
    }
    if (errNum != ENOENT) {
      XLOG(WARNING) << "error reading file " << filePath
                    << folly::exceptionStr(ex);
    }
    return folly::makeUnexpected((int)errNum);
  } catch (const std::exception& ex) {
    XLOG(WARNING) << "error reading file " << filePath
                  << folly::exceptionStr(ex);
    return folly::makeUnexpected<int>((int)EIO);
  }
  return gitIgnore;
}
} // namespace eden
} // namespace facebook
