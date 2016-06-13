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
 * A dummy BackingStore implementation, that always returns null.
 */
class NullBackingStore : public BackingStore {
 public:
  NullBackingStore();
  virtual ~NullBackingStore();

  std::unique_ptr<Tree> getTree(const Hash& id) override;
  std::unique_ptr<Blob> getBlob(const Hash& id) override;
};
}
} // facebook::eden
