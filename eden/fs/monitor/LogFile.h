/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/File.h>
#include <folly/io/async/EventHandler.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class LogFile {
 public:
  explicit LogFile(const AbsolutePath& path);

  /**
   * Write data to the log file.
   *
   * If the full buffer was successfully written 0 is returned.
   * Returns an errno value on failure.
   */
  int write(const void* buffer, size_t size);

  int fd() const {
    return log_.fd();
  }

 private:
  static constexpr size_t kBufferSize = 64 * 1024;

  folly::File log_;
};

} // namespace eden
} // namespace facebook
