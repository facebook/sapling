/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
