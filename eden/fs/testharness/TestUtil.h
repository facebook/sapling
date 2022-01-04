/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

namespace facebook {
namespace eden {

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
} // namespace eden
} // namespace facebook
