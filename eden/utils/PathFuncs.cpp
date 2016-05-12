/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PathFuncs.h"

namespace facebook {
namespace eden {

folly::StringPiece dirname(folly::StringPiece path) {
  auto slash = path.rfind('/');
  if (slash != std::string::npos) {
    return path.subpiece(0, slash);
  }
  return "";
}

folly::StringPiece basename(folly::StringPiece path) {
  auto slash = path.rfind('/');
  if (slash != std::string::npos) {
    path.advance(slash + 1);
    return path;
  }
  return path;
}
}
}
