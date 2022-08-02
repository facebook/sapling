/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"

#include <fmt/format.h>

#include <folly/Range.h>
#include <folly/String.h>

namespace facebook::eden {
EDEN_XDR_SERDE_IMPL(specdata3, specdata1, specdata2);
EDEN_XDR_SERDE_IMPL(nfstime3, seconds, nseconds);
EDEN_XDR_SERDE_IMPL(
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
EDEN_XDR_SERDE_IMPL(wcc_attr, size, mtime, ctime);
EDEN_XDR_SERDE_IMPL(wcc_data, before, after);
EDEN_XDR_SERDE_IMPL(sattr3, mode, uid, gid, size, atime, mtime);
EDEN_XDR_SERDE_IMPL(diropargs3, dir, name);
EDEN_XDR_SERDE_IMPL(GETATTR3args, object);
EDEN_XDR_SERDE_IMPL(GETATTR3resok, obj_attributes);
EDEN_XDR_SERDE_IMPL(SETATTR3args, object, new_attributes, guard);
EDEN_XDR_SERDE_IMPL(SETATTR3resok, obj_wcc);
EDEN_XDR_SERDE_IMPL(SETATTR3resfail, obj_wcc);
EDEN_XDR_SERDE_IMPL(LOOKUP3args, what);
EDEN_XDR_SERDE_IMPL(LOOKUP3resok, object, obj_attributes, dir_attributes);
EDEN_XDR_SERDE_IMPL(LOOKUP3resfail, dir_attributes);
EDEN_XDR_SERDE_IMPL(ACCESS3args, object, access);
EDEN_XDR_SERDE_IMPL(ACCESS3resok, obj_attributes, access);
EDEN_XDR_SERDE_IMPL(ACCESS3resfail, obj_attributes);
EDEN_XDR_SERDE_IMPL(READLINK3args, symlink);
EDEN_XDR_SERDE_IMPL(READLINK3resok, symlink_attributes, data);
EDEN_XDR_SERDE_IMPL(READLINK3resfail, symlink_attributes);
EDEN_XDR_SERDE_IMPL(READ3args, file, offset, count);
EDEN_XDR_SERDE_IMPL(READ3resok, file_attributes, count, eof, data);
EDEN_XDR_SERDE_IMPL(READ3resfail, file_attributes);
EDEN_XDR_SERDE_IMPL(WRITE3args, file, offset, count, stable, data);
EDEN_XDR_SERDE_IMPL(WRITE3resok, file_wcc, count, committed, verf);
EDEN_XDR_SERDE_IMPL(WRITE3resfail, file_wcc);
EDEN_XDR_SERDE_IMPL(CREATE3args, where, how);
EDEN_XDR_SERDE_IMPL(CREATE3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(CREATE3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(MKDIR3args, where, attributes);
EDEN_XDR_SERDE_IMPL(MKDIR3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(MKDIR3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(symlinkdata3, symlink_attributes, symlink_data);
EDEN_XDR_SERDE_IMPL(SYMLINK3args, where, symlink);
EDEN_XDR_SERDE_IMPL(SYMLINK3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(SYMLINK3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(devicedata3, dev_attributes, spec);
EDEN_XDR_SERDE_IMPL(MKNOD3args, where, what);
EDEN_XDR_SERDE_IMPL(MKNOD3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(MKNOD3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(REMOVE3args, object);
EDEN_XDR_SERDE_IMPL(REMOVE3resok, dir_wcc);
EDEN_XDR_SERDE_IMPL(REMOVE3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(RMDIR3args, object);
EDEN_XDR_SERDE_IMPL(RMDIR3resok, dir_wcc);
EDEN_XDR_SERDE_IMPL(RMDIR3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(RENAME3args, from, to);
EDEN_XDR_SERDE_IMPL(RENAME3resok, fromdir_wcc, todir_wcc);
EDEN_XDR_SERDE_IMPL(RENAME3resfail, fromdir_wcc, todir_wcc);
EDEN_XDR_SERDE_IMPL(LINK3args, file, link);
EDEN_XDR_SERDE_IMPL(LINK3resok, file_attributes, linkdir_wcc);
EDEN_XDR_SERDE_IMPL(LINK3resfail, file_attributes, linkdir_wcc);
EDEN_XDR_SERDE_IMPL(READDIR3args, dir, cookie, cookieverf, count);
EDEN_XDR_SERDE_IMPL(entry3, fileid, name, cookie);
EDEN_XDR_SERDE_IMPL(dirlist3, entries, eof);
EDEN_XDR_SERDE_IMPL(READDIR3resok, dir_attributes, cookieverf, reply);
EDEN_XDR_SERDE_IMPL(READDIR3resfail, dir_attributes);
EDEN_XDR_SERDE_IMPL(
    READDIRPLUS3args,
    dir,
    cookie,
    cookieverf,
    dircount,
    maxcount);
EDEN_XDR_SERDE_IMPL(
    entryplus3,
    fileid,
    name,
    cookie,
    name_attributes,
    name_handle);
EDEN_XDR_SERDE_IMPL(dirlistplus3, entries, eof);
EDEN_XDR_SERDE_IMPL(READDIRPLUS3resok, dir_attributes, cookieverf, reply);
EDEN_XDR_SERDE_IMPL(FSSTAT3args, fsroot);
EDEN_XDR_SERDE_IMPL(
    FSSTAT3resok,
    obj_attributes,
    tbytes,
    fbytes,
    abytes,
    tfiles,
    ffiles,
    afiles,
    invarsec);
EDEN_XDR_SERDE_IMPL(FSSTAT3resfail, obj_attributes);
EDEN_XDR_SERDE_IMPL(FSINFO3args, fsroot);
EDEN_XDR_SERDE_IMPL(
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
EDEN_XDR_SERDE_IMPL(FSINFO3resfail, obj_attributes);
EDEN_XDR_SERDE_IMPL(PATHCONF3args, object);
EDEN_XDR_SERDE_IMPL(
    PATHCONF3resok,
    obj_attributes,
    linkmax,
    name_max,
    no_trunc,
    chown_restricted,
    case_insensitive,
    case_preserving);
EDEN_XDR_SERDE_IMPL(PATHCONF3resfail, obj_attributes);

RpcParsingError constructInodeParsingError(
    folly::io::Cursor cursor,
    uint32_t size) {
  auto offset = cursor.getCurrentPosition();
  std::unique_ptr<folly::IOBuf> file_handle_bytes;
  cursor.cloneAtMost(file_handle_bytes, size);

  return RpcParsingError{fmt::format(
      "Failed to parse {} into an InodeNumber at input offset {}",
      folly::hexlify(file_handle_bytes->coalesce()),
      offset)};
}

} // namespace facebook::eden

#endif
