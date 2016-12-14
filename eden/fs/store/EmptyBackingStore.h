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

#include "BackingStore.h"

namespace facebook {
namespace eden {

/*
 * A dummy BackingStore implementation, that always throws std::domain_error
 * for any ID that is looked up.
 */
class EmptyBackingStore : public BackingStore {
 public:
  EmptyBackingStore();
  virtual ~EmptyBackingStore();

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
};
}
} // facebook::eden
