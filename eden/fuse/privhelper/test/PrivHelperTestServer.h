/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fuse/privhelper/PrivHelperServer.h"

#include <folly/Range.h>

namespace facebook {
namespace eden {
namespace fusell {

/*
 * A subclass of PrivHelperServer that doesn't actually perform
 * real mounts and unmounts.  This lets us use it in unit tests
 * when we are running without root privileges.
 */
class PrivHelperTestServer : public PrivHelperServer {
 public:
  explicit PrivHelperTestServer(folly::StringPiece tmpDir);

  /*
   * Get the path to the test file representing the given mount point.
   */
  std::string getMountPath(folly::StringPiece mountPath) const;

  /*
   * Check if the given mount point is mounted.
   *
   * This can be called from any process.  (It is generally called from the
   * main process during unit tests, and not from the privhelper process.)
   */
  bool isMounted(folly::StringPiece mountPath) const;

 private:
  folly::File fuseMount(const char* mountPath) override;
  void fuseUnmount(const char* mountPath) override;

  std::string tmpDir_;
};
}
}
}
