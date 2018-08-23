/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <sysexits.h>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/RocksDbLocalStore.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(blobID, "", "The blob ID");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2");
constexpr folly::StringPiece kRocksDBPath{"storage/rocks-db"};

using namespace facebook::eden;
using folly::IOBuf;

/*
 * This tool rewrites a specific blob in Eden's local store with empty contents.
 * This is intended for use in integration tests that exercise the behavior
 * with bogus blob contents in the LocalStore.
 */
int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);
  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }

  Hash blobID(FLAGS_blobID);

  auto edenDir = facebook::eden::canonicalPath(FLAGS_edenDir);
  const auto rocksPath = edenDir + RelativePathPiece{kRocksDBPath};
  RocksDbLocalStore localStore(rocksPath);

  Blob blob(blobID, IOBuf());
  localStore.putBlob(blobID, &blob);

  return EX_OK;
}
