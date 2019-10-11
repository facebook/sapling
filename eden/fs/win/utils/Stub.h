/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <iostream>
#include <string>
#include "eden/fs/utils/PathFuncs.h"

// This is a stub to compile eden/service on Window.
struct fuse_init_out {
  uint32_t major;
  uint32_t minor;
};

struct fuse_in_header {
  uint32_t len;
};

namespace facebook {
namespace eden {

struct SerializedInodeMap {
  int stub;
};

struct SerializedFileHandleMap {
  int stub;
};

class PrivHelper {
 public:
  int stub;
  int stub1;
};

using uid_t = int;
using gid_t = int;

struct InodePtr {
  int stub;
};

struct TreeInodePtr {
  int stub;
};

class TakeoverData {
 public:
  int stub;
  struct MountInfo {
    int stub;
  };
};

struct FuseChannelData {
  //  folly::File fd;
  int fd;
  fuse_init_out connInfo;
};

class ProcessNameCache {
  int stub;
};

static int unlink(const char* path) {
  // Ideally unlink should be a part of folly portability layer but there is a
  // deprecated definition of unlink in stdio which will make it ambiguous and
  // break the build for other softwares using folly on Windows.
  return _unlink(path);
}

#define NOT_IMPLEMENTED()                                                    \
  do {                                                                       \
    std::cout << " +++++  NOT IMPLEMETED +++++++ Function: " << __FUNCTION__ \
              << " Line:" << __LINE__;                                       \
    throw;                                                                   \
  } while (true)

} // namespace eden
} // namespace facebook
