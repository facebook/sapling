/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/dynamic.h>
#include <optional>
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/config/ParentCommit.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

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
   * Get the parent commit of the working directory.
   */
  ParentCommit getParentCommit() const;

  /**
   * Set the currently checked out commit of the working copy.
   */
  void setCheckedOutCommit(const RootId& commit) const;

  /**
   * Set the working copy parent commit.
   */
  void setWorkingCopyParentCommit(const RootId& commit) const;

  /**
   * Indicate that a checkout operation is in progress.
   *
   * A setCheckedOutCommit call should be made once checkout is complete.
   */
  void setCheckoutInProgress(const RootId& from, const RootId& to) const;

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
  MountProtocol getMountProtocol() const;

  /**
   * Get the raw MountProtocol stored in the config.
   *
   * This should generally not be used except in tests.
   */
  MountProtocol getRawMountProtocol() const {
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
  CaseSensitivity getCaseSensitive() const {
    return caseSensitive_;
  }

  /** Whether this repository should allow non-utf8 path */
  bool getRequireUtf8Path() const {
    return requireUtf8Path_;
  }

  /** Whether this repository is using tree overlay */
  bool getEnableTreeOverlay() const {
    return enableTreeOverlay_;
  }

  /** Whether use FUSE write back cache feature */
  bool getUseWriteBackCache() const {
    return useWriteBackCache_;
  }

  const std::string& getReUseCase() const {
    return reUseCase_;
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
  MountProtocol mountProtocol_{kMountProtocolDefault};
  CaseSensitivity caseSensitive_{kPathMapDefaultCaseSensitive};
  bool requireUtf8Path_{true};

  // Tree Overlay is default on Windows
  bool enableTreeOverlay_{folly::kIsWindows};

  bool useWriteBackCache_{false};

  std::string reUseCase_{"buck2-default"};

#ifdef _WIN32
  Guid repoGuid_;
#endif
};

} // namespace facebook::eden
