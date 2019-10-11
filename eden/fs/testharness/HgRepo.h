/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Subprocess.h>
#include <string>
#include <vector>

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class Subprocess;
}

namespace facebook {
namespace eden {

class Hash;

/**
 * A helper class for working with a mercurial repository in unit tests.
 */
class HgRepo {
 public:
  explicit HgRepo(AbsolutePathPiece path);

  const AbsolutePath& path() const {
    return path_;
  }

  /**
   * Run an hg command.
   *
   * The parameters are the arguments to pass to hg.  This should not
   * include the "hg" program name itself (argument 0).
   *
   * e.g., hg("add") will run "hg add" in the repository.
   * Arguments can be strings, RelativePaths, or AbsolutePaths.
   *
   * Returns the data that the command printed on stdout.
   * Throws if the command exited with a non-zero status.
   */
  template <typename... Args>
  std::string hg(const Args&... args) {
    std::vector<std::string> argsVector;
    buildHgArgs(argsVector, args...);
    return hg(std::move(argsVector));
  }

  /**
   * Run an hg command.
   *
   * @param args The arguments to pass to "hg" (not including argument 0, "hg"
   *     itself).
   *
   * Returns the data that the command printed on stdout.
   * Throws if the command exited with a non-zero status.
   */
  std::string hg(std::vector<std::string> args);

  /**
   * Start an hg command and return the folly::Subprocess object without waiting
   * for it to complete.
   */
  template <typename... Args>
  folly::Subprocess invokeHg(const Args&... args) {
    std::vector<std::string> argsVector;
    buildHgArgs(argsVector, args...);
    return invokeHg(std::move(argsVector));
  }
  folly::Subprocess invokeHg(std::vector<std::string> args);
  folly::Subprocess invokeHg(
      std::vector<std::string> args,
      const folly::Subprocess::Options& options);

  /**
   * Call "hg init" to create the repository.
   */
  void hgInit(std::vector<std::string> extraArgs = {});

  /**
   * Configure the repository's hgrc to enable treemanifest.
   */
  void enableTreeManifest(AbsolutePathPiece cacheDirectory);

  /**
   * Call "hg clone" to create the repository.
   */
  void cloneFrom(
      folly::StringPiece serverRepoUrl,
      std::vector<std::string> extraArgs = {});

  /**
   * Append data to the repository's hgrc file
   */
  void appendToHgrc(folly::StringPiece data);
  void appendToHgrc(const std::vector<std::string>& lines);

  Hash commit(folly::StringPiece message);

  void mkdir(RelativePathPiece path, mode_t permissions = 0755);
  void mkdir(folly::StringPiece path, mode_t permissions = 0755) {
    mkdir(RelativePathPiece{path}, permissions);
  }

  void writeFile(
      RelativePathPiece path,
      folly::StringPiece contents,
      mode_t permissions = 0644);
  void writeFile(
      folly::StringPiece path,
      folly::StringPiece contents,
      mode_t permissions = 0644) {
    writeFile(RelativePathPiece{path}, contents, permissions);
  }

  void symlink(folly::StringPiece contents, RelativePathPiece path);

 private:
  void buildHgArgs(std::vector<std::string>& /* cmd */) {}
  template <typename... Args>
  void buildHgArgs(
      std::vector<std::string>& cmd,
      folly::StringPiece str,
      const Args&... args) {
    cmd.push_back(str.str());
    buildHgArgs(cmd, args...);
  }
  template <typename... Args>
  void buildHgArgs(
      std::vector<std::string>& cmd,
      RelativePathPiece path,
      const Args&... args) {
    cmd.push_back(path.value().str());
    buildHgArgs(cmd, args...);
  }
  template <typename... Args>
  void buildHgArgs(
      std::vector<std::string>& cmd,
      AbsolutePathPiece path,
      const Args&... args) {
    cmd.push_back(path.value().str());
    buildHgArgs(cmd, args...);
  }

  AbsolutePath hgCmd_;
  std::vector<std::string> hgEnv_;
  AbsolutePath path_;
};
} // namespace eden
} // namespace facebook
