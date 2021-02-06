/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/Mountd.h"

#include <memory>
#include <unordered_map>

#include <folly/Synchronized.h>
#include <folly/Utility.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include "eden/fs/nfs/MountdRpc.h"

namespace facebook::eden {

class MountdServerProcessor final : public RpcServerProcessor {
 public:
  MountdServerProcessor() = default;

  MountdServerProcessor(const MountdServerProcessor&) = delete;
  MountdServerProcessor(MountdServerProcessor&&) = delete;
  MountdServerProcessor& operator=(const MountdServerProcessor&) = delete;
  MountdServerProcessor& operator=(MountdServerProcessor&&) = delete;

  folly::Future<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::Appender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber) override;

  folly::Future<folly::Unit>
  null(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  mount(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  dump(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  umount(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  umountAll(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  exprt(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);

  void registerMount(AbsolutePathPiece path, InodeNumber rootIno);
  void unregisterMount(AbsolutePathPiece path);

 private:
  folly::Synchronized<std::unordered_map<AbsolutePath, InodeNumber>>
      mountPoints_;
};

namespace {

using Handler = folly::Future<folly::Unit> (MountdServerProcessor::*)(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid);

struct HandlerEntry {
  constexpr HandlerEntry() = default;
  constexpr HandlerEntry(folly::StringPiece n, Handler h)
      : name(n), handler(h) {}

  folly::StringPiece name;
  Handler handler = nullptr;
};

constexpr auto kMountHandlers = [] {
  std::array<HandlerEntry, 6> handlers;
  handlers[folly::to_underlying(mountProcs::null)] = {
      "NULL", &MountdServerProcessor::null};
  handlers[folly::to_underlying(mountProcs::mnt)] = {
      "MNT", &MountdServerProcessor::mount};
  handlers[folly::to_underlying(mountProcs::dump)] = {
      "DUMP", &MountdServerProcessor::dump};
  handlers[folly::to_underlying(mountProcs::umnt)] = {
      "UMOUNT", &MountdServerProcessor::umount};
  handlers[folly::to_underlying(mountProcs::umntAll)] = {
      "UMOUNTALL", &MountdServerProcessor::umountAll};
  handlers[folly::to_underlying(mountProcs::exprt)] = {
      "EXPORT", &MountdServerProcessor::exprt};

  return handlers;
}();

} // namespace

folly::Future<folly::Unit> MountdServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::mount(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  AbsolutePath path{XdrTrait<std::string>::deserialize(deser)};
  XLOG(DBG7) << "Mounting: " << path;

  auto mounts = mountPoints_.rlock();
  auto found = mounts->find(path);
  if (found != mounts->end()) {
    XdrTrait<mountstat3>::serialize(ser, mountstat3::MNT3_OK);
    XdrTrait<mountres3_ok>::serialize(
        ser, mountres3_ok{{found->second}, {auth_flavor::AUTH_UNIX}});
  } else {
    XdrTrait<mountstat3>::serialize(ser, mountstat3::MNT3ERR_NOENT);
  }
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dump(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umount(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umountAll(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::exprt(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  /*
   * In theory, we're supposed to return a list of exported FS, but since
   * EdenFS is not intended to be exposed as a generic NFS server, properly
   * answering with the list of exported FS isn't necessary. For now we can
   * just pretend that we don't export anything.
   *
   * When using libnfs, this may be called during mount to recursively mount
   * nested NFS mounts.
   */
  XdrTrait<bool>::serialize(ser, false);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dispatchRpc(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid,
    uint32_t progNumber,
    uint32_t progVersion,
    uint32_t procNumber) {
  if (progNumber != kMountdProgNumber) {
    serializeReply(ser, accept_stat::PROG_UNAVAIL, xid);
    return folly::unit;
  }

  if (progVersion != kMountdProgVersion) {
    serializeReply(ser, accept_stat::PROG_MISMATCH, xid);
    XdrTrait<mismatch_info>::serialize(
        ser, mismatch_info{kMountdProgVersion, kMountdProgVersion});
    return folly::unit;
  }

  if (procNumber >= kMountHandlers.size()) {
    XLOG(ERR) << "Invalid procedure: " << procNumber;
    serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
    return folly::unit;
  }

  auto handlerEntry = kMountHandlers[procNumber];

  XLOG(DBG7) << handlerEntry.name;
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}

void MountdServerProcessor::registerMount(
    AbsolutePathPiece path,
    InodeNumber ino) {
  auto map = mountPoints_.wlock();
  auto [iter, inserted] = map->emplace(path.copy(), ino);
  XCHECK_EQ(inserted, true);
}

void MountdServerProcessor::unregisterMount(AbsolutePathPiece path) {
  auto map = mountPoints_.wlock();
  auto numRemoved = map->erase(path.copy());
  XCHECK_EQ(numRemoved, 1u);
}

Mountd::Mountd(bool registerWithRpcbind)
    : proc_(std::make_shared<MountdServerProcessor>()), server_(proc_) {
  if (registerWithRpcbind) {
    server_.registerService(kMountdProgNumber, kMountdProgVersion);
  }
}

void Mountd::registerMount(AbsolutePathPiece path, InodeNumber ino) {
  proc_->registerMount(path, ino);
}

void Mountd::unregisterMount(AbsolutePathPiece path) {
  proc_->unregisterMount(path);
}

} // namespace facebook::eden

#endif
