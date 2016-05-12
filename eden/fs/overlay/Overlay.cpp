/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Overlay.h"
#include <dirent.h>
#include <folly/Exception.h>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using folly::File;
using folly::StringPiece;
using folly::fbstring;
using folly::fbvector;

/* Overlay directory structure.
 *
 * We draw on two concepts from unionfs:
 * - Whiteout files
 * - Opaque files.
 *
 * When we remove an entry from the layer beneath the overlay, we create a
 * whiteout file as a placeholder to track that it is no longer there.
 * The whiteout file has a special name prefix so that we can elide it from
 * the normal directory listing; we prefix the original name with kWhiteout
 * so that we can return a special entry for the name sans-prefix.
 *
 * There is a special case where we have deleted a directory and then created
 * a new directory in its place.  In this situation we need to signal to the
 * consumer of Overlay that this new generation of the dir is opaque wrt.
 * the layer beneath us.  We use a special Opaque file for this purpose; if
 * it is present in a directory, then that directory is considered to be opaque.
 *
 * Neither the whiteout files nor the opaque files are visible via the readdir
 * method of the Overlay class.
 */

/// Files with this prefix have been removed from the layer beneath
constexpr StringPiece kWhiteout{".edenrm."};

/// Files with this name indicate a directory that obscures the layer beneath
constexpr StringPiece kOpaque{".edenopaque"};

Overlay::Overlay(AbsolutePathPiece localDir) : localDir_(localDir) {}

Overlay::DirContents Overlay::readDir(RelativePathPiece path) {
  auto dirPath = localDir_ + path;

  auto dir = opendir(dirPath.c_str());
  if (!dir) {
    if (errno == ENOENT) {
      // If the dir doesn't exist it either means that we have no overlay info,
      // or that there may be a whiteout for some component of the directory
      // structure they're looking for.
      if (isWhiteout(path)) {
        folly::throwSystemErrorExplicit(ENOENT);
      }

      // If we make it here, we know that we have no positive information
      // about the deletion status or any overlay content, so we return
      // an empty set.
      return DirContents();
    }
    // Something funky going on: throw an error.
    folly::throwSystemError("opening overlay dir ", dirPath);
  }

  SCOPE_EXIT {
    closedir(dir);
  };

  DirContents contents;
  dirent* ent;
  while ((ent = readdir(dir)) != nullptr) {
    StringPiece name(ent->d_name);

    if (name == "." || name == "..") {
      continue;
    }

    if (name == kOpaque) {
      contents.isOpaque = true;
      continue;
    }

    // We pass up the underlying d_type field; depending on the filesystem
    // that backs the local dir, this may be set to something useful or
    // may just be simply DT_UNKNOWN.
    auto d_type = static_cast<dtype_t>(ent->d_type);

    if (name.startsWith(kWhiteout)) {
      d_type = dtype_t::Whiteout;
      // Report the un-decorated named
      name.advance(kWhiteout.size());
    }
    contents.entries.emplace(std::make_pair(PathComponent(name), d_type));
  }

  return contents;
}

bool Overlay::isWhiteout(RelativePathPiece path) {
  if (path.empty()) {
    return false;
  }

  // Iterate the various path combinations in path.
  for (auto candidatePath : path) {
    struct stat st;

    auto whiteoutPath = localDir_ + candidatePath.dirname() +
        PathComponent(folly::to<fbstring>(
            kWhiteout, candidatePath.basename().stringPiece()));
    if (stat(whiteoutPath.c_str(), &st) == 0) {
      // It's been whiteout'd (whited'out?)
      return true;
    }

    auto fullCandidatePath = localDir_ + candidatePath;
    if (stat(fullCandidatePath.c_str(), &st) == 0) {
      // Doesn't exist; we have no information, fall out the bottom.
      break;
    }

    // OK, not whiteout'd.  Carry on.
    // Optimization note: if this path proves to be hot, we could build
    // out an empty directory tree down to the leaf to avoid this work.
  }
  return false;
}

void Overlay::makeDirs(RelativePathPiece path) {
  if (path.empty()) {
    // We're already at the root.
    return;
  }

  auto parent = path.dirname();
  if (!parent.empty()) {
    makeDirs(parent);
  }

  auto dirPath = localDir_ + path;
  folly::checkUnixError(
      mkdir(dirPath.value().c_str(), 0700), "mkdir: ", dirPath);
}

RelativePath Overlay::computeWhiteoutName(RelativePathPiece path) {
  auto dir = path.dirname();
  auto base = path.basename();

  return dir + PathComponent(folly::to<fbstring>(kWhiteout, base));
}

void Overlay::makeWhiteout(RelativePathPiece path) {
  auto whitename = localDir_ + computeWhiteoutName(path);
  // the file descriptor for whiteFile will be automatically released
  // when we return from this function.
  File whiteFile(whitename.c_str(), O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
}

void Overlay::makeOpaque(RelativePathPiece path) {
  auto oname = localDir_ + path + PathComponentPiece(kOpaque);
  // the file descriptor for file will be automatically released
  // when we return from this function.
  File file(oname.c_str(), O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
}

bool Overlay::removeWhiteout(RelativePathPiece path) {
  auto white = localDir_ + computeWhiteoutName(path);
  if (unlink(white.c_str()) == 0) {
    // we removed the whiteout.
    return true;
  }
  if (errno == ENOENT) {
    // There was no whiteout to remove.
    return false;
  }

  // There was an error removing the whiteout.
  folly::throwSystemError("unlink ", white);
}

void Overlay::removeDir(RelativePathPiece path, bool needWhiteout) {
  auto dirPath = localDir_ + path;

  // We allow for this to fail with ENOENT in the case that we have an
  // empty local tree and want to record a delete for something that we
  // haven't materialized yet.
  if (rmdir(dirPath.c_str()) == -1 && errno != ENOENT) {
    folly::throwSystemError("rmdir: ", dirPath);
  }

  if (needWhiteout) {
    makeWhiteout(path);
  }
}

void Overlay::removeFile(RelativePathPiece path, bool needWhiteout) {
  auto filePath = localDir_ + path;

  // We allow for this to fail with ENOENT in the case that we have an
  // empty local tree and want to record a delete for something that we
  // haven't materialized yet.
  if (unlink(filePath.c_str()) == -1 && errno != ENOENT) {
    folly::throwSystemError("unlink: ", filePath);
  }

  if (needWhiteout) {
    makeWhiteout(path);
  }
}

void Overlay::makeDir(RelativePathPiece path, mode_t mode) {
  auto dirPath = localDir_ + path;

  auto parent = path.dirname();
  if (isWhiteout(parent)) {
    folly::throwSystemErrorExplicit(
        ENOTDIR, "a parent of ", path, " is whiteout");
  }

  makeDirs(parent);

  folly::checkUnixError(
      mkdir(dirPath.c_str(), mode), "mkdir: ", dirPath, " mode=", mode);

  if (removeWhiteout(path)) {
    // Transitioning from whiteout -> dir makes this an opaque dir
    makeOpaque(path);
  }
}

folly::File Overlay::openFile(RelativePathPiece path, int flags, mode_t mode) {
  auto parent = path.dirname();
  if (isWhiteout(parent)) {
    folly::throwSystemErrorExplicit(
        ENOTDIR, "a parent of ", path, " is whiteout");
  }
  makeDirs(parent);

  auto filePath = localDir_ + path;
  folly::File file(filePath.c_str(), flags, mode);

  if (flags & O_CREAT) {
    removeWhiteout(path);
  }

  return file;
}

const AbsolutePath& Overlay::getLocalDir() const {
  return localDir_;
}
}
}
