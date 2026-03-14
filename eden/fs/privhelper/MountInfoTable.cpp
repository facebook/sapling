/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/privhelper/MountInfoTable.h"

#ifdef __linux__

#include <folly/logging/xlog.h>

#include <unistd.h>
#include <cerrno>
#include <cstring>

#include "eden/fs/utils/Statmount.h"

// STATMOUNT_SB_SOURCE was added in Linux 6.11; define locally if missing.
#ifndef STATMOUNT_SB_SOURCE
#define STATMOUNT_SB_SOURCE 0x00000100U
#endif

namespace facebook::eden {

namespace {

constexpr uint64_t kStatmountMask = STATMOUNT_SB_BASIC | STATMOUNT_MNT_POINT |
    STATMOUNT_FS_TYPE | STATMOUNT_SB_SOURCE;

constexpr size_t kStatmountBufSize = 4096;
constexpr size_t kListmountBufSize = 1024;

/**
 * Call listmount(2) to enumerate all mount IDs under LSMT_ROOT.
 * Returns an error code on failure (ENOSYS for unsupported kernels,
 * or the errno from the failed syscall).
 */
folly::Expected<std::vector<uint64_t>, int> listAllMountIds() {
  std::vector<uint64_t> ids(kListmountBufSize);
  struct mnt_id_req req{};
  req.size = MNT_ID_REQ_SIZE_VER0;
  req.mnt_id = LSMT_ROOT;
  req.param = 0;

  long ret = syscall(__NR_listmount, &req, ids.data(), ids.size(), 0);
  if (ret < 0) {
    return folly::makeUnexpected(errno);
  }

  if (static_cast<size_t>(ret) == kListmountBufSize) {
    XLOGF(
        WARN,
        "listmount(2) returned exactly {} entries; results may be truncated",
        kListmountBufSize);
  }

  ids.resize(static_cast<size_t>(ret));
  return ids;
}

/**
 * Call statmount(2) for a single mount ID.
 * Returns an error code on failure (ENOSYS for unsupported kernels,
 * or the errno from the failed syscall).
 */
folly::Expected<MountInfo, int> statmountById(uint64_t mntId) {
  // Allocate buffer for statmount result including variable-length strings
  std::vector<char> buf(kStatmountBufSize);
  auto* sm = reinterpret_cast<struct statmount*>(buf.data());

  struct mnt_id_req req{};
  req.size = MNT_ID_REQ_SIZE_VER0;
  req.mnt_id = mntId;
  req.param = kStatmountMask;

  long ret = syscall(__NR_statmount, &req, sm, buf.size(), 0);
  if (ret < 0) {
    if (errno == EOVERFLOW) {
      // Buffer too small — try again with reported size
      buf.resize(sm->size);
      sm = reinterpret_cast<struct statmount*>(buf.data());
      ret = syscall(__NR_statmount, &req, sm, buf.size(), 0);
      if (ret < 0) {
        return folly::makeUnexpected(errno);
      }
    } else {
      return folly::makeUnexpected(errno);
    }
  }

  MountInfo info;
  info.devMajor = sm->sb_dev_major;
  info.devMinor = sm->sb_dev_minor;

  if (sm->mask & STATMOUNT_MNT_POINT) {
    info.mountPoint = sm->str + sm->mnt_point;
  }
  if (sm->mask & STATMOUNT_FS_TYPE) {
    info.fsType = sm->str + sm->fs_type;
  }
  if (sm->mask & STATMOUNT_SB_SOURCE) {
    info.mountSource = sm->str + sm->sb_source;
  }

  return info;
}

} // namespace

folly::Expected<std::optional<MountInfo>, int> getMountInfoForPath(
    const char* path) {
  auto idsResult = listAllMountIds();
  if (idsResult.hasError()) {
    return folly::makeUnexpected(idsResult.error());
  }

  for (auto id : idsResult.value()) {
    auto infoResult = statmountById(id);
    if (infoResult.hasError()) {
      return folly::makeUnexpected(infoResult.error());
    }
    if (infoResult.value().mountPoint == path) {
      return infoResult.value();
    }
  }
  return std::nullopt;
}

folly::Expected<std::vector<MountInfo>, int> getMountsUnderPath(
    const std::string& prefix) {
  std::vector<MountInfo> result;
  std::string prefixWithSlash = prefix + "/";

  auto idsResult = listAllMountIds();
  if (idsResult.hasError()) {
    return folly::makeUnexpected(idsResult.error());
  }

  for (auto id : idsResult.value()) {
    auto infoResult = statmountById(id);
    if (infoResult.hasError()) {
      return folly::makeUnexpected(infoResult.error());
    }
    if (infoResult.value().mountPoint.compare(
            0, prefixWithSlash.size(), prefixWithSlash) == 0) {
      result.push_back(std::move(infoResult.value()));
    }
  }
  return result;
}

} // namespace facebook::eden

#endif // __linux__
#endif // _WIN32
