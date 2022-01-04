/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/Executor.h>
#include <folly/futures/Future.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class ObjectId;
class Hash20;
class LocalStore;
class MetadataImporter;
class ReloadableConfig;
class Tree;
class TreeMetadata;

using MetadataImporterFactory = std::function<std::unique_ptr<MetadataImporter>(
    std::shared_ptr<ReloadableConfig> config,
    std::string repoName,
    std::shared_ptr<LocalStore> localStore)>;

class MetadataImporter {
 public:
  virtual ~MetadataImporter() = default;

  /**
   * Get the metadata for the entries in a tree for the tree specified by the
   * edenId
   */
  virtual folly::SemiFuture<std::unique_ptr<TreeMetadata>> getTreeMetadata(
      const ObjectId& edenId,
      const Hash20& manifestId) = 0;

  /**
   * Returns if metadata fetching is supported on the current platform and
   * is configured, if not the DefaultMetadataImporter should be used.
   */
  virtual bool metadataFetchingAvailable() = 0;

  template <typename T>
  static MetadataImporterFactory getMetadataImporterFactory() {
    return [](std::shared_ptr<ReloadableConfig> config,
              std::string repoName,
              std::shared_ptr<LocalStore> localStore) {
      return std::make_unique<T>(
          std::move(config), std::move(repoName), localStore);
    };
  }
};

/**
 * Metdata importer where all the fetching and storing operations are no-ops.
 * To be used when scs metadata fetching is not supported.
 */
class DefaultMetadataImporter : public MetadataImporter {
 public:
  DefaultMetadataImporter(
      std::shared_ptr<ReloadableConfig> /*config*/,
      std::string /*repoName*/,
      std::shared_ptr<LocalStore> /*localStore*/) {}

  folly::SemiFuture<std::unique_ptr<TreeMetadata>> getTreeMetadata(
      const ObjectId& edenId,
      const Hash20& manifestId) override;

  bool metadataFetchingAvailable() override;
};

} // namespace facebook::eden
