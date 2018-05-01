/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/testharness/HgRepo.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/Subprocess.h>
#include <folly/logging/xlog.h>
#include <sys/stat.h>
#include <unistd.h>

#include "eden/fs/model/Hash.h"

using folly::StringPiece;
using folly::Subprocess;
using std::string;
using std::vector;

namespace facebook {
namespace eden {
HgRepo::HgRepo(AbsolutePathPiece path) : path_{path} {
  // Find the hg command and store it in hgCmd_
  auto pathPtr = getenv("PATH");
  if (!pathPtr) {
    throw std::runtime_error("unable to find hg command: no PATH");
  }
  StringPiece pathEnv{pathPtr};
  vector<string> pathEnvParts;
  folly::split(":", pathEnv, pathEnvParts);

  bool found = false;
  for (const auto& dir : pathEnvParts) {
    for (const auto& name : {"hg.real", "hg"}) {
      auto exePath = folly::to<string>(dir, "/", name);
      XLOG(DBG5) << "Checking for hg at " << exePath;
      if (access(exePath.c_str(), X_OK) == 0) {
        hgCmd_ = realpath(exePath);
        found = true;
        break;
      }
    }
    if (found) {
      break;
    }
  }
  if (!found) {
    throw std::runtime_error("unable to find hg in PATH");
  }
  XLOG(DBG1) << "Using hg command: " << hgCmd_;

  // Set up hgEnv_
  hgEnv_.push_back(folly::to<string>("PATH=", path));
  hgEnv_.push_back("HGPLAIN=1");
  hgEnv_.push_back("CHGDISABLE=1");
  hgEnv_.push_back("LOCALE=C");
}

string HgRepo::hg(vector<string> args) {
  args.insert(args.begin(), "hg");

  XLOG(DBG1) << "repo " << path_ << " running: " << folly::join(" ", args);
  const auto options{Subprocess::Options().chdir(path_.value()).pipeStdout()};
  Subprocess p(args, options, hgCmd_.value().c_str(), &hgEnv_);
  const auto outputs{p.communicate()};
  p.waitChecked();
  return outputs.first;
}

void HgRepo::hgInit() {
  XLOG(DBG1) << "creating new hg repository at " << path_;

  // Invoke Subprocess directly here rather than using our hg() helper
  // function.  The hg() function requires the repository directory to already
  // exist.
  Subprocess p(
      {"hg", "init", path_.value()},
      Subprocess::Options(),
      hgCmd_.value().c_str(),
      &hgEnv_);
  p.waitChecked();
}

void HgRepo::appendToHgrc(folly::StringPiece data) {
  auto hgrcPath =
      path_ + PathComponentPiece{".hg"} + PathComponentPiece{"hgrc"};
  folly::File hgrc{hgrcPath.stringPiece(), O_WRONLY | O_APPEND | O_CREAT};
  if (folly::writeFull(hgrc.fd(), data.data(), data.size()) < 0) {
    folly::throwSystemError("error writing to ", hgrcPath);
  }
}

void HgRepo::appendToHgrc(const std::vector<std::string>& lines) {
  appendToHgrc(folly::join("\n", lines));
}

Hash HgRepo::commit(StringPiece message) {
  hg("commit",
     "-u",
     "Test User <user@example.com>",
     "-d",
     "2017-01-01 13:00:00",
     "-m",
     message.str());
  auto output = hg("log", "-r.", "-T{node}\\n");
  return Hash{folly::rtrimWhitespace(output)};
}

void HgRepo::mkdir(RelativePathPiece path, mode_t permissions) {
  auto fullPath = path_ + path;
  auto rc = ::mkdir(fullPath.value().c_str(), permissions);
  folly::checkUnixError(rc, "mkdir ", fullPath);
}

void HgRepo::writeFile(
    RelativePathPiece path,
    StringPiece contents,
    mode_t permissions) {
  auto fullPath = path_ + path;
  folly::writeFileAtomic(fullPath.value(), contents, permissions);
}

void HgRepo::symlink(StringPiece contents, RelativePathPiece path) {
  auto fullPath = path_ + path;
  auto rc = ::symlink(contents.str().c_str(), fullPath.value().c_str());
  checkUnixError(rc, "error creating symlink at ", path);
}
} // namespace eden
} // namespace facebook
