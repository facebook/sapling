/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "HgCommand.h"

#include <folly/dynamic.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <thread>

namespace facebook {
namespace hgsparse {

using folly::Future;
using folly::Try;

namespace {
folly::Singleton<HgCommand> cmd_singleton;
}

DEFINE_int32(file_cache_size, 65536, "maximum number of file entries to cache");

static folly::StringPiece dirname(folly::StringPiece name) {
  auto slash = name.rfind('/');
  if (slash != std::string::npos) {
    return name.subpiece(0, slash);
  }
  return "";
}

static folly::StringPiece basename(folly::StringPiece name) {
  auto slash = name.rfind('/');
  if (slash != std::string::npos) {
    name.advance(slash + 1);
    return name;
  }
  return name;
}

// Generic function to insert an item in sorted order
template <typename T, typename COMP, typename CONT>
inline typename CONT::iterator sorted_insert(CONT& vec, T&& val, COMP compare) {
  auto find = std::lower_bound(vec.begin(), vec.end(), val, compare);
  if (find != vec.end() && !compare(val, *find)) {
    // Already exists
    return find;
  }
  return vec.emplace(find, val);
}

struct compare_str {
  inline bool operator()(const std::string& a, const std::string& b) {
    // Bias dotfiles later so that we're more likely to match `ls` access
    // patterns
    int ascore = a[0] == '.' ? 0 : 1;
    int bscore = b[0] == '.' ? 0 : 1;
    if (ascore == bscore) {
      return a < b;
    }
    return ascore > bscore;
  }
};

HgDirInformation& HgTreeInformation::makeDir(folly::StringPiece name) {
  auto key = name.str();
  auto find = dirs_.find(key);
  if (find == dirs_.end()) {
    // Recursively build out parents if missing
    auto parent_dir = dirname(name);
    if (parent_dir != name) {
      auto& parent = makeDir(dirname(name));
      // Add ourselves to the parent
      sorted_insert(parent.dirs, basename(name).str(), compare_str());
    }
  }
  return dirs_[key];
}

void HgTreeInformation::loadManifest() {
  std::thread thr([this] {
    XLOG(INFO) << "Parsing manifest for " << repoDir_ << " @ " << rev_;
    folly::Subprocess proc(
        {"hg", "manifest", "-v", "-r", rev_},
        folly::Subprocess::Options()
            .pipeStdout()
            .chdir(repoDir_)
            .closeOtherFds()
            .usePath());
    auto read_cb = folly::Subprocess::readLinesCallback(
        [this](int fd, folly::StringPiece line) {
          if (fd == STDOUT_FILENO) {
            if (line.empty()) {
              return false;
            }
            line.removeSuffix("\n");

            folly::StringPiece flags("");
            if (line[4] == '@') {
              flags = "l";
            } else if (line[4] == '*') {
              flags = "x";
            }
            auto filename = line.subpiece(6);
            fileInfo_.set(
                filename.str(),
                std::make_shared<HgFileInformation>(
                    flags, 0, basename(filename)));
          } else {
            XLOG(ERR) << "[" << repoDir_ << "] hg files -r " << rev_
                      << " stderr: " << line;
          }
          return false; // Keep reading from the child
        });
    proc.communicate(std::ref(read_cb), [](int /*pfd*/, int /*cfd*/) {
      // Don't write to the child
      return true;
    });
    proc.wait();
    XLOG(INFO) << "manifest loaded";
  });
  thr.detach();
}

void HgTreeInformation::buildTree() {
  XLOG(INFO) << "Parsing file list for " << repoDir_ << " @ " << rev_;
  size_t num_files = 0;

  folly::Subprocess proc(
      {"hg", "files", "-r", rev_},
      folly::Subprocess::Options()
          .pipeStdout()
          .chdir(repoDir_)
          .closeOtherFds()
          .usePath());

  auto read_cb = folly::Subprocess::readLinesCallback(
      [this, &num_files](int fd, folly::StringPiece line) {
        if (fd == STDOUT_FILENO) {
          if (line.empty()) {
            return false;
          }
          line.removeSuffix("\n");
          folly::StringPiece dir = dirname(line);
          folly::StringPiece filename = basename(line);

          // This will create the dir node on demand
          auto& d = makeDir(dir);
          // and add this file to its list
          sorted_insert(d.files, filename.str(), compare_str());
          num_files++;
        } else {
          XLOG(ERR) << "[" << repoDir_ << "] hg files -r " << rev_
                    << " stderr: " << line;
        }
        return false; // Keep reading from the child
      });
  proc.communicate(std::ref(read_cb), [](int /*pfd*/, int /*cfd*/) {
    // Don't write to the child
    return true;
  });
  proc.waitChecked();
  XLOG(INFO) << "build tree with " << dirs_.size() << " dirs";
  fileInfo_.setMaxSize(num_files * 1.2);
  loadManifest();
}

const HgDirInformation& HgTreeInformation::readDir(folly::StringPiece name) {
  return dirs_.at(name.str());
}

HgFileInformation::HgFileInformation(
    folly::StringPiece flags,
    size_t fileSize,
    folly::StringPiece filename)
    : size(fileSize), name(filename.str()) {
  if (flags.find('d') != std::string::npos) {
    mode = S_IFDIR | 0755;
  } else {
    mode = S_IFREG;

    if (flags.find('l') != std::string::npos) {
      mode = S_IFLNK;
    }

    if (flags.find('x') != std::string::npos) {
      mode |= 0755;
    } else {
      mode |= 0644;
    }
  }
}

folly::Future<std::shared_ptr<HgFileInformation>>
HgTreeInformation::rawStatFile(const std::string& filename) {
  auto find_dir = dirs_.find(filename);
  if (find_dir != dirs_.end()) {
    auto dir = basename(filename);
    return std::make_shared<HgFileInformation>("d", 0, dir);
  }

  folly::Promise<std::shared_ptr<HgFileInformation>> promise;
  auto future = promise.getFuture();

  std::thread thr([this, promise = std::move(promise), filename]() mutable {
    promise.setWith([this, filename = std::move(filename)] {
      std::vector<std::string> args = {"hg",
                                       "files",
                                       "-r",
                                       rev_,
                                       "-vT",
                                       "{size}\\0{flags}\\0{abspath}\\n",
                                       filename};
      folly::Subprocess proc(
          args,
          folly::Subprocess::Options()
              .pipeStdout()
              .chdir(repoDir_)
              .closeOtherFds()
              .usePath());
      auto output = proc.communicate();
      proc.waitChecked();

      folly::StringPiece line(output.first);

      line.removeSuffix("\n");
      folly::fbvector<folly::StringPiece> fields;
      folly::split('\0', line, fields);

      if (fields.size() != 3) {
        throw std::runtime_error(
            folly::to<std::string>("bad output from hg files: ", line));
      }

      auto fullname = fields[2];

      return std::make_shared<HgFileInformation>(
          fields[1], folly::to<size_t>(fields[0]), basename(fullname));
    });
  });
  thr.detach();
  return future;
}

folly::Future<std::vector<std::shared_ptr<HgFileInformation>>>
HgTreeInformation::statFiles(const std::vector<std::string>& files) {
  std::vector<folly::Future<std::shared_ptr<HgFileInformation>>> futures;

  return folly::collectAllSemiFuture(
             folly::window(
                 files,
                 [this](std::string name) { return fileInfo_.get(name); },
                 sysconf(_SC_NPROCESSORS_ONLN) / 2))
      .toUnsafeFuture()
      .thenTry([](Try<std::vector<Try<std::shared_ptr<HgFileInformation>>>>&&
                      items) {
        std::vector<std::shared_ptr<HgFileInformation>> res;
        for (auto& item : items.value()) {
          res.push_back(item.value());
        }
        return res;
      });
}

folly::Future<std::vector<std::shared_ptr<HgFileInformation>>>
HgTreeInformation::statDir(folly::StringPiece name) {
  std::vector<std::string> names;
  auto stat = dirs_.at(name.str());

  for (auto& file_name : stat.dirs) {
    names.emplace_back(
        name.empty() ? file_name
                     : folly::to<std::string>(name, "/", file_name));
  }
  for (auto& file_name : stat.files) {
    names.emplace_back(
        name.empty() ? file_name
                     : folly::to<std::string>(name, "/", file_name));
  }

  return statFiles(names);
}

HgTreeInformation::HgTreeInformation(
    const std::string& repoDir,
    const std::string& rev)
    : repoDir_(repoDir),
      rev_(rev),
      fileInfo_(FLAGS_file_cache_size, [=](const std::string name) {
        return rawStatFile(name);
      }) {
  buildTree();
}

folly::Future<std::string> HgCommand::future_run(folly::Subprocess&& proc) {
  struct proc_state {
    folly::Subprocess proc;
    folly::Promise<std::string> promise;

    proc_state(folly::Subprocess&& p) : proc(std::move(p)) {}
  };

  auto state = std::make_shared<proc_state>(std::move(proc));
  std::thread thr([state] {
    state->promise.setWith([state] {
      auto res = state->proc.communicate();
      state->proc.waitChecked();
      // Return stdout
      return res.first;
    });
  });
  thr.detach();
  return state->promise.getFuture();
}

void HgCommand::setRepoDir(const std::string& repoDir) {
  repoDir_ = repoDir;
}
void HgCommand::setRepoRev(const std::string& rev) {
  rev_ = rev;
}
const std::string& HgCommand::getRepoRev() {
  return rev_;
}

std::string HgCommand::run(const std::vector<std::string>& args) {
  folly::Subprocess proc(
      args,
      folly::Subprocess::Options()
          .pipeStdout()
          .pipeStderr()
          .closeOtherFds()
          .usePath());
  auto output = proc.communicate();
  auto res = proc.returnCode();
  if (!res.exited() || res.exitStatus() != 0) {
    XLOG(ERR) << res.str() << ": " << output.second;
    proc.pollChecked();
  }
  return output.first;
}

std::shared_ptr<HgTreeInformation> HgCommand::getTree(const std::string& rev) {
  {
    std::lock_guard<std::mutex> g(lock_);
    auto find = treeInfo_.find(rev);
    if (find != treeInfo_.end()) {
      return find->second;
    }
  }
  auto t = std::make_shared<HgTreeInformation>(repoDir_, rev);
  {
    std::lock_guard<std::mutex> g(lock_);
    treeInfo_.set(rev, t);
  }
  return t;
}

HgCommand::HgCommand() : treeInfo_(16) {}

std::string HgCommand::identifyRev() {
  folly::Subprocess proc(
      {"hg", "log", "-r", ".", "-T", "{node}"},
      folly::Subprocess::Options().pipeStdout().closeOtherFds().usePath().chdir(
          repoDir_));
  auto output = proc.communicate();
  proc.waitChecked();
  folly::StringPiece hash(output.first);
  hash.removeSuffix('+');
  return hash.str();
}

} // namespace hgsparse
} // namespace facebook
