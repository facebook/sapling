/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <fmt/printf.h>
#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/StringConv.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProjfsUtil.h"

DEFINE_string(path, "", "The path to the file to check for rename.");
DEFINE_bool(
    checksparse,
    false,
    "Use the sparse attribute to limit rename checks.");

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);
#ifdef _WIN32
  if (FLAGS_path.empty()) {
    fmt::print(stderr, "error: the --path argument is required\n");
    return 3;
  }

  auto path = facebook::eden::canonicalPath(FLAGS_path);

  if (FLAGS_checksparse) {
    WIN32_FIND_DATAW findFileData;
    HANDLE h = FindFirstFileExW(
        path.wide().c_str(),
        FindExInfoBasic,
        &findFileData,
        FindExSearchNameMatch,
        nullptr,
        0);

    if (h == INVALID_HANDLE_VALUE) {
      fmt::print("unable to find file - {}", path);
      return 3;
    }

    auto sparse = (findFileData.dwFileAttributes &
                   FILE_ATTRIBUTE_SPARSE_FILE) == FILE_ATTRIBUTE_SPARSE_FILE;

    if (!sparse) {
      fmt::print("file is not marked sparse - {}", path);
      return 4;
    }
  }

  auto result = facebook::eden::isRenamedPlaceholder(path.wide().c_str());
  if (result.hasException()) {
    fmt::print(
        stderr,
        "exception checking reparse point - {} - {}",
        path.value(),
        result.exception().what());
    return 2;
  }

  if (result.value()) {
    return 0;
  } else {
    fmt::print(stderr, "file is not renamed - {}", path.value());
    return 1;
  }
#else
  return 0;
#endif
}
