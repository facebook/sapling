/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "LocalMercurialRepoAndRev.h"
#include "MercurialFullManifest.h"

namespace facebook {
namespace eden {

LocalMercurialRepoAndRev::LocalMercurialRepoAndRev(
    folly::StringPiece rev, std::shared_ptr<LocalMercurialRepo> repo)
    : rev_(rev.fbstr()),
      repo_(repo),
      manifest_(MercurialFullManifest::parseManifest(*this)) {}

const folly::fbstring& LocalMercurialRepoAndRev::getRev() const { return rev_; }

std::shared_ptr<LocalMercurialRepo> LocalMercurialRepoAndRev::getRepo() const {
  return repo_;
}

MercurialFullManifest& LocalMercurialRepoAndRev::getManifest() {
  return *manifest_.get();
}
}
}
