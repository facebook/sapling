/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

/*
 * This file contains forward declarations of InodePtr and related types
 */

namespace facebook {
namespace eden {

class FileInode;
class InodeBase;
class TreeInode;
class DotEdenInode;

template <typename InodeType>
class InodePtrImpl;

/*
 * Friendly names for the various InodePtr classes.
 */
using DotEdenInodePtr = InodePtrImpl<DotEdenInode>;
using FileInodePtr = InodePtrImpl<FileInode>;
using TreeInodePtr = InodePtrImpl<TreeInode>;
class InodePtr;
}
}
