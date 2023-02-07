/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProjfsUtil.h"

#ifdef _WIN32
#include <fmt/format.h>

#include "folly/Try.h"

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/StringConv.h"
#include "eden/fs/utils/FileUtils.h"

namespace facebook::eden {

namespace {
// byte 4 in the projFs flag byte seems to have a bit that represents renamed
// placeholders from manual testing.
static const uint8_t PROJFS_RENAMED_BIT = 1 << 3;
} // namespace

folly::Try<bool> isRenamedPlaceholder(const wchar_t* path) {
  FileHandle handle{CreateFileW(
      path,
      0,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      NULL,
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
      NULL)};
  if (handle.get() == INVALID_HANDLE_VALUE) {
    return folly::Try<bool>{std::runtime_error{fmt::format(
        "Unable to get a handle to determine reparse point type for {}: {}",
        wideToMultibyteString<std::string>(path),
        GetLastError())}};
  }

  auto reparse_data = getReparseData(handle.get());
  if (reparse_data.hasException()) {
    return folly::Try<bool>(reparse_data.exception());
  }
  if (reparse_data.value()->ReparseDataLength == 0) {
    return folly::Try<bool>(false);
  }
  uint8_t projfs_type_byte =
      reparse_data.value()->ProjFsReparseBuffer.ProjFsFlags;
  bool renamed = projfs_type_byte & PROJFS_RENAMED_BIT;
  return folly::Try<bool>(renamed);
}

} // namespace facebook::eden
#endif
