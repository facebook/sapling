/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include "eden/fs/model/TreeFwd.h"

namespace facebook::eden {

class ObjectId;

/**
 * Creates an Eden Tree from the serialized version of a Git tree object.
 * As such, the SHA-1 of the gitTreeObject should match the id.
 */
TreePtr deserializeGitTree(const ObjectId& id, const folly::IOBuf* treeData);
TreePtr deserializeGitTree(const ObjectId& id, folly::ByteRange treeData);

} // namespace facebook::eden
