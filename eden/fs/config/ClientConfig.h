/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Optional.h>
#include <folly/dynamic.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class BindMount {
 public:
  BindMount(AbsolutePathPiece clientDirPath, AbsolutePathPiece mountDirPath)
      : pathInClientDir(clientDirPath), pathInMountDir(mountDirPath) {}

  bool operator==(const BindMount& other) const {
    return pathInClientDir == other.pathInClientDir &&
        pathInMountDir == other.pathInMountDir;
  }

  AbsolutePath pathInClientDir;
  AbsolutePath pathInMountDir;
};

inline void operator<<(std::ostream& out, const BindMount& bindMount) {
  out << "BindMount{pathInClientDir=" << bindMount.pathInClientDir
      << "; pathInMountDir=" << bindMount.pathInMountDir << "}";
}

class ClientConfig {
 public:
  /**
   * Manually construct a ClientConfig object.
   *
   * Note that most callers will probably want to use the
   * loadFromClientDirectory() factory function to create a ClientConfig object
   * from an existing client directory, rather than directly calling this
   * constructor.
   */
  ClientConfig(AbsolutePathPiece mountPath, AbsolutePathPiece clientDirectory);

  /**
   * Load a ClientConfig object from the edenrc file in a client directory.
   *
   * @param mountPath  The path where the client is (or will be) mounted.
   * @param clientDirectory  The eden client data directory, where the client
   *     configuration file can be found (along with its overlay and other
   *     data).
   */
  static std::unique_ptr<ClientConfig> loadFromClientDirectory(
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
      folly::Optional<Hash> parent2 = folly::none) const;

  const AbsolutePath& getMountPath() const {
    return mountPath_;
  }

  /** @return Path to the directory where overlay information is stored. */
  AbsolutePath getOverlayPath() const;

  const std::vector<BindMount>& getBindMounts() const {
    return bindMounts_;
  }

  /**
   * Get the repository type.
   *
   * Currently supported types include "git" and "hg".
   */
  const std::string& getRepoType() const {
    return repoType_;
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

 private:
  ClientConfig(
      AbsolutePathPiece clientDirectory,
      AbsolutePathPiece mountPath,
      std::vector<BindMount>&& bindMounts);

  AbsolutePath clientDirectory_;
  AbsolutePath mountPath_;
  std::vector<BindMount> bindMounts_;
  std::string repoType_;
  std::string repoSource_;
};
} // namespace eden
} // namespace facebook
