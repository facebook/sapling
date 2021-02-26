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
#include "eden/fs/utils/SystemError.h"

namespace facebook::eden {

namespace {
class Nfsd3ServerProcessor final : public RpcServerProcessor {
 public:
  explicit Nfsd3ServerProcessor(
      std::unique_ptr<NfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      bool caseSensitive)
      : dispatcher_(std::move(dispatcher)),
        straceLogger_(straceLogger),
        caseSensitive_(caseSensitive) {}

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

 private:
  std::unique_ptr<NfsDispatcher> dispatcher_;
  const folly::Logger* straceLogger_;
  bool caseSensitive_;
};

/**
 * Convert a exception to the appropriate NFS error value.
 */
nfsstat3 exceptionToNfsError(const folly::exception_wrapper& ex) {
  if (auto* err = ex.get_exception<std::system_error>()) {
    if (!isErrnoError(*err)) {
      return nfsstat3::NFS3ERR_SERVERFAULT;
    }

    switch (err->code().value()) {
      case EPERM:
        return nfsstat3::NFS3ERR_PERM;
      case ENOENT:
        return nfsstat3::NFS3ERR_NOENT;
      case EIO:
      case ETXTBSY:
        return nfsstat3::NFS3ERR_IO;
      case ENXIO:
        return nfsstat3::NFS3ERR_NXIO;
      case EACCES:
        return nfsstat3::NFS3ERR_ACCES;
      case EEXIST:
        return nfsstat3::NFS3ERR_EXIST;
      case EXDEV:
        return nfsstat3::NFS3ERR_XDEV;
      case ENODEV:
        return nfsstat3::NFS3ERR_NODEV;
      case ENOTDIR:
        return nfsstat3::NFS3ERR_NOTDIR;
      case EISDIR:
        return nfsstat3::NFS3ERR_ISDIR;
      case EINVAL:
        return nfsstat3::NFS3ERR_INVAL;
      case EFBIG:
        return nfsstat3::NFS3ERR_FBIG;
      case EROFS:
        return nfsstat3::NFS3ERR_ROFS;
      case EMLINK:
        return nfsstat3::NFS3ERR_MLINK;
      case ENAMETOOLONG:
        return nfsstat3::NFS3ERR_NAMETOOLONG;
      case ENOTEMPTY:
        return nfsstat3::NFS3ERR_NOTEMPTY;
      case EDQUOT:
        return nfsstat3::NFS3ERR_DQUOT;
      case ESTALE:
        return nfsstat3::NFS3ERR_STALE;
      case ETIMEDOUT:
      case EAGAIN:
      case ENOMEM:
        return nfsstat3::NFS3ERR_JUKEBOX;
      case ENOTSUP:
        return nfsstat3::NFS3ERR_NOTSUPP;
      case ENFILE:
        return nfsstat3::NFS3ERR_SERVERFAULT;
    }
    return nfsstat3::NFS3ERR_SERVERFAULT;
  } else if (ex.get_exception<folly::FutureTimeout>()) {
    return nfsstat3::NFS3ERR_JUKEBOX;
  } else {
    return nfsstat3::NFS3ERR_SERVERFAULT;
  }
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

/**
 * Convert the POSIX mode to NFS file type.
 */
ftype3 modeToFtype3(mode_t mode) {
  if (S_ISREG(mode)) {
    return ftype3::NF3REG;
  } else if (S_ISDIR(mode)) {
    return ftype3::NF3DIR;
  } else if (S_ISBLK(mode)) {
    return ftype3::NF3BLK;
  } else if (S_ISCHR(mode)) {
    return ftype3::NF3CHR;
  } else if (S_ISLNK(mode)) {
    return ftype3::NF3LNK;
  } else if (S_ISSOCK(mode)) {
    return ftype3::NF3SOCK;
  } else {
    XDCHECK(S_ISFIFO(mode));
    return ftype3::NF3FIFO;
  }
}

/**
 * Convert the POSIX mode to NFS mode.
 *
 * TODO(xavierd): For now, the owner always has RW access, the group R access
 * and others no access.
 */
uint32_t modeToNfsMode(mode_t mode) {
  return kReadOwnerBit | kWriteOwnerBit | kReadGroupBit |
      ((mode & S_IXUSR) ? kExecOwnerBit : 0);
}

/**
 * Convert a POSIX timespec to an NFS time.
 */
nfstime3 timespecToNfsTime(const struct timespec& time) {
  return nfstime3{
      folly::to_narrow(folly::to_unsigned(time.tv_sec)),
      folly::to_narrow(folly::to_unsigned(time.tv_nsec))};
}

fattr3 statToFattr3(const struct stat& stat) {
  return fattr3{
      /*type*/ modeToFtype3(stat.st_mode),
      /*mode*/ modeToNfsMode(stat.st_mode),
      /*nlink*/ folly::to_narrow(stat.st_nlink),
      /*uid*/ stat.st_uid,
      /*gid*/ stat.st_gid,
      /*size*/ folly::to_unsigned(stat.st_size),
      /*used*/ folly::to_unsigned(stat.st_blocks) * 512u,
      /*rdev*/ specdata3{0, 0}, // TODO(xavierd)
      /*fsid*/ folly::to_unsigned(stat.st_dev),
      /*fileid*/ stat.st_ino,
#ifdef __linux__
      /*atime*/ timespecToNfsTime(stat.st_atim),
      /*mtime*/ timespecToNfsTime(stat.st_mtim),
      /*ctime*/ timespecToNfsTime(stat.st_ctim),
#else
      /*atime*/ timespecToNfsTime(stat.st_atimespec),
      /*mtime*/ timespecToNfsTime(stat.st_mtimespec),
      /*ctime*/ timespecToNfsTime(stat.st_ctimespec),
#endif
  };
}

post_op_attr statToPostOpAttr(folly::Try<struct stat>&& stat) {
  if (stat.hasException()) {
    return post_op_attr{{false, std::monostate{}}};
  } else {
    return post_op_attr{{true, statToFattr3(stat.value())}};
  }
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::getattr(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<GETATTR3args>::deserialize(deser);

  // TODO(xavierd): make an NfsRequestContext.
  static auto context =
      ObjectFetchContext::getNullContextWithCauseDetail("getattr");

  return dispatcher_->getattr(args.object.ino, *context)
      .thenTry([ser = std::move(ser)](folly::Try<struct stat>&& try_) mutable {
        if (try_.hasException()) {
          GETATTR3res res{
              {exceptionToNfsError(try_.exception()), std::monostate{}}};
          XdrTrait<GETATTR3res>::serialize(ser, res);
        } else {
          auto stat = std::move(try_).value();

          GETATTR3res res{
              {nfsstat3::NFS3_OK, GETATTR3resok{statToFattr3(stat)}}};
          XdrTrait<GETATTR3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::setattr(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::lookup(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<LOOKUP3args>::deserialize(deser);

  // TODO(xavierd): make an NfsRequestContext.
  static auto context =
      ObjectFetchContext::getNullContextWithCauseDetail("lookup");

  // TODO(xavierd): the lifetime of this future is a bit tricky and it needs to
  // be consumed in this function to avoid use-after-free. This future may also
  // need to be executed after the lookup call to conform to fill the "post-op"
  // attributes
  auto dirAttrFut = dispatcher_->getattr(args.what.dir.ino, *context);

  if (args.what.name.length() > NAME_MAX) {
    // The filename is too long, let's try to get the attributes of the
    // directory and fail.
    return std::move(dirAttrFut)
        .thenTry(
            [ser = std::move(ser)](folly::Try<struct stat>&& try_) mutable {
              if (try_.hasException()) {
                LOOKUP3res res{
                    {nfsstat3::NFS3ERR_NAMETOOLONG,
                     LOOKUP3resfail{post_op_attr{{false, std::monostate{}}}}}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              } else {
                LOOKUP3res res{
                    {nfsstat3::NFS3ERR_NAMETOOLONG,
                     LOOKUP3resfail{
                         post_op_attr{{true, statToFattr3(try_.value())}}}}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              }

              return folly::unit;
            });
  }

  return folly::makeFutureWith([this, args = std::move(args)]() mutable {
           if (args.what.name == ".") {
             return dispatcher_->getattr(args.what.dir.ino, *context)
                 .thenValue(
                     [ino = args.what.dir.ino](struct stat && stat)
                         -> std::tuple<InodeNumber, struct stat> {
                       return {ino, std::move(stat)};
                     });
           } else if (args.what.name == "..") {
             return dispatcher_->getParent(args.what.dir.ino, *context)
                 .thenValue([this](InodeNumber ino) {
                   return dispatcher_->getattr(ino, *context)
                       .thenValue(
                           [ino](struct stat && stat)
                               -> std::tuple<InodeNumber, struct stat> {
                             return {ino, std::move(stat)};
                           });
                 });
           } else {
             return dispatcher_->lookup(
                 args.what.dir.ino, PathComponent(args.what.name), *context);
           }
         })
      .thenTry([ser = std::move(ser), dirAttrFut = std::move(dirAttrFut)](
                   folly::Try<std::tuple<InodeNumber, struct stat>>&&
                       lookupTry) mutable {
        return std::move(dirAttrFut)
            .thenTry([ser = std::move(ser), lookupTry = std::move(lookupTry)](
                         folly::Try<struct stat>&& dirStat) mutable {
              if (lookupTry.hasException()) {
                LOOKUP3res res{
                    {exceptionToNfsError(lookupTry.exception()),
                     LOOKUP3resfail{statToPostOpAttr(std::move(dirStat))}}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              } else {
                auto& [ino, stat] = lookupTry.value();
                LOOKUP3res res{
                    {nfsstat3::NFS3_OK,
                     LOOKUP3resok{
                         /*object*/ nfs_fh3{ino},
                         /*obj_attributes*/
                         post_op_attr{{true, statToFattr3(stat)}},
                         /*dir_attributes*/
                         statToPostOpAttr(std::move(dirStat)),
                     }}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              }
              return folly::unit;
            });
      });
}

uint32_t getEffectiveAccessRights(
    const struct stat& /*stat*/,
    uint32_t desiredAccess) {
  // TODO(xavierd): we should look at the uid/gid of the user doing the
  // request. This should be part of the RPC credentials.
  return desiredAccess;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::access(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<ACCESS3args>::deserialize(deser);

  // TODO(xavierd): make an NfsRequestContext.
  static auto context =
      ObjectFetchContext::getNullContextWithCauseDetail("access");

  return dispatcher_->getattr(args.object.ino, *context)
      .thenTry([ser = std::move(ser), desiredAccess = args.access](
                   folly::Try<struct stat>&& try_) mutable {
        if (try_.hasException()) {
          ACCESS3res res{
              {exceptionToNfsError(try_.exception()),
               ACCESS3resfail{post_op_attr{{false, std::monostate{}}}}}};
          XdrTrait<ACCESS3res>::serialize(ser, res);
        } else {
          auto stat = std::move(try_).value();

          ACCESS3res res{
              {nfsstat3::NFS3_OK,
               ACCESS3resok{
                   post_op_attr{{true, statToFattr3(stat)}},
                   /*access*/ getEffectiveAccessRights(stat, desiredAccess),
               }}};
          XdrTrait<ACCESS3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::readlink(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<READLINK3args>::deserialize(deser);

  static auto context =
      ObjectFetchContext::getNullContextWithCauseDetail("readlink");

  auto getattr = dispatcher_->getattr(args.symlink.ino, *context);
  return dispatcher_->readlink(args.symlink.ino, *context)
      .thenTry([ser = std::move(ser), getattr = std::move(getattr)](
                   folly::Try<std::string> tryReadlink) mutable {
        return std::move(getattr).thenTry(
            [ser = std::move(ser), tryReadlink = std::move(tryReadlink)](
                folly::Try<struct stat> tryAttr) mutable {
              if (tryReadlink.hasException()) {
                READLINK3res res{
                    {exceptionToNfsError(tryReadlink.exception()),
                     READLINK3resfail{statToPostOpAttr(std::move(tryAttr))}}};
                XdrTrait<READLINK3res>::serialize(ser, res);
              } else {
                auto link = std::move(tryReadlink).value();

                READLINK3res res{
                    {nfsstat3::NFS3_OK,
                     READLINK3resok{
                         /*symlink_attributes*/ statToPostOpAttr(
                             std::move(tryAttr)),
                         /*data*/ std::move(link),
                     }}};
                XdrTrait<READLINK3res>::serialize(ser, res);
              }

              return folly::unit;
            });
      });
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
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<FSINFO3args>::deserialize(deser);
  (void)args;

  FSINFO3res res{
      {nfsstat3::NFS3_OK,
       FSINFO3resok{
           // TODO(xavierd): fill the post_op_attr and check the values chosen
           // randomly below.
           post_op_attr{},
           /*rtmax=*/1024 * 1024,
           /*rtpref=*/1024 * 1024,
           /*rtmult=*/1,
           /*wtmax=*/1024 * 1024,
           /*wtpref=*/1024 * 1024,
           /*wtmult=*/1,
           /*dtpref=*/1024 * 1024,
           /*maxfilesize=*/std::numeric_limits<uint64_t>::max(),
           nfstime3{0, 1},
           /*properties*/ FSF3_SYMLINK | FSF3_HOMOGENEOUS | FSF3_CANSETTIME,
       }}};

  XdrTrait<FSINFO3res>::serialize(ser, res);

  return folly::unit;
}

folly::Future<folly::Unit> Nfsd3ServerProcessor::pathconf(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<PATHCONF3args>::deserialize(deser);
  (void)args;

  PATHCONF3res res{
      {nfsstat3::NFS3_OK,
       PATHCONF3resok{
           // TODO(xavierd): fill up the post_op_attr
           post_op_attr{},
           /*linkmax=*/0,
           /*name_max=*/NAME_MAX,
           /*no_trunc=*/true,
           /*chown_restricted=*/true,
           /*case_insensitive=*/!caseSensitive_,
           /*case_preserving=*/true,
       }}};

  XdrTrait<PATHCONF3res>::serialize(ser, res);

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
  // TODO(xavierd): log the arguments too.
  FB_LOGF(*straceLogger_, DBG7, "{}()", handlerEntry.name);
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}
} // namespace

Nfsd3::Nfsd3(
    bool registerWithRpcbind,
    folly::EventBase* evb,
    std::unique_ptr<NfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> /*processNameCache*/,
    folly::Duration /*requestTimeout*/,
    Notifications* /*notifications*/,
    bool caseSensitive)
    : server_(
          std::make_shared<Nfsd3ServerProcessor>(
              std::move(dispatcher),
              straceLogger,
              caseSensitive),
          evb) {
  if (registerWithRpcbind) {
    server_.registerService(kNfsdProgNumber, kNfsd3ProgVersion);
  }
}

Nfsd3::~Nfsd3() {
  // TODO(xavierd): wait for the pending requests, and the sockets being tore
  // down
  stopPromise_.setValue(Nfsd3::StopData{});
}

folly::SemiFuture<Nfsd3::StopData> Nfsd3::getStopFuture() {
  return stopPromise_.getSemiFuture();
}

} // namespace facebook::eden

#endif
