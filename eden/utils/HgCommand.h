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
#include <string>
#include <vector>
#include <mutex>
#include "LeaseCache.h"
#include <folly/futures/Future.h>
#include <folly/Subprocess.h>
#include <folly/Singleton.h>

namespace facebook {
namespace hgsparse {

struct HgFileInformation {
  size_t size;
  std::string name;
  mode_t mode;

  HgFileInformation(folly::StringPiece flags,
                    size_t fileSize,
                    folly::StringPiece filename);
};

struct HgDirInformation {
  folly::fbvector<folly::fbstring> files;
  folly::fbvector<folly::fbstring> dirs;
};

class HgTreeInformation
    : public std::enable_shared_from_this<HgTreeInformation> {
  std::string repoDir_;
  std::string rev_;
  std::unordered_map<std::string, HgDirInformation> dirs_;
  eden::LeaseCache<std::string, HgFileInformation> fileInfo_;

  void buildTree();
  void loadManifest();
  HgDirInformation& makeDir(folly::StringPiece name);

  folly::Future<std::shared_ptr<HgFileInformation>> rawStatFile(
      const std::string& filename);

 public:
  // Constructs the tree information and parses the initial manifest data
  HgTreeInformation(const std::string& repoDir, const std::string& rev);

  // Get the stat information for the files in the specified dir
  folly::Future<std::vector<std::shared_ptr<HgFileInformation>>> statDir(
      folly::StringPiece name);

  // Given a list of files relative to the root, stat each of them
  folly::Future<std::vector<std::shared_ptr<HgFileInformation>>> statFiles(
      const std::vector<std::string>& files);

  // Get the list of files and dirs contained in the specified dir
  const HgDirInformation& readDir(folly::StringPiece name);
};

class HgCommand {
  folly::EvictingCacheMap<std::string, std::shared_ptr<HgTreeInformation>>
      treeInfo_;
  std::mutex lock_;
  std::string repoDir_;
  std::string rev_;

 public:
  HgCommand();

  void setRepoDir(const std::string& repoDir);
  void setRepoRev(const std::string& rev);
  const std::string &getRepoRev();

  // Executes a command, returning stdout.
  // If the command failed, throws an exception with the error
  // code and the stderr text
  static std::string run(const std::vector<std::string>& args);

  std::string identifyRev();

  std::shared_ptr<HgTreeInformation> getTree(const std::string& rev);

  // Wait for a subprocess to complete.  Yields the stdout or
  // an exception if there was an error
  static folly::Future<std::string> future_run(folly::Subprocess&& proc);
};

}
}
