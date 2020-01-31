/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/LogFile.h"

#include <fcntl.h>

#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

LogFile::LogFile(const AbsolutePath& path)
    : log_(path.c_str(), O_CREAT | O_WRONLY | O_APPEND, 0644) {}

int LogFile::write(const void* buffer, size_t size) {
  // TODO: Rotate the log file if necessary
  auto bytesWritten = folly::writeFull(log_.fd(), buffer, size);
  if (bytesWritten == -1) {
    return errno;
  }
  return 0;
}

} // namespace eden
} // namespace facebook
