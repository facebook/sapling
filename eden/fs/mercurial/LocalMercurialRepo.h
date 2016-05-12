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
#include <folly/String.h>

namespace facebook {
namespace eden {

// Represents what we know about a mercurial repo on the local disk.
// We may add more information in here in the future.
// The thought is that we'll keep this repo checked out on the null
// rev, so that it is roughly equivalent to a bare checkout.
// See also LocalMercurialRepoAndRev
class LocalMercurialRepo {
  // The path to the directory that contains the .hg dir for the repo
  folly::fbstring localPath_;

 public:
  explicit LocalMercurialRepo(folly::StringPiece path);

  const folly::fbstring& getPath() const;
};
}
}
