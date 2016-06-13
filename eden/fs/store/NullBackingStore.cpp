/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "NullBackingStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"

using std::unique_ptr;

namespace facebook {
namespace eden {

NullBackingStore::NullBackingStore() {}

NullBackingStore::~NullBackingStore() {}

unique_ptr<Tree> NullBackingStore::getTree(const Hash& id) {
  return nullptr;
}

unique_ptr<Blob> NullBackingStore::getBlob(const Hash& id) {
  return nullptr;
}
}
} // facebook::eden
