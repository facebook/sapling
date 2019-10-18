/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/Dispatcher.h"

#include <folly/Exception.h>
#include <folly/Format.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>

#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/utils/StatTimes.h"

using namespace folly;

namespace facebook {
namespace eden {

Dispatcher::Attr::Attr(const struct stat& st, uint64_t timeout)
    : st(st), timeout_seconds(timeout) {}

fuse_attr_out Dispatcher::Attr::asFuseAttr() const {
  // Ensure that we initialize the members to zeroes;
  // This is important on macOS where there are a couple
  // of additional fields (notably `flags`) that influence
  // file accessibility.
  fuse_attr_out result{};

  result.attr.ino = st.st_ino;
  result.attr.size = st.st_size;
  result.attr.blocks = st.st_blocks;
  result.attr.atime = st.st_atime;
  result.attr.atimensec = stAtime(st).tv_nsec;
  result.attr.mtime = st.st_mtime;
  result.attr.mtimensec = stMtime(st).tv_nsec;
  result.attr.ctime = st.st_ctime;
  result.attr.ctimensec = stCtime(st).tv_nsec;
  result.attr.mode = st.st_mode;
  result.attr.nlink = st.st_nlink;
  result.attr.uid = st.st_uid;
  result.attr.gid = st.st_gid;
  result.attr.rdev = st.st_rdev;
  result.attr.blksize = st.st_blksize;

  result.attr_valid_nsec = 0;
  result.attr_valid = timeout_seconds;

  return result;
}

Dispatcher::~Dispatcher() {}

Dispatcher::Dispatcher(EdenStats* stats) : stats_(stats) {}

void Dispatcher::initConnection(const fuse_init_out& out) {
  connInfo_ = out;
}

void Dispatcher::destroy() {}

folly::Future<fuse_entry_out> Dispatcher::lookup(
    InodeNumber /*parent*/,
    PathComponentPiece /*name*/) {
  throwSystemErrorExplicit(ENOENT);
}

void Dispatcher::forget(InodeNumber /*ino*/, unsigned long /*nlookup*/) {}

folly::Future<Dispatcher::Attr> Dispatcher::getattr(InodeNumber /*ino*/) {
  throwSystemErrorExplicit(ENOENT);
}

folly::Future<Dispatcher::Attr> Dispatcher::setattr(
    InodeNumber /*ino*/,
    const fuse_setattr_in& /*attr*/
) {
  FUSELL_NOT_IMPL();
}

folly::Future<std::string> Dispatcher::readlink(
    InodeNumber /*ino*/,
    bool /*kernelCachesReadlink*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out> Dispatcher::mknod(
    InodeNumber /*parent*/,
    PathComponentPiece /*name*/,
    mode_t /*mode*/,
    dev_t /*rdev*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::mkdir(InodeNumber, PathComponentPiece, mode_t) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::unlink(InodeNumber, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::rmdir(InodeNumber, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::symlink(InodeNumber, PathComponentPiece, folly::StringPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::rename(
    InodeNumber,
    PathComponentPiece,
    InodeNumber,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::link(InodeNumber, InodeNumber, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<uint64_t> Dispatcher::open(InodeNumber /*ino*/, int /*flags*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::release(
    InodeNumber /*ino*/,
    uint64_t /*fh*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<uint64_t> Dispatcher::opendir(
    InodeNumber /*ino*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::releasedir(
    InodeNumber /*ino*/,
    uint64_t /*fh*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<BufVec>
Dispatcher::read(InodeNumber /*ino*/, size_t /*size*/, off_t /*off*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<size_t>
Dispatcher::write(InodeNumber /*ino*/, StringPiece /*data*/, off_t /*off*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::flush(InodeNumber, uint64_t) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::fsync(InodeNumber, bool) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::fsyncdir(InodeNumber, bool) {
  FUSELL_NOT_IMPL();
}

folly::Future<DirList>
Dispatcher::readdir(InodeNumber, DirList&&, off_t, uint64_t) {
  FUSELL_NOT_IMPL();
}

folly::Future<struct fuse_kstatfs> Dispatcher::statfs(InodeNumber /*ino*/) {
  struct fuse_kstatfs info = {};

  // Suggest a large blocksize to software that looks at that kind of thing
  // bsize will be returned to applications that call pathconf() with
  // _PC_REC_MIN_XFER_SIZE
  info.bsize = getConnInfo().max_readahead;

  // The fragment size is returned as the _PC_REC_XFER_ALIGN and
  // _PC_ALLOC_SIZE_MIN pathconf() settings.
  // 4096 is commonly used by many filesystem types.
  info.frsize = 4096;

  // Ensure that namelen is set to a non-zero value.
  // The value we return here will be visible to programs that call pathconf()
  // with _PC_NAME_MAX.  Returning 0 will confuse programs that try to honor
  // this value.
  info.namelen = 255;

  return info;
}

folly::Future<folly::Unit> Dispatcher::setxattr(
    InodeNumber /*ino*/,
    folly::StringPiece /*name*/,
    folly::StringPiece /*value*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}

const int Dispatcher::kENOATTR =
#ifndef ENOATTR
    ENODATA // Linux
#else
    ENOATTR
#endif
    ;

folly::Future<std::string> Dispatcher::getxattr(
    InodeNumber /*ino*/,
    folly::StringPiece /*name*/) {
  throwSystemErrorExplicit(kENOATTR);
}

folly::Future<std::vector<std::string>> Dispatcher::listxattr(
    InodeNumber /*ino*/) {
  return std::vector<std::string>();
}

folly::Future<folly::Unit> Dispatcher::removexattr(
    InodeNumber /*ino*/,
    folly::StringPiece /*name*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::access(
    InodeNumber /*ino*/,
    int /*mask*/) {
  // Note that if you mount with the "default_permissions" kernel mount option,
  // the kernel will perform all permissions checks for you, and will never
  // invoke access() directly.
  //
  // Implementing access() is only needed when not using the
  // "default_permissions" option.
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::create(InodeNumber, PathComponentPiece, mode_t, int) {
  FUSELL_NOT_IMPL();
}

folly::Future<uint64_t>
Dispatcher::bmap(InodeNumber /*ino*/, size_t /*blocksize*/, uint64_t /*idx*/) {
  FUSELL_NOT_IMPL();
}

const fuse_init_out& Dispatcher::getConnInfo() const {
  return connInfo_;
}

EdenStats* Dispatcher::getStats() const {
  return stats_;
}

} // namespace eden
} // namespace facebook
