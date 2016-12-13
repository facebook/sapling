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

class IObjectStore {
 public:
  virtual ~IObjectStore() {}

  virtual std::unique_ptr<Tree> getTree(const Hash& id) const = 0;
  virtual std::unique_ptr<Blob> getBlob(const Hash& id) const = 0;
  virtual std::unique_ptr<Tree> getTreeForCommit(
      const Hash& commitID) const = 0;

  /**
   * Return the SHA1 hash of the blob contents.
   *
   * (Note that this is different than the Hash identifying the blob.  The
   * hash identifying the blob may be computed using a separate mechanism, and
   * may not be the same as the SHA1-hash of its contents.)
   */
  virtual Hash getSha1ForBlob(const Hash& id) const = 0;
};
}
}
