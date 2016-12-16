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

#include <folly/Range.h>

namespace facebook {
namespace eden {

class Hash;

/**
 * Helper function for creating Hash values to use in tests.
 *
 * The input should be an ASCII hex string.  It may be less than 40-bytes long,
 * in which case it will be sign-extended to 40 bytes.
 */
Hash makeTestHash(folly::StringPiece value);
}
}
