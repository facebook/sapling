/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/utils/FileDescriptor.h"

namespace facebook::eden {

struct Pipe {
  FileDescriptor read;
  FileDescriptor write;

  explicit Pipe(bool nonBlocking = false);
};

struct SocketPair {
  FileDescriptor read;
  FileDescriptor write;

  explicit SocketPair(bool nonBlocking = false);
};

} // namespace facebook::eden
