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

#include "eden/fs/model/Hash.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

struct BindMount {
  AbsolutePath pathInClientDir;
  AbsolutePath pathInMountDir;

  bool operator==(const BindMount& other) const {
    return pathInClientDir == other.pathInClientDir &&
        pathInMountDir == other.pathInMountDir;
  }
};

inline void operator<<(std::ostream& out, const BindMount& bindMount) {
  out << "BindMount{pathInClientDir=" << bindMount.pathInClientDir
      << "; pathInMountDir=" << bindMount.pathInMountDir << "}";
}

class ClientConfig {
 public:
  static std::unique_ptr<ClientConfig> loadFromClientDirectory(
      AbsolutePathPiece mountPoint,
      AbsolutePathPiece clientDirectory,
      AbsolutePathPiece configPath);

  Hash getSnapshotID() const;

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
}
}
