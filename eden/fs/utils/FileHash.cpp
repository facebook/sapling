/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/OpenSSL.h>
#include <filesystem>

#include "eden/common/utils/WinError.h"
#include "eden/fs/digest/Blake3.h"
#include "eden/fs/utils/FileHash.h"

namespace facebook::eden {

#ifdef _WIN32

namespace {
constexpr size_t kBufSize = 8192;

template <typename Hasher>
void hash(Hasher&& hasher, AbsolutePathPiece filePath) {
  const auto widePath = filePath.wide();

  std::error_code ec;
  auto stdPath = std::filesystem::path(widePath);
  auto lnk = std::filesystem::read_symlink(stdPath, ec);
  if (ec.value() == 0) {
    std::wstring lnkW = lnk.wstring();
    std::string content;
    std::transform(
        lnkW.begin(), lnkW.end(), std::back_inserter(content), [](wchar_t c) {
          return (char)c;
        });
    std::replace(content.begin(), content.end(), '\\', '/');
    hasher(content.c_str(), content.size());
    return;
  }

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

  uint8_t buf[kBufSize];
  while (true) {
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

    hasher(buf, bytesRead);
  }
}
} // namespace

Hash32 getFileBlake3(
    AbsolutePathPiece filePath,
    const std::optional<std::string>& maybeBlake3Key) {
  auto hasher = Blake3::create(maybeBlake3Key);
  hash(
      [&hasher](const auto* buf, auto len) { hasher.update(buf, len); },
      filePath);
  static_assert(Hash32::RAW_SIZE == BLAKE3_OUT_LEN);
  Hash32 blake3;
  hasher.finalize(blake3.mutableBytes());

  return blake3;
}

Hash20 getFileSha1(AbsolutePathPiece filePath) {
  SHA_CTX ctx;
  SHA1_Init(&ctx);
  hash(
      [&ctx](const auto* buf, auto len) { SHA1_Update(&ctx, buf, len); },
      filePath);
  static_assert(Hash20::RAW_SIZE == SHA_DIGEST_LENGTH);
  Hash20 sha1;
  SHA1_Final(sha1.mutableBytes().begin(), &ctx);

  return sha1;
}

#endif

} // namespace facebook::eden
