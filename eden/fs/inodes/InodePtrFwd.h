/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
} // namespace eden
} // namespace facebook
