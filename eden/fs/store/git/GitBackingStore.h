/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/PathFuncs.h"

struct git_oid;
struct git_repository;

namespace facebook::eden {

class LocalStore;

/**
 * A BackingStore implementation that loads data out of a git repository.
 */
class GitBackingStore final : public BackingStore {
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

  RootId parseRootId(folly::StringPiece rootId) override;
  std::string renderRootId(const RootId& rootId) override;

  folly::SemiFuture<std::unique_ptr<Tree>> getRootTree(
      const RootId& rootId,
      ObjectFetchContext& context) override;
  folly::SemiFuture<std::unique_ptr<TreeEntry>> getTreeEntryForRootId(
      const RootId& /* rootId */,
      TreeEntryType /* treeEntryType */,
      facebook::eden::PathComponentPiece /* pathComponentPiece */,
      ObjectFetchContext& /* context */) override {
    throw std::domain_error("unimplemented");
  }
  folly::SemiFuture<BackingStore::GetTreeRes> getTree(
      const Hash& id,
      ObjectFetchContext& context) override;
  folly::SemiFuture<BackingStore::GetBlobRes> getBlob(
      const Hash& id,
      ObjectFetchContext& context) override;

 private:
  GitBackingStore(GitBackingStore const&) = delete;
  GitBackingStore& operator=(GitBackingStore const&) = delete;

  std::unique_ptr<Tree> getTreeImpl(const Hash& id);
  std::unique_ptr<Blob> getBlobImpl(const Hash& id);

  static git_oid root2Oid(const RootId& rootId);

  static git_oid hash2Oid(const Hash& hash);
  static Hash oid2Hash(const git_oid* oid);

  LocalStore* localStore_{nullptr};
  git_repository* repo_{nullptr};
};

} // namespace facebook::eden
