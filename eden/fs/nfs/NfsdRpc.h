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

enum class nfsstat3 : uint32_t {
  NFS3_OK = 0,
  NFS3ERR_PERM = 1,
  NFS3ERR_NOENT = 2,
  NFS3ERR_IO = 5,
  NFS3ERR_NXIO = 6,
  NFS3ERR_ACCES = 13,
  NFS3ERR_EXIST = 17,
  NFS3ERR_XDEV = 18,
  NFS3ERR_NODEV = 19,
  NFS3ERR_NOTDIR = 20,
  NFS3ERR_ISDIR = 21,
  NFS3ERR_INVAL = 22,
  NFS3ERR_FBIG = 27,
  NFS3ERR_NOSPC = 28,
  NFS3ERR_ROFS = 30,
  NFS3ERR_MLINK = 31,
  NFS3ERR_NAMETOOLONG = 63,
  NFS3ERR_NOTEMPTY = 66,
  NFS3ERR_DQUOT = 69,
  NFS3ERR_STALE = 70,
  NFS3ERR_REMOTE = 71,
  NFS3ERR_BADHANDLE = 10001,
  NFS3ERR_NOT_SYNC = 10002,
  NFS3ERR_BAD_COOKIE = 10003,
  NFS3ERR_NOTSUPP = 10004,
  NFS3ERR_TOOSMALL = 10005,
  NFS3ERR_SERVERFAULT = 10006,
  NFS3ERR_BADTYPE = 10007,
  NFS3ERR_JUKEBOX = 10008
};

enum class ftype3 : uint32_t {
  NF3REG = 1,
  NF3DIR = 2,
  NF3BLK = 3,
  NF3CHR = 4,
  NF3LNK = 5,
  NF3SOCK = 6,
  NF3FIFO = 7
};

struct specdata3 {
  uint32_t specdata1;
  uint32_t specdata2;
};
EDEN_XDR_SERDE_DECL(specdata3, specdata1, specdata2);

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

struct nfstime3 {
  uint32_t seconds;
  uint32_t nseconds;
};
EDEN_XDR_SERDE_DECL(nfstime3, seconds, nseconds);

struct fattr3 {
  ftype3 type;
  uint32_t mode;
  uint32_t nlink;
  uint32_t uid;
  uint32_t gid;
  uint64_t size;
  uint64_t used;
  specdata3 rdev;
  uint64_t fsid;
  uint64_t fileid;
  nfstime3 atime;
  nfstime3 mtime;
  nfstime3 ctime;
};
EDEN_XDR_SERDE_DECL(
    fattr3,
    type,
    mode,
    nlink,
    uid,
    gid,
    size,
    used,
    rdev,
    fsid,
    fileid,
    atime,
    mtime,
    ctime);

struct post_op_attr : public XdrVariant<bool, fattr3> {};

template <>
struct XdrTrait<post_op_attr> : public XdrTrait<post_op_attr::Base> {
  static post_op_attr deserialize(folly::io::Cursor& cursor) {
    post_op_attr ret;
    ret.tag = XdrTrait<bool>::deserialize(cursor);
    if (ret.tag) {
      ret.v = XdrTrait<fattr3>::deserialize(cursor);
    }
    return ret;
  }
};

// FSINFO Procedure:

const uint32_t FSF3_LINK = 0x0001;
const uint32_t FSF3_SYMLINK = 0x0002;
const uint32_t FSF3_HOMOGENEOUS = 0x0008;
const uint32_t FSF3_CANSETTIME = 0x0010;

struct FSINFO3resok {
  post_op_attr obj_attributes;
  uint32_t rtmax;
  uint32_t rtpref;
  uint32_t rtmult;
  uint32_t wtmax;
  uint32_t wtpref;
  uint32_t wtmult;
  uint32_t dtpref;
  uint64_t maxfilesize;
  nfstime3 time_delta;
  uint32_t properties;
};
EDEN_XDR_SERDE_DECL(
    FSINFO3resok,
    obj_attributes,
    rtmax,
    rtpref,
    rtmult,
    wtmax,
    wtpref,
    wtmult,
    dtpref,
    maxfilesize,
    time_delta,
    properties);

struct FSINFO3resfail {
  post_op_attr obj_attributes;
};
EDEN_XDR_SERDE_DECL(FSINFO3resfail, obj_attributes);

struct FSINFO3res : public XdrVariant<nfsstat3, FSINFO3resok, FSINFO3resfail> {
};

template <>
struct XdrTrait<FSINFO3res> : public XdrTrait<FSINFO3res::Base> {
  static FSINFO3res deserialize(folly::io::Cursor& cursor) {
    FSINFO3res ret;
    ret.tag = XdrTrait<nfsstat3>::deserialize(cursor);
    switch (ret.tag) {
      case nfsstat3::NFS3_OK:
        ret.v = XdrTrait<FSINFO3resok>::deserialize(cursor);
        break;
      default:
        ret.v = XdrTrait<FSINFO3resfail>::deserialize(cursor);
        break;
    }
    return ret;
  }
};

// PATHCONF Procedure:

struct PATHCONF3resok {
  post_op_attr obj_attributes;
  uint32_t linkmax;
  uint32_t name_max;
  bool no_trunc;
  bool chown_restricted;
  bool case_insensitive;
  bool case_preserving;
};
EDEN_XDR_SERDE_DECL(
    PATHCONF3resok,
    obj_attributes,
    linkmax,
    name_max,
    no_trunc,
    chown_restricted,
    case_insensitive,
    case_preserving);

struct PATHCONF3resfail {
  post_op_attr obj_attributes;
};
EDEN_XDR_SERDE_DECL(PATHCONF3resfail, obj_attributes);

struct PATHCONF3res
    : public XdrVariant<nfsstat3, PATHCONF3resok, PATHCONF3resfail> {};

template <>
struct XdrTrait<PATHCONF3res> : public XdrTrait<PATHCONF3res::Base> {
  static PATHCONF3res deserialize(folly::io::Cursor& cursor) {
    PATHCONF3res ret;
    ret.tag = XdrTrait<nfsstat3>::deserialize(cursor);
    switch (ret.tag) {
      case nfsstat3::NFS3_OK:
        ret.v = XdrTrait<PATHCONF3resok>::deserialize(cursor);
        break;
      default:
        ret.v = XdrTrait<PATHCONF3resfail>::deserialize(cursor);
        break;
    }
    return ret;
  }
};

} // namespace facebook::eden

#endif
