/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/io/IOBuf.h>

namespace facebook::eden {

class ObjectId;
class Tree;

/**
 * Creates an Eden Tree from the serialized version of a Git tree object.
 * As such, the SHA-1 of the gitTreeObject should match the hash.
 */
std::unique_ptr<Tree> deserializeGitTree(
    const ObjectId& hash,
    const folly::IOBuf* treeData);
std::unique_ptr<Tree> deserializeGitTree(
    const ObjectId& hash,
    folly::ByteRange treeData);

} // namespace facebook::eden
