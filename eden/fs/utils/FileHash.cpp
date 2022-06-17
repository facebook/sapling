/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileHash.h"
#include <folly/portability/OpenSSL.h>
#include "eden/common/utils/WinError.h"

namespace facebook::eden {

#ifdef _WIN32

Hash20 getFileSha1(AbsolutePathPiece filePath) {
  auto widePath = filePath.wide();

  HANDLE fileHandle = CreateFileW(
      widePath.c_str(),
      GENERIC_READ,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL,
      nullptr);
  if (INVALID_HANDLE_VALUE == fileHandle) {
    throw makeWin32ErrorExplicit(
        GetLastError(), fmt::format(FMT_STRING("couldn't open {}"), filePath));
  }

  SCOPE_EXIT {
    CloseHandle(fileHandle);
  };

  SHA_CTX ctx;
  SHA1_Init(&ctx);
  while (true) {
    uint8_t buf[8192];

    DWORD bytesRead;
    if (!ReadFile(fileHandle, buf, sizeof(buf), &bytesRead, nullptr)) {
      throw makeWin32ErrorExplicit(
          GetLastError(),
          fmt::format(
              FMT_STRING("Error while computing SHA1 of {}"), filePath));
    }

    if (bytesRead == 0) {
      break;
    }

    SHA1_Update(&ctx, buf, bytesRead);
  }

  static_assert(Hash20::RAW_SIZE == SHA_DIGEST_LENGTH);
  Hash20 sha1;
  SHA1_Final(sha1.mutableBytes().begin(), &ctx);

  return sha1;
}

#endif

} // namespace facebook::eden
