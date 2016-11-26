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

class ObjectStore;
class Tree;

namespace fusell {
class MountPoint;
}

std::unique_ptr<Tree> getRootTreeForMountPoint(
    fusell::MountPoint* mountPoint,
    ObjectStore* objectStore);

/**
 * @return vector with the RelativePath of every directory that is modified
 *     according to the overlay in the mount. The vector will be ordered as a
 *     depth-first traversal of the overlay.
 */
std::unique_ptr<std::vector<RelativePath>> getModifiedDirectoriesForMount(
    fusell::MountPoint* mountPoint);
}
}
