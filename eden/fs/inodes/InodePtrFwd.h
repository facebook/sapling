/*
 *  Copyright (c) 2017, Facebook, Inc.
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

template <typename InodeType>
class InodePtrImpl;
template <typename InodeType>
class InodeBasePtrImpl;

/*
 * Friendly names for the various InodePtr classes.
 */
using FileInodePtr = InodePtrImpl<FileInode>;
using TreeInodePtr = InodePtrImpl<TreeInode>;
using InodePtr = InodeBasePtrImpl<InodeBase>;
using ConstFileInodePtr = InodePtrImpl<const FileInode>;
using ConstTreeInodePtr = InodePtrImpl<const TreeInode>;
using ConstInodePtr = InodeBasePtrImpl<const InodeBase>;
}
}
