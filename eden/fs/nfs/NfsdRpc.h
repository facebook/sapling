/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/nfs/rpc/Rpc.h"

/*
 * Nfsd protocol described in RFC1813:
 * https://tools.ietf.org/html/rfc1813
 */

namespace facebook::eden {

constexpr uint32_t kNfsdProgNumber = 100003;
constexpr uint32_t kNfsd3ProgVersion = 3;

/**
 * Procedure values.
 */
enum class nfsv3Procs : uint32_t {
  null = 0,
  getattr = 1,
  setattr = 2,
  lookup = 3,
  access = 4,
  readlink = 5,
  read = 6,
  write = 7,
  create = 8,
  mkdir = 9,
  symlink = 10,
  mknod = 11,
  remove = 12,
  rmdir = 13,
  rename = 14,
  link = 15,
  readdir = 16,
  readdirplus = 17,
  fsstat = 18,
  fsinfo = 19,
  pathconf = 20,
  commit = 21,
};

/**
 * The NFS spec specify this struct as being opaque from the client
 * perspective, and thus we are free to use what is needed to uniquely identify
 * a file. In EdenFS, this is perfectly represented by an InodeNumber.
 *
 * As an InodeNumber is unique per mount, an Nfsd program can only handle one
 * mount per instance. This will either need to be extended to support multiple
 * mounts, or an Nfsd instance per mount will need to be created.
 *
 * Note that this structure is serialized as an opaque byte vector, and will
 * thus be preceded by a uint32_t.
 */
struct nfs_fh3 {
  InodeNumber ino;
};

template <>
struct XdrTrait<nfs_fh3> {
  static void serialize(folly::io::Appender& appender, const nfs_fh3& fh) {
    XdrTrait<uint32_t>::serialize(appender, sizeof(nfs_fh3));
    XdrTrait<uint64_t>::serialize(appender, fh.ino.get());
  }

  static nfs_fh3 deserialize(folly::io::Cursor& cursor) {
    uint32_t size = XdrTrait<uint32_t>::deserialize(cursor);
    XCHECK_EQ(size, sizeof(nfs_fh3));
    return {InodeNumber{XdrTrait<uint64_t>::deserialize(cursor)}};
  }
};

inline bool operator==(const nfs_fh3& a, const nfs_fh3& b) {
  return a.ino == b.ino;
}

} // namespace facebook::eden

#endif
