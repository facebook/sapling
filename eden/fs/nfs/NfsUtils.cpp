/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/NfsUtils.h"

#ifndef _WIN32

#include <utility>

namespace facebook::eden {
uint32_t getEffectiveAccessRights(
    const struct stat& stat,
    uint32_t desiredAccess) {
  bool accessRead = (stat.st_mode & S_IRUSR) | (stat.st_mode & S_IRGRP) |
      (stat.st_mode & S_IROTH);
  bool accessWrite = (stat.st_mode & S_IWUSR) | (stat.st_mode & S_IWGRP) |
      (stat.st_mode & S_IWOTH);
  bool accessExecute = (stat.st_mode & S_IXUSR) | (stat.st_mode & S_IXGRP) |
      (stat.st_mode & S_IXOTH);

  // The delete bit indicates whether entries can be deleted from a directory
  // or not. NOT whether this file can be deleted. So this bit is kinda useless
  // for files. The NFS spec suggests that NFS servers should return 0 for
  // files, so we only set this bit for directories.
  bool accessDelete = (stat.st_mode & S_IFDIR) && accessWrite;

  uint32_t expandedAccessBits = 0;
  if (accessRead) {
    expandedAccessBits |= ACCESS3_READ;
    expandedAccessBits |= ACCESS3_LOOKUP;
  }

  if (accessWrite) {
    expandedAccessBits |= ACCESS3_MODIFY;
    expandedAccessBits |= ACCESS3_EXTEND;
  }

  if (accessDelete) {
    expandedAccessBits |= ACCESS3_DELETE;
  }

  if (accessExecute) {
    expandedAccessBits |= ACCESS3_EXECUTE;
  }

  return desiredAccess & expandedAccessBits;
}

} // namespace facebook::eden
#endif
