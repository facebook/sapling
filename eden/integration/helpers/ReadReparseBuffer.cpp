/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <fmt/core.h>
#include <folly/String.h>
#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/StringConv.h"
#include "eden/common/utils/windows/WinError.h"

DEFINE_string(path, "", "The path to the file to check for rename.");

using namespace facebook::eden;

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);
  if (FLAGS_path.empty()) {
    fmt::print(stderr, "error: the --path argument is required\n");
    return 1;
  }

  auto path = canonicalPath(FLAGS_path);

  FileHandle handle{CreateFileW(
      path.wide().c_str(),
      0,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
      nullptr)};
  if (handle.get() == INVALID_HANDLE_VALUE) {
    fmt::print(
        "Unable to determine reparse point type for {}: {}",
        path,
        win32ErrorToString(GetLastError()));
    return 1;
  }

  try {
    auto reparse_data = getReparseData(handle.get());
    unsigned char* reparse_buffer =
        reparse_data.value()->GenericReparseBuffer.DataBuffer;
    fmt::print(
        "{}",
        folly::hexlify(std::string_view{
            reinterpret_cast<char*>(reparse_buffer),
            reparse_data.value()->ReparseDataLength}));
    return 0;
  } catch (std::exception& err) {
    fmt::print(
        stderr,
        "exception checking reparse point - {} - {}",
        path.value(),
        err.what());
    return 1;
  }
}
