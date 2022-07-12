/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/concurrency/UnboundedQueue.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
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
 * to record this are correctly stored in a folly::UnboundedQueue in the right
 * order. Waits until a timeout of 1000ms to dequeue the next event off the
 * queue and checks thats its progress (START vs END) and inode number are as
 * given.
 */
bool isInodeMaterializedInQueue(
    folly::UnboundedQueue<InodeTraceEvent, true, true, false>&
        materializationQueue,
    InodeEventProgress progress,
    InodeNumber ino);
} // namespace facebook::eden
