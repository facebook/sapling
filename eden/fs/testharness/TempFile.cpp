/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/TempFile.h"

#include <unistd.h>
#include <cstdlib>
#include "eden/fs/utils/SystemError.h"

using folly::StringPiece;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;

namespace {
boost::filesystem::path computeTempDir() {
  const char* envVar = nullptr;
  if ((envVar = std::getenv("TMPDIR")) || (envVar = std::getenv("TMP")) ||
      (envVar = std::getenv("TEMP")) || (envVar = std::getenv("TEMPDIR"))) {
    // If we found an explicit directory through the environment, use that.
    // We canonicalize it because `/var/tmp` on macOS is a symlink and
    // some of our tests compare the results of canonicalizing things
    // that are relative to it.
    return boost::filesystem::canonical(boost::filesystem::path(envVar));
  }

  // Try the following locations in order:
  for (const auto& path : {"/dev/shm", "/tmp"}) {
    if (access(path, W_OK) == 0) {
      return boost::filesystem::path(path);
    }
  }

  throw std::runtime_error("unable to find a suitable temporary directory");
}

const boost::filesystem::path& getTempDir() {
  static const auto tempDir = computeTempDir();
  return tempDir;
}
} // namespace

namespace facebook {
namespace eden {

TemporaryFile makeTempFile(StringPiece prefix, TemporaryFile::Scope scope) {
  return TemporaryFile(prefix, getTempDir(), scope);
}

TemporaryDirectory makeTempDir(
    StringPiece prefix,
    TemporaryDirectory::Scope scope) {
  return TemporaryDirectory(prefix, getTempDir(), scope);
}

} // namespace eden
} // namespace facebook
