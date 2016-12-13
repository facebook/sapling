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

class InodeBase;
class TreeInode;
class FileInode;

/*
 * Pointer aliases.
 * Eventually we will likely need to replace these with a custom time that
 * gives us finer-grained control over reference counting and object
 * destruction.
 */
using InodePtr = std::shared_ptr<InodeBase>;
using TreeInodePtr = std::shared_ptr<TreeInode>;
using FileInodePtr = std::shared_ptr<FileInode>;
}
}
