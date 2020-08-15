/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileUtils.h"
#include <folly/Format.h>
#include <folly/Utility.h>
#include <openssl/sha.h>
#include <filesystem>
#include <iostream>
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/FileUtils.h"
#include "eden/fs/win/utils/WinError.h"

using folly::ByteRange;
using folly::sformat;

namespace facebook {
namespace eden {

struct FileHandleTraits {
  using Type = HANDLE;

  static Type invalidHandleValue() noexcept {
    return INVALID_HANDLE_VALUE;
  }
  static void close(Type handle) noexcept {
    CloseHandle(handle);
  }
};

using FileHandle = HandleBase<FileHandleTraits>;

Hash getFileSha1(AbsolutePathPiece filePath) {
  SHA_CTX ctx;
  SHA1_Init(&ctx);

  auto winPath = filePath.wide();

  FileHandle fileHandle{CreateFileW(
      winPath.c_str(),
      GENERIC_READ,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL,
      nullptr)};

  while (true) {
    uint8_t buf[8192];

    DWORD bytesRead;
    if (!ReadFile(fileHandle.get(), buf, sizeof(buf), &bytesRead, nullptr)) {
      throw makeWin32ErrorExplicit(
          GetLastError(),
          sformat("Error while computing SHA1 of {}", filePath));
    }

    if (bytesRead == 0) {
      break;
    }

    SHA1_Update(&ctx, buf, bytesRead);
  }

  static_assert(Hash::RAW_SIZE == SHA_DIGEST_LENGTH);
  Hash sha1;
  SHA1_Final(sha1.mutableBytes().begin(), &ctx);

  return sha1;
}

} // namespace eden
} // namespace facebook
