/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInodeDirHandle.h"

#include "Overlay.h"
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/DirType.h"

namespace facebook {
namespace eden {

TreeInodeDirHandle::TreeInodeDirHandle(TreeInodePtr inode) : inode_(inode) {}

} // namespace eden
} // namespace facebook
