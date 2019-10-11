/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

/*
 * This file contains helper functions for creating temporary files and
 * directories.  These are small utilities that use folly's TemporaryDirectory
 * and TemporaryFile underneath.
 *
 * The main advantage of these functions is that they try to do smarter job
 * about picking a location for temporary files.  Many of the Eden tests are
 * somewhat I/O heavy, and the tests can be quite slow if the temporary files
 * are stored on a physical spinning disk.  This attempts to put temporary files
 * in ramdisk if possible.
 */

#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>

namespace facebook {
namespace eden {

folly::test::TemporaryFile makeTempFile(
    folly::StringPiece prefix,
    folly::test::TemporaryFile::Scope scope =
        folly::test::TemporaryFile::Scope::UNLINK_ON_DESTRUCTION);

inline folly::test::TemporaryFile makeTempFile(
    folly::test::TemporaryFile::Scope scope =
        folly::test::TemporaryFile::Scope::UNLINK_ON_DESTRUCTION) {
  return makeTempFile("eden_test", scope);
}

folly::test::TemporaryDirectory makeTempDir(
    folly::StringPiece prefix,
    folly::test::TemporaryDirectory::Scope scope =
        folly::test::TemporaryDirectory::Scope::DELETE_ON_DESTRUCTION);

inline folly::test::TemporaryDirectory makeTempDir(
    folly::test::TemporaryDirectory::Scope scope =
        folly::test::TemporaryDirectory::Scope::DELETE_ON_DESTRUCTION) {
  return makeTempDir("eden_test", scope);
}
} // namespace eden
} // namespace facebook
