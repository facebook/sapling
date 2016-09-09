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
#include <folly/File.h>
#include <folly/String.h>
#include <map>
#include "eden/utils/DirType.h"
#include "eden/utils/PathFuncs.h"
#include "eden/utils/PathMap.h"

namespace facebook {
namespace eden {

/** Manages the write overlay storage area.
 *
 * The overlay is where we store files that are not yet part of a snapshot.
 *
 * The contents of this storage layer are overlaid on top of the object store
 * snapshot that is active in a given mount point.
 *
 * There is one overlay area associated with each eden client instance.
 *
 * We use the Overlay to manage mutating the structure of the checkout;
 * each time we create or delete a directory entry, we do so through
 * the overlay class.
 *
 * The Overlay class keeps track of the mutated tree; if we mutate some
 * file "foo/bar/baz" then the Overlay records metadata about the list
 * of files in the root, the list of files in "foo", the list of files in
 * "foo/bar" and finally materializes "foo/bar/baz".
 */
class Overlay {
 public:
  explicit Overlay(AbsolutePathPiece localDir);

  /** Returns the path to the root of the Overlay storage area */
  const AbsolutePath& getLocalDir() const;

  /** Returns the path to the root of the materialized tree.
   * This is a sub-directory of the local dir */
  const AbsolutePath& getContentDir() const;

 private:
  /** path to ".eden/CLIENT/local" */
  AbsolutePath localDir_;
  /** location of the serialized metadata file */
  AbsolutePath metaFile_;
  /** location of the materialized files/dirs */
  AbsolutePath contentDir_;
};
}
}
