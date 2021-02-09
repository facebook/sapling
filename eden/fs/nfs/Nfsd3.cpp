/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/Nfsd3.h"

#include <folly/Utility.h>
#include <folly/futures/Future.h>
#include "eden/fs/nfs/NfsdRpc.h"

namespace facebook::eden {

namespace {
class Nfsd3ServerProcessor final : public RpcServerProcessor {
 public:
  Nfsd3ServerProcessor() = default;

  Nfsd3ServerProcessor(const Nfsd3ServerProcessor&) = delete;
  Nfsd3ServerProcessor(Nfsd3ServerProcessor&&) = delete;
  Nfsd3ServerProcessor& operator=(const Nfsd3ServerProcessor&) = delete;
  Nfsd3ServerProcessor& operator=(Nfsd3ServerProcessor&&) = delete;

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
  getattr(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  setattr(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  lookup(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  access(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  readlink(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  read(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  write(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  create(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  mkdir(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  symlink(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  mknod(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  remove(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  rmdir(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  rename(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  link(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  readdir(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  readdirplus(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  fsstat(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  fsinfo(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  pathconf(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  commit(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
};

folly::Future<folly::Unit> Nfsd3ServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::getattr(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::setattr(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::lookup(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::access(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::readlink(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::read(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::write(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::create(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::mkdir(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::symlink(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::mknod(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::remove(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::rmdir(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::rename(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::link(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::readdir(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::readdirplus(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::fsstat(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::fsinfo(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::pathconf(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::commit(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

using Handler = folly::Future<folly::Unit> (Nfsd3ServerProcessor::*)(
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

constexpr auto kNfs3dHandlers = [] {
  std::array<HandlerEntry, 22> handlers;
  handlers[folly::to_underlying(nfsv3Procs::null)] = {
      "NULL", &Nfsd3ServerProcessor::null};
  handlers[folly::to_underlying(nfsv3Procs::getattr)] = {
      "GETATTR", &Nfsd3ServerProcessor::getattr};
  handlers[folly::to_underlying(nfsv3Procs::setattr)] = {
      "SETATTR", &Nfsd3ServerProcessor::setattr};
  handlers[folly::to_underlying(nfsv3Procs::lookup)] = {
      "LOOKUP", &Nfsd3ServerProcessor::lookup};
  handlers[folly::to_underlying(nfsv3Procs::access)] = {
      "ACCESS", &Nfsd3ServerProcessor::access};
  handlers[folly::to_underlying(nfsv3Procs::readlink)] = {
      "READLINK", &Nfsd3ServerProcessor::readlink};
  handlers[folly::to_underlying(nfsv3Procs::read)] = {
      "READ", &Nfsd3ServerProcessor::read};
  handlers[folly::to_underlying(nfsv3Procs::write)] = {
      "WRITE", &Nfsd3ServerProcessor::write};
  handlers[folly::to_underlying(nfsv3Procs::create)] = {
      "CREATE", &Nfsd3ServerProcessor::create};
  handlers[folly::to_underlying(nfsv3Procs::mkdir)] = {
      "MKDIR", &Nfsd3ServerProcessor::mkdir};
  handlers[folly::to_underlying(nfsv3Procs::symlink)] = {
      "SYMLINK", &Nfsd3ServerProcessor::symlink};
  handlers[folly::to_underlying(nfsv3Procs::mknod)] = {
      "MKNOD", &Nfsd3ServerProcessor::mknod};
  handlers[folly::to_underlying(nfsv3Procs::remove)] = {
      "REMOVE", &Nfsd3ServerProcessor::remove};
  handlers[folly::to_underlying(nfsv3Procs::rmdir)] = {
      "RMDIR", &Nfsd3ServerProcessor::rmdir};
  handlers[folly::to_underlying(nfsv3Procs::rename)] = {
      "RENAME", &Nfsd3ServerProcessor::rename};
  handlers[folly::to_underlying(nfsv3Procs::link)] = {
      "LINK", &Nfsd3ServerProcessor::link};
  handlers[folly::to_underlying(nfsv3Procs::readdir)] = {
      "READDIR", &Nfsd3ServerProcessor::readdir};
  handlers[folly::to_underlying(nfsv3Procs::readdirplus)] = {
      "READDIRPLUS", &Nfsd3ServerProcessor::readdirplus};
  handlers[folly::to_underlying(nfsv3Procs::fsstat)] = {
      "FSSTAT", &Nfsd3ServerProcessor::fsstat};
  handlers[folly::to_underlying(nfsv3Procs::fsinfo)] = {
      "FSINFO", &Nfsd3ServerProcessor::fsinfo};
  handlers[folly::to_underlying(nfsv3Procs::pathconf)] = {
      "PATHCONF", &Nfsd3ServerProcessor::pathconf};
  handlers[folly::to_underlying(nfsv3Procs::commit)] = {
      "COMMIT", &Nfsd3ServerProcessor::commit};

  return handlers;
}();

folly::Future<folly::Unit> Nfsd3ServerProcessor::dispatchRpc(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid,
    uint32_t progNumber,
    uint32_t progVersion,
    uint32_t procNumber) {
  if (progNumber != kNfsdProgNumber) {
    serializeReply(ser, accept_stat::PROG_UNAVAIL, xid);
    return folly::unit;
  }

  if (progVersion != kNfsd3ProgVersion) {
    serializeReply(ser, accept_stat::PROG_MISMATCH, xid);
    XdrTrait<mismatch_info>::serialize(
        ser, mismatch_info{kNfsd3ProgVersion, kNfsd3ProgVersion});
    return folly::unit;
  }

  if (procNumber >= kNfs3dHandlers.size()) {
    XLOG(ERR) << "Invalid procedure: " << procNumber;
    serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
    return folly::unit;
  }

  auto handlerEntry = kNfs3dHandlers[procNumber];
  XLOG(DBG7) << handlerEntry.name;
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}
} // namespace

Nfsd3::Nfsd3(bool registerWithRpcbind, folly::EventBase* evb)
    : server_(std::make_shared<Nfsd3ServerProcessor>(), evb) {
  if (registerWithRpcbind) {
    server_.registerService(kNfsdProgNumber, kNfsd3ProgVersion);
  }
}
} // namespace facebook::eden

#endif
