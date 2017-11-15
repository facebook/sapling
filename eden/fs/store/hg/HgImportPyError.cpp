/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/hg/HgImportPyError.h"

#include <folly/Conv.h>

using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

constexpr folly::StringPiece HgImportPyError::kSeparator;

HgImportPyError::HgImportPyError(StringPiece errorType, StringPiece message)
    : fullMessage_{folly::to<string>(errorType, kSeparator, message)},
      errorType_{fullMessage_.data(), errorType.size()},
      message_{fullMessage_.data() + errorType.size() + kSeparator.size(),
               fullMessage_.data() + fullMessage_.size()} {}

} // namespace eden
} // namespace facebook
