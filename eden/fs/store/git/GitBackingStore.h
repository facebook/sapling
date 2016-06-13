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

#include "eden/fs/store/BackingStore.h"

#include <folly/Range.h>

namespace facebook {
namespace eden {

class LocalStore;

/**
 * A BackingStore implementation that loads data out of a git repository.
 */
class GitBackingStore : public BackingStore {
 public:
  /**
   * Create a new GitBackingStore.
   *
   * The LocalStore object is owned by the EdenServer (which also owns this
   * GitBackingStore object).  It is guaranteed to be valid for the lifetime of
   * the GitBackingStore object.
   */
  GitBackingStore(folly::StringPiece repository, LocalStore* localStore);
  virtual ~GitBackingStore();

  std::unique_ptr<Tree> getTree(const Hash& id) override;
  std::unique_ptr<Blob> getBlob(const Hash& id) override;
  std::unique_ptr<Tree> getTreeForCommit(const Hash& commitID) override;

 private:
  GitBackingStore(GitBackingStore const&) = delete;
  GitBackingStore& operator=(GitBackingStore const&) = delete;

  LocalStore* localStore_{nullptr};
};
}
}
