/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/test/ScopedEnvVar.h"

#include <folly/Exception.h>
#include <stdlib.h>

namespace facebook {
namespace eden {
ScopedEnvVar::ScopedEnvVar(folly::StringPiece name) : name_(name.str()) {
  auto orig = getenv(name_->c_str());
  if (orig) {
    origValue_ = orig;
  }
}

ScopedEnvVar::~ScopedEnvVar() {
  if (!name_) {
    return;
  }
  if (origValue_) {
    setenv(name_->c_str(), origValue_->c_str(), 1);
  } else {
    unsetenv(name_->c_str());
  }
}

void ScopedEnvVar::unset() {
  auto rc = unsetenv(name_->c_str());
  folly::checkUnixError(
      rc, "failed to clear environment variable ", name_.value());
}

void ScopedEnvVar::set(const char* value) {
  // If the caller provides a const char* it is already nul-terminated.
  auto rc = setenv(name_->c_str(), value, 1);
  folly::checkUnixError(
      rc, "failed to set environment variable ", name_.value());
}

void ScopedEnvVar::set(const std::string& value) {
  // If the caller passes in a string we need to call c_str() to make sure it
  // is nul-terminated.  If the string happens to have an internal nul
  // setenv() will only read up to the first nul byte.
  auto rc = setenv(name_->c_str(), value.c_str(), 1);
  folly::checkUnixError(
      rc, "failed to set environment variable ", name_.value());
}

void ScopedEnvVar::set(folly::StringPiece value) {
  // If the data is in an arbitrary StringPiece we have to copy it into a
  // nul-terminated string first.  If the StringPiece happens to have an
  // internal nul setenv() will only read up to the first nul byte.
  auto rc = setenv(name_->c_str(), value.str().c_str(), 1);
  folly::checkUnixError(
      rc, "failed to set environment variable ", name_.value());
}
} // namespace eden
} // namespace facebook
