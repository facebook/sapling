/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/utils/PathFuncs.h"

struct git_oid;
struct git_repository;

namespace facebook {
namespace eden {

class Hash;
class LocalStore;

/**
 * A BackingStore implementation that loads data out of a git repository.
 */
class GitBackingStore : public BackingStore {
 public:
  /**
   * Create a new GitBackingStore.
   *
   * The LocalStore object is owned by the EdenServer (which also owns this
   * GitBackingStore object).  It is guaranteed to be valid for the lifetime of
   * the GitBackingStore object.
   */
  GitBackingStore(AbsolutePathPiece repository, LocalStore* localStore);
  ~GitBackingStore() override;

  /**
   * Get the repository path.
   *
   * This returns the path to the .git directory itself.
   */
  const char* getPath() const;

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::SemiFuture<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;

 private:
  GitBackingStore(GitBackingStore const&) = delete;
  GitBackingStore& operator=(GitBackingStore const&) = delete;

  std::unique_ptr<Tree> getTreeImpl(const Hash& id);
  std::unique_ptr<Blob> getBlobImpl(const Hash& id);

  static git_oid hash2Oid(const Hash& hash);
  static Hash oid2Hash(const git_oid* oid);

  LocalStore* localStore_{nullptr};
  git_repository* repo_{nullptr};
};
} // namespace eden
} // namespace facebook
