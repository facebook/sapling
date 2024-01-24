/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "BackingStoreType.h"
#include "eden/fs/utils/Throw.h"

namespace facebook::eden {

BackingStoreType toBackingStoreType(std::string_view type) {
  if (type == "git") {
    return BackingStoreType::GIT;
  } else if (type == "hg") {
    return BackingStoreType::HG;
  } else if (type == "filteredhg") {
    return BackingStoreType::FILTEREDHG;
  } else if (type == "recas") {
    return BackingStoreType::RECAS;
  } else if (type == "http") {
    return BackingStoreType::HTTP;
  } else if (type.empty()) {
    return BackingStoreType::EMPTY;
  } else {
    throwf<std::domain_error>("unsupported backing store type: ", type);
  }
}

std::string_view toBackingStoreString(BackingStoreType type) {
  if (type == BackingStoreType::GIT) {
    return "git";
  } else if (type == BackingStoreType::HG) {
    return "hg";
  } else if (type == BackingStoreType::FILTEREDHG) {
    return "filteredhg";
  } else if (type == BackingStoreType::RECAS) {
    return "recas";
  } else if (type == BackingStoreType::HTTP) {
    return "http";
  } else if (type == BackingStoreType::EMPTY) {
    return "";
  } else {
    throwf<std::domain_error>("unsupported backing store type: ", type);
  }
}

} // namespace facebook::eden
