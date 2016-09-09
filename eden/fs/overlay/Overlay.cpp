/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Overlay.h"
#include <dirent.h>
#include <folly/Exception.h>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using folly::File;
using folly::StringPiece;
using folly::fbstring;
using folly::fbvector;

/* Relative to the localDir, the metaFile holds the serialized rendition
 * of the overlay_ data.  We use thrift CompactSerialization for this.
 */
constexpr StringPiece kMetaFile{"overlay"};

/* Relative to the localDir, the overlay tree is where we create the
 * materialized directory structure; directories and files are created
 * here. */
constexpr StringPiece kOverlayTree{"tree"};

Overlay::Overlay(AbsolutePathPiece localDir)
    : localDir_(localDir),
      metaFile_(localDir + PathComponentPiece(kMetaFile)),
      contentDir_(localDir + PathComponentPiece(kOverlayTree)) {
  auto res = mkdir(contentDir_.c_str(), 0700);
  if (res == -1 && errno != EEXIST) {
    folly::throwSystemError("mkdir: ", contentDir_);
  }
}

const AbsolutePath& Overlay::getLocalDir() const {
  return localDir_;
}

const AbsolutePath& Overlay::getContentDir() const {
  return contentDir_;
}
}
}
