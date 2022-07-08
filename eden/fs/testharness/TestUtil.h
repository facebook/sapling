/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/telemetry/ActivityBuffer.h"

namespace facebook::eden {

class ObjectId;
class Hash20;

/**
 * Helper function for creating Hash values to use in tests.
 *
 * The input should be an ASCII hex string.  It may be less than 40-bytes long,
 * in which case it will be sign-extended to 40 bytes.
 */
ObjectId makeTestHash(folly::StringPiece value);

Hash20 makeTestHash20(folly::StringPiece value);

/**
 * Helper function for ensuring an inode finished materializing and events
 * to record this are correctly stored in a given ActivityBuffer. Ensures that
 * exactly one START materialization event and one END materialization event is
 * present in the ActivityBuffer. Used in FileInode and TreeInode tests.
 */
bool isInodeMaterializedInBuffer(ActivityBuffer& buff, InodeNumber ino);
} // namespace facebook::eden
