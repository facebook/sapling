/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/dynamic.h>
#include <optional>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/utils/PathFuncs.h"

#ifdef _WIN32
#include "eden/fs/utils/Guid.h"
#endif

namespace facebook {
namespace eden {

enum class MountProtocol {
  FUSE,
  PRJFS,
  NFS,
};

/**
 * CheckoutConfig contains the configuration state for a single Eden checkout.
 *
 * This data is stored on disk in the file
 * EDEN_DIR/clients/CHECKOUT_NAME/config.toml
 */
class CheckoutConfig {
 public:
  /**
   * Manually construct a CheckoutConfig object.
   *
   * Note that most callers will probably want to use the
   * loadFromClientDirectory() factory function to create a CheckoutConfig
   * object from an existing client directory, rather than directly calling this
   * constructor.
   */
  CheckoutConfig(
      AbsolutePathPiece mountPath,
      AbsolutePathPiece clientDirectory);

  /**
   * Load a CheckoutConfig object from the edenrc file in a client directory.
   *
   * @param mountPath  The path where the client is (or will be) mounted.
   * @param clientDirectory  The eden client data directory, where the client
   *     configuration file can be found (along with its overlay and other
   *     data).
   */
  static std::unique_ptr<CheckoutConfig> loadFromClientDirectory(
      AbsolutePathPiece mountPath,
      AbsolutePathPiece clientDirectory);

  static folly::dynamic loadClientDirectoryMap(AbsolutePathPiece edenDir);

  /**
   * Get the parent commit(s) of the working directory.
   */
  ParentCommits getParentCommits() const;

  /**
   * Set the parent commit(s) of the working directory.
   */
  void setParentCommits(const ParentCommits& parents) const;
  void setParentCommits(
      Hash parent1,
      std::optional<Hash> parent2 = std::nullopt) const;

  const AbsolutePath& getMountPath() const {
    return mountPath_;
  }

  /** @return Path to the directory where overlay information is stored. */
  AbsolutePath getOverlayPath() const;

  /**
   * Get the repository type.
   *
   * Currently supported types include "git" and "hg".
   */
  const std::string& getRepoType() const {
    return repoType_;
  }

  /**
   * Get the channel type that this mount should be using.
   */
  MountProtocol getMountProtocol() const {
    return mountProtocol_;
  }

  /**
   * Get the repository source.
   *
   * The meaning and format of repository source string depends on the
   * repository type.  For git and hg repositories, this is the path to the
   * git or mercuial repository.
   */
  const std::string& getRepoSource() const {
    return repoSource_;
  }

  /** Path to the file where the current commit ID is stored */
  AbsolutePath getSnapshotPath() const;

  /** Path to the client directory */
  const AbsolutePath& getClientDirectory() const;

  /** Whether this repository is mounted in case-sensitive mode */
  bool getCaseSensitive() const;

  /** Whether this repository should allow non-utf8 path */
  bool getRequireUtf8Path() const {
    return requireUtf8Path_;
  }

  /** Whether this repository is using tree overlay */
  bool getEnableTreeOverlay() const {
    return enableTreeOverlay_;
  }

#ifdef _WIN32
  /** Guid for that repository */
  Guid getRepoGuid() const {
    return repoGuid_;
  }
#endif

 private:
  const AbsolutePath clientDirectory_;
  const AbsolutePath mountPath_;
  std::string repoType_;
  std::string repoSource_;
  MountProtocol mountProtocol_;
  bool caseSensitive_{!folly::kIsWindows};
  bool requireUtf8Path_{true};
  bool enableTreeOverlay_{false};
#ifdef _WIN32
  Guid repoGuid_;
#endif
};
} // namespace eden
} // namespace facebook
