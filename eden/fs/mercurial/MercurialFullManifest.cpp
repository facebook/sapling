/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MercurialFullManifest.h"
#include "LocalMercurialRepoAndRev.h"
#include "eden/utils/PathFuncs.h"
#include "eden/utils/SortedInsert.h"
#include <folly/Subprocess.h>
#include <wangle/concurrent/GlobalExecutor.h>

DEFINE_int32(hg_manifest_file_cache_size,
             65536,
             "maximum number of file entries to cache");

namespace facebook {
namespace eden {

MercurialFullManifest::MercurialFullManifest(LocalMercurialRepoAndRev& repo)
    : repo_(repo),
      fileInfo_(
          FLAGS_hg_manifest_file_cache_size,
          [=](const folly::fbstring& name) { return fetchFileInfo(name); }) {
  load();
}

MercurialFullManifest::DirListing& MercurialFullManifest::getOrMakeEntry(
    folly::StringPiece name) {
  auto key = name.fbstr();

  auto find = dirs_.find(key);
  if (find == dirs_.end()) {
    // Recursively build out parents if missing
    auto parent_dir = dirname(name);
    if (parent_dir != name) {
      auto& parent = getOrMakeEntry(dirname(name));
      // Add ourselves to the parent
      sorted_insert(parent.dirs, basename(name).str(), CompareString());
    }
  }
  return dirs_[key];
}

std::unique_ptr<MercurialFullManifest> MercurialFullManifest::parseManifest(
    LocalMercurialRepoAndRev& repo) {
  std::unique_ptr<MercurialFullManifest> manifest(
      new MercurialFullManifest(repo));
  manifest->load();
  return manifest;
}

void MercurialFullManifest::load() {
  auto path = repo_.getRepo()->getPath();
  LOG(INFO) << "Parsing file list for " << path << " @ " << repo_.getRev();
  size_t num_files = 0;

  folly::Subprocess proc({"hg", "files", "-r", repo_.getRev().toStdString()},
                         folly::Subprocess::pipeStdout()
                             .chdir(path.toStdString())
                             .closeOtherFds()
                             .usePath());

  auto read_cb = folly::Subprocess::readLinesCallback(
      [this, &num_files, &path](int fd, folly::StringPiece line) {
        if (fd == STDOUT_FILENO) {
          if (line.empty()) {
            return false;
          }
          line.removeSuffix("\n");
          folly::StringPiece dir = dirname(line);
          folly::StringPiece filename = basename(line);

          // This will create the dir node on demand
          auto& entry = getOrMakeEntry(dir);
          // and add this file to its list
          sorted_insert(entry.files, filename.fbstr(), CompareString());
          num_files++;
        } else {
          LOG(ERROR) << "[" << path << "] hg files -r " << repo_.getRev()
                     << " stderr: " << line;
        }
        return false; // Keep reading from the child
      });
  proc.communicate(std::ref(read_cb), [](int, int) {
    // Don't write to the child
    return true;
  });
  proc.waitChecked();
  LOG(INFO) << "build tree with " << dirs_.size() << " dirs";
}

MercurialFullManifest::FileInfo::FileInfo(mode_t mode, size_t size)
    : mode(mode), size(size) {}

static mode_t flags_to_mode(folly::StringPiece flags) {
  if (flags.find('d') != std::string::npos) {
    return S_IFDIR | 0755;
  }

  mode_t mode = S_IFREG;

  if (flags.find('l') != std::string::npos) {
    mode = S_IFLNK;
  }

  if (flags.find('x') != std::string::npos) {
    mode |= 0755;
  } else {
    mode |= 0644;
  }

  return mode;
}

folly::Future<std::shared_ptr<MercurialFullManifest::FileInfo>>
MercurialFullManifest::getFileInfo(RelativePathPiece name) {
  return fileInfo_.get(name.copy().value());
}

folly::Future<folly::Unit> MercurialFullManifest::prefetchFileInfoForDir(
    RelativePathPiece name) {
  return via(wangle::getCPUExecutor().get()).then([ =, name = name.copy() ] {
    const auto& listing = getListing(name.value());
    if (listing.files.empty()) {
      return;
    }

    std::vector<std::string> args = {"hg",
                                     "files",
                                     "-r",
                                     repo_.getRev().toStdString(),
                                     "-vT",
                                     "{size}\\0{flags}\\0{abspath}\\n"};

    size_t n = 0;
    for (auto& file : listing.files) {
      auto full_name = (name + PathComponentPiece(file)).value().toStdString();
      if (fileInfo_.exists(full_name)) {
        continue;
      }
      args.emplace_back(std::move(full_name));
      n++;
    }

    if (n == 0) {
      // Everything is already warm enough
      return;
    }

    LOG(INFO) << "Running hg files on dir '" << name << "' for " << n
              << " files";
    folly::Subprocess proc(args,
                           folly::Subprocess::pipeStdout()
                               .chdir(repo_.getRepo()->getPath().toStdString())
                               .closeOtherFds()
                               .usePath());

    auto read_cb = folly::Subprocess::readLinesCallback([this](
        int fd, folly::StringPiece line) {
      if (fd == STDOUT_FILENO) {
        if (line.empty()) {
          return false;
        }

        line.removeSuffix("\n");
        folly::fbvector<folly::StringPiece> fields;
        folly::split('\0', line, fields);

        if (fields.size() != 3) {
          throw std::runtime_error(
              folly::to<std::string>("bad output from hg files: ", line));
        }

        fileInfo_.set(fields[2].fbstr(),
                      std::make_shared<FileInfo>(flags_to_mode(fields[1]),
                                                 folly::to<size_t>(fields[0])));
      }
      return false; // Keep reading from the child
    });
    proc.communicate(std::ref(read_cb), [](int, int) {
      // Don't write to the child
      return true;
    });
    proc.wait();
  });
}

folly::Future<std::shared_ptr<MercurialFullManifest::FileInfo>>
MercurialFullManifest::fetchFileInfo(const folly::fbstring& name) {
  // First, if it is a dir then we can very quickly return its info
  if (dirs_.find(name) != dirs_.end()) {
    return std::make_shared<FileInfo>(S_IFDIR | 0755, 0);
  }

  return via(wangle::getCPUExecutor().get()).then([=] {
    std::vector<std::string> args = {"hg",
                                     "files",
                                     "-r",
                                     repo_.getRev().toStdString(),
                                     "-vT",
                                     "{size}\\0{flags}\\n",
                                     name.toStdString()};
    LOG(INFO) << "Running hg files on " << name;
    folly::Subprocess proc(args,
                           folly::Subprocess::pipeStdout()
                               .chdir(repo_.getRepo()->getPath().toStdString())
                               .closeOtherFds()
                               .usePath());
    auto output = proc.communicate();
    proc.waitChecked();

    folly::StringPiece line(output.first);

    line.removeSuffix("\n");
    folly::fbvector<folly::StringPiece> fields;
    folly::split('\0', line, fields);

    if (fields.size() != 2) {
      throw std::runtime_error(
          folly::to<std::string>("bad output from hg files: ", line));
    }

    return std::make_shared<FileInfo>(flags_to_mode(fields[1]),
                                      folly::to<size_t>(fields[0]));

  });
}

const MercurialFullManifest::DirListing& MercurialFullManifest::getListing(
    const folly::fbstring& name) {
  return dirs_.at(name);
}

folly::Future<std::string> MercurialFullManifest::catFile(
    RelativePathPiece path) {
  return via(wangle::getCPUExecutor().get()).then([
    =,
    path = path.stringPiece().str()
  ] {
    folly::Subprocess proc(
        {"hg", "cat", "-r", repo_.getRev().toStdString(), path},
        folly::Subprocess::pipeStdout()
            .pipeStderr()
            .chdir(repo_.getRepo()->getPath().toStdString())
            .closeOtherFds()
            .usePath());
    auto output = proc.communicate();
    try {
      proc.waitChecked();
      if (!output.second.empty()) {
        LOG(ERROR) << "stderr not empty while running `hg cat -r "
                   << repo_.getRev() << " " << path << "`: " << output.second;
      }
      return output.first;
    } catch (const std::exception& e) {
      LOG(ERROR) << "Exception while running `hg cat -r " << repo_.getRev()
                 << " " << path << "`: " << e.what() << ", " << output.second;
      throw; // Will typically bubble up as EIO to the consumer
    }
  });
}
}
}
