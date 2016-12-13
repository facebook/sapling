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
#include <vector>
#include "eden/utils/PathFuncs.h"

/**
 * Utility functions for use with various members of EdenMount.
 */
namespace facebook {
namespace eden {

class EdenMount;

/**
 * @param toIgnore elements of the set are relative to the root of the mount.
 * @return vector with the RelativePath of every directory that is modified
 *     according to the overlay in the mount, but scoped to directoryInMount.
 *     The vector will be ordered as a depth-first traversal of the overlay.
 */
std::vector<RelativePath> getModifiedDirectories(
    const EdenMount* mount,
    RelativePathPiece directoryInMount,
    const std::unordered_set<RelativePathPiece>* toIgnore);

/**
 * @return vector with the RelativePath of every directory that is modified
 *     according to the overlay in the mount. The vector will be ordered as a
 *     depth-first traversal of the overlay.
 */
std::vector<RelativePath> getModifiedDirectoriesForMount(
    const EdenMount* mount,
    const std::unordered_set<RelativePathPiece>* toIgnore);
}
}
