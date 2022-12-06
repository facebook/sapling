/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/testharness/HgRepo.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Portability.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include <sys/stat.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/FileUtils.h"

using folly::StringPiece;
using std::string;
using std::vector;

namespace facebook::eden {

HgRepo::HgRepo(AbsolutePathPiece path, AbsolutePath hgCmd)
    : hgCmd_(hgCmd), path_(path) {
  XLOG(DBG1) << "Using hg command: " << hgCmd_;

  // Set up hgEnv_
  std::vector<const char*> passthroughVars{
      {"HG_REAL_BIN", "HGEXECUTABLEPATH", "LLVM_PROFILE_FILE", "PATH"}};
  hgEnv_.clear();
  for (const char* varName : passthroughVars) {
    auto value = getenv(varName);
    if (value) {
      hgEnv_.set(varName, value);
    }
  }

  hgEnv_.set("HGPLAIN", "1");
  hgEnv_.set("HGRCPATH", "");
  hgEnv_.set("CHGDISABLE", "1");
  hgEnv_.set("NOSCMLOG", "1");
  hgEnv_.set("LOCALE", "en_US.UTF-8");
  hgEnv_.set("LC_ALL", "en_US.UTF-8");
  // Trick Mercurial into thinking it's in a test so it doesn't generate
  // prod configs.
  hgEnv_.set("TESTTMP", (path.dirname() + "cache"_pc).value());
}

HgRepo::HgRepo(AbsolutePathPiece path)
    : HgRepo(path, findAndConfigureHgBinary()) {}

string HgRepo::hg(vector<string> args) {
  auto process = invokeHg(std::move(args));
  const auto outputs{process.communicate()};
  process.waitChecked();
  return outputs.first;
}

SpawnedProcess HgRepo::invokeHg(vector<string> args) {
  SpawnedProcess::Options opts;
  opts.chdir(path_);
  opts.pipeStdout();
  return invokeHg(std::move(args), std::move(opts));
}

SpawnedProcess HgRepo::invokeHg(
    vector<string> args,
    SpawnedProcess::Options&& options) {
  args.insert(args.begin(), {"hg", "--traceback"});

  XLOG(DBG1) << "repo " << path_ << " running: " << folly::join(" ", args);
  options.environment() = hgEnv_;
  options.executablePath(hgCmd_);
  return SpawnedProcess(args, std::move(options));
}

void HgRepo::hgInit(
    AbsolutePathPiece cacheDirectory,
    std::vector<std::string> extraArgs) {
  XLOG(DBG1) << "creating new hg repository at " << path_;

  // Invoke SpawnedProcess directly here rather than using our hg() helper
  // function.  The hg() function requires the repository directory to already
  // exist.
  std::vector<std::string> args = {"hg", "init", path_.value()};
  args.insert(args.end(), extraArgs.begin(), extraArgs.end());
  SpawnedProcess::Options opts;
  opts.environment() = hgEnv_;
  opts.executablePath(hgCmd_);
  SpawnedProcess p(args, std::move(opts));
  p.waitChecked();

  appendToRequires("remotefilelog\n");

  appendToHgrc(fmt::format(
      "[extensions]\n"
      "remotefilelog =\n"
      "remotenames =\n"
      "treemanifest =\n"
      "[treemanifest]\n"
      "treeonly = true\n"
      "[remotefilelog]\n"
      "server = false\n"
      "reponame = test\n"
      "cachepath = {}\n"
      "[scmstore]\n"
      "backingstore = true\n",
      cacheDirectory));
}

void HgRepo::cloneFrom(
    StringPiece serverRepoUrl,
    std::vector<std::string> extraArgs) {
  XLOG(DBG1) << "cloning new hg repository at " << path_ << " from "
             << serverRepoUrl;

  std::vector<std::string> args = {"hg", "clone"};
  args.insert(args.end(), extraArgs.begin(), extraArgs.end());
  args.push_back(serverRepoUrl.str());
  args.push_back(path_.value());
  XLOG(DBG1) << "running: " << folly::join(" ", args);

  SpawnedProcess::Options opts;
  opts.executablePath(hgCmd_);
  opts.environment() = hgEnv_;
  SpawnedProcess p(args, std::move(opts));
  p.waitChecked();
}

void HgRepo::appendToHgrc(folly::StringPiece data) {
  auto hgrcPath = path_ + ".hg"_pc + "hgrc"_pc;
  folly::File hgrc{hgrcPath.view(), O_WRONLY | O_APPEND | O_CREAT};
  if (folly::writeFull(hgrc.fd(), data.data(), data.size()) < 0) {
    folly::throwSystemError("error writing to ", hgrcPath.view());
  }
}

void HgRepo::appendToHgrc(const std::vector<std::string>& lines) {
  appendToHgrc(folly::join("\n", lines) + "\n");
}

void HgRepo::appendToRequires(folly::StringPiece data) {
  auto hgrcPath = path_ + ".hg"_pc + "requires"_pc;
  folly::File hgrc{hgrcPath.view(), O_WRONLY | O_APPEND | O_CREAT};
  if (folly::writeFull(hgrc.fd(), data.data(), data.size()) < 0) {
    folly::throwSystemError("error writing to ", hgrcPath.view());
  }
}

RootId HgRepo::commit(StringPiece message) {
  hg("commit",
     "-u",
     "Test User <user@example.com>",
     "-d",
     "2017-01-01 13:00:00",
     "-m",
     message.str());
  auto output = hg("log", "-r.", "-T{node}\\n");
  return RootId{Hash20{folly::rtrimWhitespace(output)}.toString()};
}

Hash20 HgRepo::getManifestForCommit(const RootId& commit) {
  auto output = hg("log", "-r", commit.value(), "-T{manifest}\\n");
  return Hash20{folly::rtrimWhitespace(output)};
}

void HgRepo::mkdir(RelativePathPiece path, mode_t permissions) {
  auto fullPath = path_ + path;
  auto rc = ::mkdir(fullPath.value().c_str(), permissions);
  folly::checkUnixError(rc, "mkdir ", fullPath.view());
}

void HgRepo::writeFile(
    RelativePathPiece path,
    StringPiece contents,
    mode_t permissions) {
  // TODO(xavierd): remove permissions from the callers.
  (void)permissions;
  auto fullPath = path_ + path;
  writeFileAtomic(fullPath, contents).value();
}

void HgRepo::symlink(StringPiece contents, RelativePathPiece path) {
  auto fullPath = path_ + path;
  auto rc = ::symlink(contents.str().c_str(), fullPath.value().c_str());
  folly::checkUnixError(rc, "error creating symlink at ", path.view());
}

bool testEnvironmentSupportsHg() {
  // In opt builds, we don't want to optimize away references to HgImporter.cpp,
  // since it defines the hgPath gflag. Mask the kIsSanitizeThread constant from
  // the optimizer.
  static volatile bool kIsSanitizeThread = folly::kIsSanitizeThread;
  return !kIsSanitizeThread;
}

} // namespace facebook::eden

#endif
