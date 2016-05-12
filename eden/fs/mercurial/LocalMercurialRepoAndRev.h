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
#include "LocalMercurialRepo.h"
#include "MercurialFullManifest.h"

namespace facebook {
namespace eden {

class MercurialFullManifest;

// References a repo at a specific revision.
// Since we don't intend to have a full checkout maintained at any revision
// and we may also end up serving multiple users and checkouts, we need
// a way to reference the source of the history as well as a current
// revision.
// In addition to referencing the repo and revision, this provides
// an accessor to the manifest.
// At present we only have access to the full manifest information in
// mercurial, and we materialize this during construction.  The intention
// is that we'll do this all lazily in the future when hg has support
// for querying it in that fashion.
class LocalMercurialRepoAndRev
    : public std::enable_shared_from_this<LocalMercurialRepoAndRev> {
  folly::fbstring rev_;
  std::shared_ptr<LocalMercurialRepo> repo_;
  std::unique_ptr<MercurialFullManifest> manifest_;

 public:
  LocalMercurialRepoAndRev(folly::StringPiece rev,
                           std::shared_ptr<LocalMercurialRepo> repo);
  const folly::fbstring& getRev() const;
  std::shared_ptr<LocalMercurialRepo> getRepo() const;

  MercurialFullManifest& getManifest();
};
}
}
