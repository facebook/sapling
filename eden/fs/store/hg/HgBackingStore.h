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
#include "eden/fs/store/hg/HgImporter.h"

#include <folly/Range.h>
#include <folly/Synchronized.h>

namespace facebook {
namespace eden {

class LocalStore;

/**
 * A BackingStore implementation that loads data out of a mercurial repository.
 */
class HgBackingStore : public BackingStore {
 public:
  /**
   * Create a new HgBackingStore.
   *
   * The LocalStore object is owned by the EdenServer (which also owns this
   * HgBackingStore object).  It is guaranteed to be valid for the lifetime of
   * the HgBackingStore object.
   */
  HgBackingStore(folly::StringPiece repository, LocalStore* localStore);
  virtual ~HgBackingStore();

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;

 private:
  // Forbidden copy constructor and assignment operator
  HgBackingStore(HgBackingStore const&) = delete;
  HgBackingStore& operator=(HgBackingStore const&) = delete;

  std::unique_ptr<Tree> getTreeForCommitImpl(const Hash& commitID);

  // TODO: In the future we may want to maintain a pool of HgImporter objects,
  // rather than just a single one, so we can perform multiple imports in
  // parallel.
  folly::Synchronized<HgImporter> importer_;
  LocalStore* localStore_{nullptr};
};
}
} // facebook::eden
