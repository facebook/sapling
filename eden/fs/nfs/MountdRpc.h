/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"

/*
 * Mountd prococol described in the Appendix I of RFC1813:
 * https://tools.ietf.org/html/rfc1813#page-106
 */

namespace facebook::eden {

constexpr uint32_t kMountdProgNumber = 100005;
constexpr uint32_t kMountdProgVersion = 3;

/**
 * Procedure values.
 */
enum class mountProcs : uint32_t {
  null = 0,
  mnt = 1,
  dump = 2,
  umnt = 3,
  umntAll = 4,
  exprt = 5,
};

enum class mountstat3 {
  MNT3_OK = 0, /* no error */
  MNT3ERR_PERM = 1, /* Not owner */
  MNT3ERR_NOENT = 2, /* No such file or directory */
  MNT3ERR_IO = 5, /* I/O error */
  MNT3ERR_ACCES = 13, /* Permission denied */
  MNT3ERR_NOTDIR = 20, /* Not a directory */
  MNT3ERR_INVAL = 22, /* Invalid argument */
  MNT3ERR_NAMETOOLONG = 63, /* Filename too long */
  MNT3ERR_NOTSUPP = 10004, /* Operation not supported */
  MNT3ERR_SERVERFAULT = 10006 /* A failure on the server */
};

/**
 * Return value of the mnt procedure.
 */
struct mountres3_ok {
  nfs_fh3 fhandle3;
  std::vector<auth_flavor> auth_flavors;
};
EDEN_XDR_SERDE_DECL(mountres3_ok, fhandle3, auth_flavors);

} // namespace facebook::eden

#endif
