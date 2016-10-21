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

#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class IObjectStore;
class Tree;
class TreeEntry;

/**
 * Given a Tree and a RelativePathPiece, returns the corresponding Tree in the
 * ObjectStore, if it exists. Note the `path` is relative to the specified
 * `tree`.
 */
std::unique_ptr<Tree> getTreeForDirectory(
    RelativePathPiece path,
    const Tree* tree,
    const IObjectStore* objectStore);

/**
 * Given a Tree and a RelativePathPiece, returns the corresponding TreeEntry in
 * the ObjectStore, if it exists.
 */
std::unique_ptr<TreeEntry> getEntryForFile(
    RelativePathPiece file,
    const Tree* baseCommit,
    const IObjectStore* objectStore);
}
}
