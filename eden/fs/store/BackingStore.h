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

#include <memory>

namespace facebook {
namespace eden {

class Blob;
class Hash;
class Tree;

/**
 * Abstract interface for a BackingStore.
 *
 * A BackingStore fetches tree and blob information from an external
 * authoritative data source.
 *
 * BackingStore implementations must be thread-safe, and perform their own
 * internal locking.
 */
class BackingStore {
 public:
  BackingStore() {}
  virtual ~BackingStore() {}

  virtual std::unique_ptr<Tree> getTree(const Hash& id) = 0;
  virtual std::unique_ptr<Blob> getBlob(const Hash& id) = 0;

  virtual std::unique_ptr<Tree> getTreeForCommit(const Hash& commitID) = 0;

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;
};
}
} // facebook::eden
