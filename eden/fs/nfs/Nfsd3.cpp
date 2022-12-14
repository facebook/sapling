/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32
#include "eden/fs/nfs/Nfsd3.h"

#include <memory>

#include <folly/Utility.h>
#include <folly/executors/SerialExecutor.h>
#include <folly/futures/Future.h>

#include "eden/fs/nfs/NfsRequestContext.h"
#include "eden/fs/nfs/NfsUtils.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Server.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/FsEventLogger.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/IDGen.h"
#include "eden/fs/utils/StaticAssert.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/Throw.h"

#ifndef __APPLE__
#include <sys/sysmacros.h>
#endif

namespace facebook::eden {

namespace {
static_assert(CheckSize<NfsTraceEvent, 40>());

class Nfsd3ServerProcessor final : public RpcServerProcessor {
 public:
  explicit Nfsd3ServerProcessor(
      std::unique_ptr<NfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      const std::shared_ptr<StructuredLogger>& structuredLogger,
      CaseSensitivity caseSensitive,
      uint32_t iosize,
      folly::Promise<Nfsd3::StopData>& stopPromise,
      ProcessAccessLog& processAccessLog,
      std::atomic<size_t>& traceDetailedArguments,
      std::shared_ptr<TraceBus<NfsTraceEvent>>& traceBus)
      : dispatcher_(std::move(dispatcher)),
        straceLogger_(straceLogger),
        structuredLogger_(structuredLogger),
        caseSensitive_(caseSensitive),
        iosize_(iosize),
        stopPromise_{stopPromise},
        processAccessLog_{processAccessLog},
        traceDetailedArguments_(traceDetailedArguments),
        metadataSizeMismatchLogged_(false),
        traceBus_(traceBus) {}

  Nfsd3ServerProcessor(const Nfsd3ServerProcessor&) = delete;
  Nfsd3ServerProcessor(Nfsd3ServerProcessor&&) = delete;
  Nfsd3ServerProcessor& operator=(const Nfsd3ServerProcessor&) = delete;
  Nfsd3ServerProcessor& operator=(Nfsd3ServerProcessor&&) = delete;

  ImmediateFuture<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber) override;

  void onShutdown(RpcStopData stopData) override;
  void clientConnected() override;

  ImmediateFuture<folly::Unit> null(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> getattr(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> setattr(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> lookup(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> access(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> readlink(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> read(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> write(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> create(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> mkdir(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> symlink(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> mknod(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> remove(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> rmdir(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> rename(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> link(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> readdir(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> readdirplus(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> fsstat(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> fsinfo(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> pathconf(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);
  ImmediateFuture<folly::Unit> commit(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      NfsRequestContext& context);

 private:
  std::unique_ptr<NfsDispatcher> dispatcher_;
  // Logger that is used to observe NFS procedure calls in the EdenFS daemon.
  // All events are published here when we are in stace mode. This is a local
  // logger, the events are not logged anywhere outside of the machine this
  // EdenFS instance runs on.
  const folly::Logger* straceLogger_;
  const std::shared_ptr<StructuredLogger> structuredLogger_;
  CaseSensitivity caseSensitive_;
  uint32_t iosize_;
  // This promise is owned by the nfs3d. The nfs3d owns an RPC server that owns
  // this server processor. This promise should only be used during the
  // lifetime of  nfs3d. The way we currently enforce this is by waiting for
  // this promise to be set before destroying of the nfs3d.
  folly::Promise<Nfsd3::StopData>& stopPromise_;
  ProcessAccessLog& processAccessLog_;
  std::atomic_int32_t numberOfClients_;
  std::atomic<size_t>& traceDetailedArguments_;
  // TODO(T136422586): Remove once we've identified the cause of mismatched file
  // size metadata.
  std::atomic_bool metadataSizeMismatchLogged_;
  std::shared_ptr<TraceBus<NfsTraceEvent>>& traceBus_;
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

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  return folly::unit;
}

/**
 * Convert the given string onto a PathComponent.
 *
 * Any raised exception that constructing the PathComponent may raise will be
 * captured by the returned ImmediateFuture.
 */
ImmediateFuture<PathComponent> extractPathComponent(std::string str) {
  return makeImmediateFutureWith([&]() {
    try {
      return PathComponent{str};
    } catch (const PathComponentNotUtf8& ex) {
      throw std::system_error(EINVAL, std::system_category(), ex.what());
    }
  });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::getattr(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());

  auto args = XdrTrait<GETATTR3args>::deserialize(deser);

  return dispatcher_->getattr(args.object.ino, context.getObjectFetchContext())
      .thenTry(
          [ser = std::move(ser)](const folly::Try<struct stat>& try_) mutable {
            if (try_.hasException()) {
              GETATTR3res res{
                  {{exceptionToNfsError(try_.exception()), std::monostate{}}}};
              XdrTrait<GETATTR3res>::serialize(ser, res);
            } else {
              const auto& stat = try_.value();

              GETATTR3res res{
                  {{nfsstat3::NFS3_OK, GETATTR3resok{statToFattr3(stat)}}}};
              XdrTrait<GETATTR3res>::serialize(ser, res);
            }

            return folly::unit;
          });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::setattr(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());

  auto args = XdrTrait<SETATTR3args>::deserialize(deser);

  if (args.guard.tag) {
    // TODO(xavierd): we probably need to support this.
    XLOG(WARN) << "Guarded setattr aren't supported.";
    SETATTR3res res{{{nfsstat3::NFS3ERR_INVAL, SETATTR3resfail{}}}};
    XdrTrait<SETATTR3res>::serialize(ser, res);
    return folly::unit;
  }

  auto size = args.new_attributes.size.tag
      ? std::optional(std::get<uint64_t>(args.new_attributes.size.v))
      : std::nullopt;
  auto mode = args.new_attributes.mode.tag
      ? std::optional(std::get<uint32_t>(args.new_attributes.mode.v))
      : std::nullopt;
  auto uid = args.new_attributes.uid.tag
      ? std::optional(std::get<uint32_t>(args.new_attributes.uid.v))
      : std::nullopt;
  auto gid = args.new_attributes.gid.tag
      ? std::optional(std::get<uint32_t>(args.new_attributes.gid.v))
      : std::nullopt;

  auto makeTimespec = [this](auto& time) -> std::optional<struct timespec> {
    switch (time.tag) {
      case time_how::SET_TO_CLIENT_TIME:
        return std::optional(nfsTimeToTimespec(std::get<nfstime3>(time.v)));
      case time_how::SET_TO_SERVER_TIME:
        return std::optional(dispatcher_->getClock().getRealtime());
      default:
        return std::nullopt;
    }
  };

  DesiredMetadata desired{
      /*size*/ size,
      /*mode*/ mode,
      /*uid*/ uid,
      /*gid*/ gid,
      /*atime*/ makeTimespec(args.new_attributes.atime),
      /*mtime*/ makeTimespec(args.new_attributes.mtime),
  };

  if (desired.is_nop(true /* ignoreAtime */)) {
    // EdenFS does not support `atime`, so ignore `atime`-only changes.
    //
    // Ignoring `atime` is not strictly necessary to work around bugs
    // on macOS ARM64 with nop changes since `atime` is empty but let's be safe.
    XLOG(DBG7) << "Skipping nop setattr with ignoring `atime`";
    return folly::unit;
  }

  return dispatcher_
      ->setattr(args.object.ino, desired, context.getObjectFetchContext())
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::SetattrRes>&& try_) mutable {
        if (try_.hasException()) {
          SETATTR3res res{
              {{exceptionToNfsError(try_.exception()), SETATTR3resfail{}}}};
          XdrTrait<SETATTR3res>::serialize(ser, res);
        } else {
          const auto& setattrRes = try_.value();

          SETATTR3res res{
              {{nfsstat3::NFS3_OK,
                SETATTR3resok{
                    statToWccData(setattrRes.preStat, setattrRes.postStat)}}}};
          XdrTrait<SETATTR3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::lookup(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<LOOKUP3args>::deserialize(deser);

  // TODO(xavierd): the lifetime of this future is a bit tricky and it needs to
  // be consumed in this function to avoid use-after-free. This future may also
  // need to be executed after the lookup call to conform to fill the "post-op"
  // attributes
  auto dirAttrFut =
      dispatcher_->getattr(args.what.dir.ino, context.getObjectFetchContext());

  if (args.what.name.length() > NAME_MAX) {
    // The filename is too long, let's try to get the attributes of the
    // directory and fail.
    return std::move(dirAttrFut)
        .thenTry([ser = std::move(ser)](
                     const folly::Try<struct stat>& try_) mutable {
          if (try_.hasException()) {
            LOOKUP3res res{
                {{nfsstat3::NFS3ERR_NAMETOOLONG,
                  LOOKUP3resfail{post_op_attr{}}}}};
            XdrTrait<LOOKUP3res>::serialize(ser, res);
          } else {
            LOOKUP3res res{
                {{nfsstat3::NFS3ERR_NAMETOOLONG,
                  LOOKUP3resfail{post_op_attr{statToFattr3(try_.value())}}}}};
            XdrTrait<LOOKUP3res>::serialize(ser, res);
          }

          return folly::unit;
        });
  }

  return makeImmediateFutureWith([this, args = std::move(args), &context]() {
           if (args.what.name == ".") {
             return dispatcher_
                 ->getattr(args.what.dir.ino, context.getObjectFetchContext())
                 .thenValue(
                     [ino = args.what.dir.ino](struct stat && stat)
                         -> std::tuple<InodeNumber, struct stat> {
                       return {ino, std::move(stat)};
                     });
           } else if (args.what.name == "..") {
             return dispatcher_
                 ->getParent(args.what.dir.ino, context.getObjectFetchContext())
                 .thenValue([this, &context](InodeNumber ino) {
                   return dispatcher_
                       ->getattr(ino, context.getObjectFetchContext())
                       .thenValue(
                           [ino](struct stat && stat)
                               -> std::tuple<InodeNumber, struct stat> {
                             return {ino, std::move(stat)};
                           });
                 });
           } else {
             return extractPathComponent(std::move(args.what.name))
                 .thenValue([this, ino = args.what.dir.ino, &context](
                                PathComponent&& name) {
                   return dispatcher_->lookup(
                       ino, std::move(name), context.getObjectFetchContext());
                 });
           }
         })
      .thenTry([ser = std::move(ser), dirAttrFut = std::move(dirAttrFut)](
                   folly::Try<std::tuple<InodeNumber, struct stat>>&&
                       lookupTry) mutable {
        return std::move(dirAttrFut)
            .thenTry([ser = std::move(ser), lookupTry = std::move(lookupTry)](
                         const folly::Try<struct stat>& dirStat) mutable {
              if (lookupTry.hasException()) {
                LOOKUP3res res{
                    {{exceptionToNfsError(lookupTry.exception()),
                      LOOKUP3resfail{statToPostOpAttr(dirStat)}}}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              } else {
                const auto& [ino, stat] = lookupTry.value();
                LOOKUP3res res{
                    {{nfsstat3::NFS3_OK,
                      LOOKUP3resok{
                          /*object*/ nfs_fh3{ino},
                          /*obj_attributes*/
                          post_op_attr{statToFattr3(stat)},
                          /*dir_attributes*/
                          statToPostOpAttr(dirStat),
                      }}}};
                XdrTrait<LOOKUP3res>::serialize(ser, res);
              }
              return folly::unit;
            });
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::access(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());

  auto args = XdrTrait<ACCESS3args>::deserialize(deser);

  return dispatcher_->getattr(args.object.ino, context.getObjectFetchContext())
      .thenTry([ser = std::move(ser), desiredAccess = args.access](
                   folly::Try<struct stat>&& try_) mutable {
        if (try_.hasException()) {
          ACCESS3res res{
              {{exceptionToNfsError(try_.exception()),
                ACCESS3resfail{post_op_attr{}}}}};
          XdrTrait<ACCESS3res>::serialize(ser, res);
        } else {
          const auto& stat = try_.value();

          ACCESS3res res{
              {{nfsstat3::NFS3_OK,
                ACCESS3resok{
                    post_op_attr{statToFattr3(stat)},
                    /*access*/ getEffectiveAccessRights(stat, desiredAccess),
                }}}};
          XdrTrait<ACCESS3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::readlink(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<READLINK3args>::deserialize(deser);

  auto getattr =
      dispatcher_->getattr(args.symlink.ino, context.getObjectFetchContext());
  return dispatcher_
      ->readlink(args.symlink.ino, context.getObjectFetchContext())
      .thenTry([ser = std::move(ser), getattr = std::move(getattr)](
                   folly::Try<std::string> tryReadlink) mutable {
        return std::move(getattr).thenTry(
            [ser = std::move(ser), tryReadlink = std::move(tryReadlink)](
                const folly::Try<struct stat>& tryAttr) mutable {
              if (tryReadlink.hasException()) {
                READLINK3res res{
                    {{exceptionToNfsError(tryReadlink.exception()),
                      READLINK3resfail{statToPostOpAttr(tryAttr)}}}};
                XdrTrait<READLINK3res>::serialize(ser, res);
              } else {
                auto&& link = std::move(tryReadlink).value();

                READLINK3res res{
                    {{nfsstat3::NFS3_OK,
                      READLINK3resok{
                          /*symlink_attributes*/ statToPostOpAttr(tryAttr),
                          /*data*/ std::move(link),
                      }}}};
                XdrTrait<READLINK3res>::serialize(ser, res);
              }

              return folly::unit;
            });
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::read(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<READ3args>::deserialize(deser);

  return dispatcher_
      ->read(
          args.file.ino,
          args.count,
          args.offset,
          context.getObjectFetchContext())
      .thenTry([this, ser = std::move(ser), ino = args.file.ino, &context](
                   folly::Try<NfsDispatcher::ReadRes> tryRead) mutable {
        return dispatcher_->getattr(ino, context.getObjectFetchContext())
            .thenTry([this,
                      ser = std::move(ser),
                      tryRead = std::move(tryRead),
                      ino](const folly::Try<struct stat>& tryStat) mutable {
              if (tryRead.hasException()) {
                READ3res res{
                    {{exceptionToNfsError(tryRead.exception()),
                      READ3resfail{statToPostOpAttr(tryStat)}}}};
                XdrTrait<READ3res>::serialize(ser, res);
              } else {
                auto& read = tryRead.value();
                auto length = read.data->computeChainDataLength();

                if (UNLIKELY(
                        tryStat.hasValue() &&
                        length > folly::to_unsigned(tryStat.value().st_size) &&
                        !this->metadataSizeMismatchLogged_.exchange(true))) {
                  XLOG(
                      ERR,
                      fmt::format(
                          "Mismatch in blob size and cached size for inode {} ! "
                          "content chunk size {} greater than file size {}.",
                          ino,
                          length,
                          tryStat.value().st_size));

                  this->structuredLogger_->logEvent(
                      MetadataSizeMismatch{"NFS", "read"});
                }

                if (UNLIKELY(tryStat.hasException())) {
                  XLOG(
                      WARN,
                      fmt::format(
                          "getattr error during NFSv3 read: {}",
                          exceptionStr(tryStat.exception())));
                }

                // Make sure that we haven't read more than what we can encode.
                XDCHECK_LE(
                    length, size_t{std::numeric_limits<uint32_t>::max()});

                READ3res res{
                    {{nfsstat3::NFS3_OK,
                      READ3resok{
                          /*file_attributes*/ statToPostOpAttr(tryStat),
                          /*count*/ folly::to_narrow(length),
                          /*eof*/ read.isEof,
                          /*data*/ std::move(read.data),
                      }}}};
                XdrTrait<READ3res>::serialize(ser, res);
              }
              return folly::unit;
            });
      });
}

/**
 * Generate a unique per-EdenFS instance write cookie.
 *
 * TODO(xavierd): Note that for now this will always be 0 as this is to handle
 * the case where the server restart while the client isn't aware.
 */
writeverf3 makeWriteVerf() {
  return 0;
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::write(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<WRITE3args>::deserialize(deser);

  // I have no idea why NFS sent us data that we shouldn't write to the file,
  // but here it is, let's only take up to count bytes from the data.
  auto queue = folly::IOBufQueue();
  queue.append(std::move(args.data));
  auto data = queue.split(args.count);

  return dispatcher_
      ->write(
          args.file.ino,
          std::move(data),
          args.offset,
          context.getObjectFetchContext())
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::WriteRes> writeTry) mutable {
        if (writeTry.hasException()) {
          WRITE3res res{
              {{exceptionToNfsError(writeTry.exception()), WRITE3resfail{}}}};
          XdrTrait<WRITE3res>::serialize(ser, res);
        } else {
          const auto& writeRes = writeTry.value();

          // NFS is limited to writing a maximum of 4GB (2^32) of data
          // per write call, so despite write returning a size_t, it
          // should always fit in a uint32_t.
          XDCHECK_LE(
              writeRes.written, size_t{std::numeric_limits<uint32_t>::max()});

          WRITE3res res{
              {{nfsstat3::NFS3_OK,
                WRITE3resok{
                    /*file_wcc*/ statToWccData(
                        writeRes.preStat, writeRes.postStat),
                    /*count*/ folly::to_narrow(writeRes.written),
                    // TODO(xavierd): the following is a total lie and we
                    // should call inode->fdatasync() in the case where
                    // args.stable is anything other than
                    // stable_how::UNSTABLE. For testing purpose, this is
                    // OK.
                    /*committed*/ stable_how::FILE_SYNC,
                    /*verf*/ makeWriteVerf(),
                }}}};
          XdrTrait<WRITE3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

/**
 * Test if the exception was raised due to a EEXIST condition.
 */
bool isEexist(const folly::exception_wrapper& ex) {
  if (auto* err = ex.get_exception<std::system_error>()) {
    return isErrnoError(*err) && err->code().value() == EEXIST;
  }
  return false;
}

/**
 * Convert a set_mode3 into a useable mode_t. When unset, the returned mode
 * will be writable by the owner, readable by the group and other. This is
 * consistent with creating a file with a default umask of 022.
 */
mode_t setMode3ToMode(const set_mode3& mode) {
  if (mode.tag) {
    return std::get<uint32_t>(mode.v);
  } else {
    return 0644;
  }
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::create(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<CREATE3args>::deserialize(deser);

  if (args.how.tag == createmode3::EXCLUSIVE) {
    // Exclusive file creation is complicated, for now let's not support it.
    CREATE3res res{{{nfsstat3::NFS3ERR_NOTSUPP, CREATE3resfail{wcc_data{}}}}};
    XdrTrait<CREATE3res>::serialize(ser, res);
    return folly::unit;
  }

  auto& attr = std::get<sattr3>(args.how.v);
  auto mode = S_IFREG | setMode3ToMode(attr.mode);

  return extractPathComponent(std::move(args.where.name))
      .thenValue([this, ino = args.where.dir.ino, mode, &context](
                     PathComponent&& name) {
        return dispatcher_->create(
            ino, std::move(name), mode, context.getObjectFetchContext());
      })
      .thenTry([ser = std::move(ser), createmode = args.how.tag](
                   folly::Try<NfsDispatcher::CreateRes> try_) mutable {
        if (try_.hasException()) {
          if (createmode == createmode3::UNCHECKED &&
              isEexist(try_.exception())) {
            XLOG(WARN) << "Unchecked file creation returned EEXIST";
            // A file already exist at that location, since this is an
            // UNCHECKED creation, just pretend the file was created just fine.
            // Since no fields are populated, this forces the client to issue a
            // LOOKUP RPC to gather the InodeNumber and attributes for this
            // file. This is probably fine as creating a file that already
            // exists should be a rare event.
            // TODO(xavierd): We should change the file attributes based on
            // the requested args.how.obj_attributes.
            CREATE3res res{
                {{nfsstat3::NFS3_OK,
                  CREATE3resok{
                      /*obj*/ post_op_fh3{},
                      /*obj_attributes*/ post_op_attr{},
                      wcc_data{
                          /*before*/ pre_op_attr{},
                          /*after*/ post_op_attr{},
                      }}}}};
            XdrTrait<CREATE3res>::serialize(ser, res);
          } else {
            CREATE3res res{
                {{exceptionToNfsError(try_.exception()), CREATE3resfail{}}}};
            XdrTrait<CREATE3res>::serialize(ser, res);
          }
        } else {
          const auto& createRes = try_.value();

          CREATE3res res{
              {{nfsstat3::NFS3_OK,
                CREATE3resok{
                    /*obj*/ post_op_fh3{nfs_fh3{createRes.ino}},
                    /*obj_attributes*/
                    post_op_attr{statToFattr3(createRes.stat)},
                    /*dir_wcc*/
                    statToWccData(createRes.preDirStat, createRes.postDirStat),
                }}}};
          XdrTrait<CREATE3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::mkdir(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<MKDIR3args>::deserialize(deser);

  // Don't allow creating this directory and its parent.
  if (args.where.name == "." || args.where.name == "..") {
    MKDIR3res res{{{nfsstat3::NFS3ERR_EXIST, MKDIR3resfail{}}}};
    XdrTrait<MKDIR3res>::serialize(ser, res);
    return folly::unit;
  }

  // If the mode isn't set, make it writable by the owner, readable by the
  // group and traversable by other.
  auto mode = args.attributes.mode.tag
      ? std::get<uint32_t>(args.attributes.mode.v)
      : (S_IFDIR | 0751);

  // TODO(xavierd): For now, all the other args.attributes are ignored, is it
  // OK?

  return extractPathComponent(std::move(args.where.name))
      .thenValue([this, ino = args.where.dir.ino, mode, &context](
                     PathComponent&& name) {
        return dispatcher_->mkdir(
            ino, std::move(name), mode, context.getObjectFetchContext());
      })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::MkdirRes> try_) mutable {
        if (try_.hasException()) {
          MKDIR3res res{
              {{exceptionToNfsError(try_.exception()), MKDIR3resfail{}}}};
          XdrTrait<MKDIR3res>::serialize(ser, res);
        } else {
          const auto& mkdirRes = try_.value();

          MKDIR3res res{
              {{nfsstat3::NFS3_OK,
                MKDIR3resok{
                    /*obj*/ post_op_fh3{nfs_fh3{mkdirRes.ino}},
                    /*obj_attributes*/
                    post_op_attr{statToFattr3(mkdirRes.stat)},
                    /*dir_wcc*/
                    statToWccData(mkdirRes.preDirStat, mkdirRes.postDirStat),
                }}}};
          XdrTrait<MKDIR3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::symlink(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<SYMLINK3args>::deserialize(deser);

  // Don't allow creating a symlink named . or ..
  if (args.where.name == "." || args.where.name == "..") {
    SYMLINK3res res{{{nfsstat3::NFS3ERR_INVAL, SYMLINK3resfail{}}}};
    XdrTrait<SYMLINK3res>::serialize(ser, res);
    return folly::unit;
  }

  // TODO(xavierd): set the attributes of the symlink with symlink_attr

  return extractPathComponent(std::move(args.where.name))
      .thenValue([this,
                  ino = args.where.dir.ino,
                  symlink_data = std::move(args.symlink.symlink_data),
                  &context](PathComponent&& name) mutable {
        return dispatcher_->symlink(
            ino,
            std::move(name),
            std::move(symlink_data),
            context.getObjectFetchContext());
      })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::SymlinkRes> try_) mutable {
        if (try_.hasException()) {
          SYMLINK3res res{
              {{exceptionToNfsError(try_.exception()), SYMLINK3resfail{}}}};
          XdrTrait<SYMLINK3res>::serialize(ser, res);
        } else {
          const auto& symlinkRes = try_.value();

          SYMLINK3res res{
              {{nfsstat3::NFS3_OK,
                SYMLINK3resok{
                    /*obj*/ post_op_fh3{nfs_fh3{symlinkRes.ino}},
                    /*obj_attributes*/
                    post_op_attr{statToFattr3(symlinkRes.stat)},
                    /*dir_wcc*/
                    statToWccData(
                        symlinkRes.preDirStat, symlinkRes.postDirStat),
                }}}};
          XdrTrait<SYMLINK3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::mknod(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<MKNOD3args>::deserialize(deser);

  switch (args.what.tag) {
    case ftype3::NF3REG:
    case ftype3::NF3DIR:
    case ftype3::NF3LNK: {
      MKNOD3res res{{{nfsstat3::NFS3ERR_BADTYPE, MKNOD3resfail{}}}};
      XdrTrait<MKNOD3res>::serialize(ser, res);
      return folly::unit;
    }
    default:
      break;
  }

  // Don't allow creating a node name . or ..
  if (args.where.name == "." || args.where.name == "..") {
    MKNOD3res res{{{nfsstat3::NFS3ERR_INVAL, MKNOD3resfail{}}}};
    XdrTrait<MKNOD3res>::serialize(ser, res);
    return folly::unit;
  }

  mode_t mode = ftype3ToMode(args.what.tag);
  dev_t rdev;
  if (auto devicedata = std::get_if<devicedata3>(&args.what.v)) {
    mode |= setMode3ToMode(devicedata->dev_attributes.mode);
    rdev = makedev(devicedata->spec.specdata1, devicedata->spec.specdata2);
  } else if (auto sattr = std::get_if<sattr3>(&args.what.v)) {
    mode |= setMode3ToMode(sattr->mode);
    rdev = 0;
  } else {
    // This can only happen if the deserialization code is wrong, but let's be
    // safe.
    MKNOD3res res{{{nfsstat3::NFS3ERR_SERVERFAULT, MKNOD3resfail{}}}};
    XdrTrait<MKNOD3res>::serialize(ser, res);
    return folly::unit;
  }

  // TODO(xavierd): we should probably respect the rest of the sattr3
  // attributes.

  return extractPathComponent(std::move(args.where.name))
      .thenValue([this, ino = args.where.dir.ino, mode, rdev, &context](
                     PathComponent&& name) {
        return dispatcher_->mknod(
            ino, std::move(name), mode, rdev, context.getObjectFetchContext());
      })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::MknodRes> try_) mutable {
        if (try_.hasException()) {
          MKNOD3res res{
              {{exceptionToNfsError(try_.exception()), MKNOD3resfail{}}}};
          XdrTrait<MKNOD3res>::serialize(ser, res);
        } else {
          const auto& mknodRes = try_.value();

          MKNOD3res res{
              {{nfsstat3::NFS3_OK,
                MKNOD3resok{
                    /*obj*/ post_op_fh3{nfs_fh3{mknodRes.ino}},
                    /*obj_attributes*/
                    post_op_attr{statToFattr3(mknodRes.stat)},
                    /*dir_wcc*/
                    statToWccData(mknodRes.preDirStat, mknodRes.postDirStat),
                }}}};
          XdrTrait<MKNOD3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::remove(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<REMOVE3args>::deserialize(deser);

  // Don't allow removing the special directories.
  if (args.object.name == "." || args.object.name == "..") {
    REMOVE3res res{{{nfsstat3::NFS3ERR_ACCES, REMOVE3resfail{}}}};
    XdrTrait<REMOVE3res>::serialize(ser, res);
    return folly::unit;
  }

  // TODO(xavierd): What if args.object.name is a directory? This will fail
  // with NFS3ERR_ISDIR, but the spec is vague regarding what needs to happen
  // here, "REMOVE can be used to remove directories, subject to restrictions
  // imposed by either the client or server interfaces"

  return extractPathComponent(std::move(args.object.name))
      .thenValue(
          [this, ino = args.object.dir.ino, &context](PathComponent&& name) {
            return dispatcher_->unlink(
                ino, std::move(name), context.getObjectFetchContext());
          })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::UnlinkRes> try_) mutable {
        if (try_.hasException()) {
          REMOVE3res res{
              {{exceptionToNfsError(try_.exception()), REMOVE3resfail{}}}};
          XdrTrait<REMOVE3res>::serialize(ser, res);
        } else {
          const auto& unlinkRes = try_.value();

          REMOVE3res res{
              {{nfsstat3::NFS3_OK,
                REMOVE3resok{/*dir_wcc*/ statToWccData(
                    unlinkRes.preDirStat, unlinkRes.postDirStat)}}}};
          XdrTrait<REMOVE3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::rmdir(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<RMDIR3args>::deserialize(deser);

  // Don't allow removing the special directories.
  if (args.object.name == "." || args.object.name == "..") {
    // The NFS spec specifies 2 different error status for "." and ".."
    auto status = args.object.name == "." ? nfsstat3::NFS3ERR_INVAL
                                          : nfsstat3::NFS3ERR_EXIST;
    RMDIR3res res{{{status, RMDIR3resfail{}}}};
    XdrTrait<RMDIR3res>::serialize(ser, res);
    return folly::unit;
  }

  return extractPathComponent(std::move(args.object.name))
      .thenValue(
          [this, ino = args.object.dir.ino, &context](PathComponent&& name) {
            return dispatcher_->rmdir(
                ino, std::move(name), context.getObjectFetchContext());
          })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::RmdirRes> try_) mutable {
        if (try_.hasException()) {
          RMDIR3res res{
              {{exceptionToNfsError(try_.exception()), RMDIR3resfail{}}}};
          XdrTrait<RMDIR3res>::serialize(ser, res);
        } else {
          const auto& rmdirRes = try_.value();

          RMDIR3res res{
              {{nfsstat3::NFS3_OK,
                RMDIR3resok{/*dir_wcc*/ statToWccData(
                    rmdirRes.preDirStat, rmdirRes.postDirStat)}}}};
          XdrTrait<RMDIR3res>::serialize(ser, res);
        }
        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::rename(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<RENAME3args>::deserialize(deser);

  if (args.from.name == "." || args.from.name == ".." || args.to.name == "." ||
      args.to.name == "..") {
    RENAME3res res{{{nfsstat3::NFS3ERR_INVAL, RENAME3resfail{}}}};
    XdrTrait<RENAME3res>::serialize(ser, res);
    return folly::unit;
  }

  // Do nothing if the source and destination are the exact same file.
  if (args.from == args.to) {
    RENAME3res res{{{nfsstat3::NFS3_OK, RENAME3resok{}}}};
    XdrTrait<RENAME3res>::serialize(ser, res);
    return folly::unit;
  }

  return extractPathComponent(std::move(args.from.name))
      .thenValue([toName = args.to.name](PathComponent&& fromName) mutable {
        return extractPathComponent(std::move(toName))
            .thenValue(
                [fromName = std::move(fromName)](PathComponent&& toName) mutable
                -> std::tuple<PathComponent, PathComponent> {
                  return {std::move(fromName), std::move(toName)};
                });
      })
      .thenValue([this,
                  fromIno = args.from.dir.ino,
                  toIno = args.to.dir.ino,
                  &context](std::tuple<PathComponent, PathComponent>&& paths) {
        auto [fromName, toName] = std::move(paths);
        return dispatcher_->rename(
            fromIno,
            std::move(fromName),
            toIno,
            std::move(toName),
            context.getObjectFetchContext());
      })
      .thenTry([ser = std::move(ser)](
                   folly::Try<NfsDispatcher::RenameRes> try_) mutable {
        if (try_.hasException()) {
          RENAME3res res{
              {{exceptionToNfsError(try_.exception()), RENAME3resfail{}}}};
          XdrTrait<RENAME3res>::serialize(ser, res);
        } else {
          const auto& renameRes = try_.value();

          RENAME3res res{
              {{nfsstat3::NFS3_OK,
                RENAME3resok{
                    /*fromdir_wcc*/ statToWccData(
                        renameRes.fromPreDirStat, renameRes.fromPostDirStat),
                    /*todir_wcc*/
                    statToWccData(
                        renameRes.toPreDirStat, renameRes.toPostDirStat),
                }}}};
          XdrTrait<RENAME3res>::serialize(ser, res);
        }

        return folly::unit;
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::link(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<LINK3args>::deserialize(deser);

  // EdenFS doesn't support hardlinks, let's just collect the attributes for
  // the file and fail.
  return dispatcher_->getattr(args.file.ino, context.getObjectFetchContext())
      .thenTry(
          [ser = std::move(ser)](const folly::Try<struct stat>& try_) mutable {
            LINK3res res{
                {{nfsstat3::NFS3ERR_NOTSUPP,
                  LINK3resfail{statToPostOpAttr(try_), wcc_data{}}}}};
            XdrTrait<LINK3res>::serialize(ser, res);
            return folly::unit;
          });
}

/**
 * Verify that the passed in cookie verifier is valid.
 *
 * The verifier allows the server to know whether the directory was modified
 * across readdir calls, and to restart if this is the case.
 *
 * TODO(xavierd): For now, this only checks that the verifier is 0, in the
 * future, we may want to compare it against a global counter that is
 * incremented for each update operations. The assumption being that: "The
 * client should be careful to avoid holding directory entry cookies across
 * operations that modify the directory contents, such as REMOVE and CREATE.",
 * thus we only need to protect against concurrent update and readdir
 * operations since there is only one client per mount.
 */
bool isReaddirCookieverfValid(uint64_t verf) {
  return verf == 0;
}

/**
 * Return the current global cookie.
 *
 * See the documentation above for the meaning of the cookie verifier.
 */
uint64_t getReaddirCookieverf() {
  return 0;
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::readdir(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<READDIR3args>::deserialize(deser);

  if (!isReaddirCookieverfValid(args.cookieverf)) {
    READDIR3res res{{{nfsstat3::NFS3ERR_BAD_COOKIE, READDIR3resfail{}}}};
    XdrTrait<READDIR3res>::serialize(ser, res);
    return folly::unit;
  }

  return dispatcher_
      ->readdir(
          args.dir.ino,
          args.cookie,
          args.count,
          context.getObjectFetchContext())
      .thenTry([this, ino = args.dir.ino, ser = std::move(ser), &context](
                   folly::Try<NfsDispatcher::ReaddirRes> try_) mutable {
        return dispatcher_->getattr(ino, context.getObjectFetchContext())
            .thenTry([ser = std::move(ser), try_ = std::move(try_)](
                         const folly::Try<struct stat>& tryStat) mutable {
              if (try_.hasException()) {
                READDIR3res res{
                    {{exceptionToNfsError(try_.exception()),
                      READDIR3resfail{statToPostOpAttr(tryStat)}}}};
                XdrTrait<READDIR3res>::serialize(ser, res);
              } else {
                auto& readdirRes = try_.value();

                READDIR3res res{
                    {{nfsstat3::NFS3_OK,
                      READDIR3resok{
                          /*dir_attributes*/ statToPostOpAttr(tryStat),
                          /*cookieverf*/ getReaddirCookieverf(),
                          /*reply*/
                          dirlist3{
                              /*entries*/ readdirRes.entries
                                  .extractList<entry3>(),
                              /*eof*/ readdirRes.isEof,
                          }}}}};
                XdrTrait<READDIR3res>::serialize(ser, res);
              }
              return folly::unit;
            });
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::readdirplus(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<READDIRPLUS3args>::deserialize(deser);

  if (!isReaddirCookieverfValid(args.cookieverf)) {
    READDIRPLUS3res res{
        {{nfsstat3::NFS3ERR_BAD_COOKIE, READDIRPLUS3resfail{}}}};
    XdrTrait<READDIRPLUS3res>::serialize(ser, res);
    return folly::unit;
  }

  // TODO(T107744453): Should probably acount for args.maxcount somewhere
  return dispatcher_
      ->readdirplus(
          args.dir.ino,
          args.cookie,
          args.dircount,
          context.getObjectFetchContext())
      .thenTry([this, ino = args.dir.ino, ser = std::move(ser), &context](
                   folly::Try<NfsDispatcher::ReaddirRes> try_) mutable {
        return dispatcher_->getattr(ino, context.getObjectFetchContext())
            .thenTry([ser = std::move(ser), try_ = std::move(try_)](
                         const folly::Try<struct stat>& tryStat) mutable {
              if (try_.hasException()) {
                READDIRPLUS3res res{
                    {{exceptionToNfsError(try_.exception()),
                      READDIRPLUS3resfail{statToPostOpAttr(tryStat)}}}};
                XdrTrait<READDIRPLUS3res>::serialize(ser, res);
              } else {
                auto& readdirRes = try_.value();
                /* TODO @cuev: This is prob where we'd use args.maxcount:
                 *
                 * From rfc 1813 section 3.3.17:
                 *
                 * maxcount
                 *    The maximum size of the READDIRPLUS3resok structure, in
                 *    bytes. The size must include all XDR overhead. The server
                 *    is free to return fewer than maxcount bytes of data.
                 */
                READDIRPLUS3res res{
                    {{nfsstat3::NFS3_OK,
                      READDIRPLUS3resok{
                          /*dir_attributes*/ statToPostOpAttr(tryStat),
                          /*cookieverf*/ getReaddirCookieverf(),
                          /*reply*/
                          dirlistplus3{
                              /*entries*/ readdirRes.entries
                                  .extractList<entryplus3>(),
                              /*eof*/ readdirRes.isEof,
                          }}}}};
                XdrTrait<READDIRPLUS3res>::serialize(ser, res);
              }
              return folly::unit;
            });
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::fsstat(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<FSSTAT3args>::deserialize(deser);

  return dispatcher_->statfs(args.fsroot.ino, context.getObjectFetchContext())
      .thenTry([this, ser = std::move(ser), ino = args.fsroot.ino, &context](
                   folly::Try<struct statfs> statFsTry) mutable {
        return dispatcher_->getattr(ino, context.getObjectFetchContext())
            .thenTry([ser = std::move(ser), statFsTry = std::move(statFsTry)](
                         const folly::Try<struct stat>& statTry) mutable {
              if (statFsTry.hasException()) {
                FSSTAT3res res{
                    {{exceptionToNfsError(statFsTry.exception()),
                      FSSTAT3resfail{statToPostOpAttr(statTry)}}}};
                XdrTrait<FSSTAT3res>::serialize(ser, res);
              } else {
                auto& statfs = statFsTry.value();

                FSSTAT3res res{
                    {{nfsstat3::NFS3_OK,
                      FSSTAT3resok{
                          /*obj_attributes*/ statToPostOpAttr(statTry),
                          /*tbytes*/ statfs.f_blocks * statfs.f_bsize,
                          /*fbytes*/ statfs.f_blocks * statfs.f_bsize,
                          /*abytes*/ statfs.f_bavail * statfs.f_bsize,
                          /*tfiles*/ statfs.f_files,
                          /*ffiles*/ statfs.f_ffree,
                          /*afiles*/ statfs.f_ffree,
                          /*invarsec*/ 0,
                      }}}};
                XdrTrait<FSSTAT3res>::serialize(ser, res);
              }

              return folly::unit;
            });
      });
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::fsinfo(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<FSINFO3args>::deserialize(deser);
  (void)args;

  FSINFO3res res{
      {{nfsstat3::NFS3_OK,
        FSINFO3resok{
            // TODO(xavierd): fill the post_op_attr.
            post_op_attr{},
            /*rtmax=*/iosize_,
            /*rtpref=*/iosize_,
            /*rtmult=*/1,
            /*wtmax=*/iosize_,
            /*wtpref=*/iosize_,
            /*wtmult=*/1,
            /*dtpref=*/iosize_,
            /*maxfilesize=*/std::numeric_limits<uint64_t>::max(),
            nfstime3{0, 1},
            /*properties*/ FSF3_SYMLINK | FSF3_HOMOGENEOUS | FSF3_CANSETTIME,
        }}}};

  XdrTrait<FSINFO3res>::serialize(ser, res);

  return folly::unit;
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::pathconf(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::SUCCESS, context.getXid());
  auto args = XdrTrait<PATHCONF3args>::deserialize(deser);
  (void)args;

  PATHCONF3res res{
      {{nfsstat3::NFS3_OK,
        PATHCONF3resok{
            // TODO(xavierd): fill up the post_op_attr
            post_op_attr{},
            /*linkmax=*/0,
            /*name_max=*/NAME_MAX,
            /*no_trunc=*/true,
            /*chown_restricted=*/true,
            /*case_insensitive=*/caseSensitive_ == CaseSensitivity::Insensitive,
            /*case_preserving=*/true,
        }}}};

  XdrTrait<PATHCONF3res>::serialize(ser, res);

  return folly::unit;
}

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::commit(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    NfsRequestContext& context) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, context.getXid());
  return folly::unit;
}

NfsArgsDetails formatNull(folly::io::Cursor /*deser*/) {
  return {""};
}

NfsArgsDetails formatGetattr(folly::io::Cursor deser) {
  auto args = XdrTrait<GETATTR3args>::deserialize(deser);
  return {fmt::format(FMT_STRING("ino={}"), args.object.ino), args.object.ino};
}

NfsArgsDetails formatSattr3(const sattr3& attr) {
  auto formatOpt = [](auto&& val, const char* fmtString = "{}") {
    using T = std::decay_t<decltype(val)>;
    if (val.tag) {
      return fmt::format(
          fmt::runtime(fmtString), std::get<typename T::TrueVariant>(val.v));
    }
    return std::string();
  };

  // TODO(xavierd): format the times too?
  return fmt::format(
      FMT_STRING("mode={}, uid={}, gid={}, size={}"),
      formatOpt(attr.mode, "{:#o}"),
      formatOpt(attr.uid),
      formatOpt(attr.gid),
      formatOpt(attr.size));
}

NfsArgsDetails formatSetattr(folly::io::Cursor deser) {
  auto args = XdrTrait<SETATTR3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("ino={}, attr=({}) guarded={}"),
          args.object.ino,
          formatSattr3(args.new_attributes).str,
          args.guard.tag),
      args.object.ino};
}

NfsArgsDetails formatLookup(folly::io::Cursor deser) {
  auto args = XdrTrait<LOOKUP3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}"), args.what.dir.ino, args.what.name),
      args.what.dir.ino};
}

NfsArgsDetails formatAccess(folly::io::Cursor deser) {
  auto args = XdrTrait<ACCESS3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("ino={}, access={:#x}"), args.object.ino, args.access),
      args.object.ino};
}

NfsArgsDetails formatReadlink(folly::io::Cursor deser) {
  auto args = XdrTrait<READLINK3args>::deserialize(deser);
  return {
      fmt::format(FMT_STRING("ino={}"), args.symlink.ino), args.symlink.ino};
}

NfsArgsDetails formatRead(folly::io::Cursor deser) {
  auto args = XdrTrait<READ3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("ino={}, size={}, offset={}"),
          args.file.ino,
          args.count,
          args.offset),
      args.file.ino};
}

NfsArgsDetails formatWrite(folly::io::Cursor deser) {
  auto args = XdrTrait<WRITE3args>::deserialize(deser);
  auto formatStable = [](stable_how stable) {
    switch (stable) {
      case stable_how::UNSTABLE:
        return "UNSTABLE";
      case stable_how::DATA_SYNC:
        return "DATA_SYNC";
      case stable_how::FILE_SYNC:
        return "FILE_SYNC";
    }
    throw_<std::domain_error>(
        "unexpected stable_how ", folly::to_underlying(stable));
  };
  return {
      fmt::format(
          FMT_STRING("ino={}, size={}, offset={}, stable={}"),
          args.file.ino,
          args.count,
          args.offset,
          formatStable(args.stable)),
      args.file.ino};
}

NfsArgsDetails formatCreate(folly::io::Cursor deser) {
  auto args = XdrTrait<CREATE3args>::deserialize(deser);
  auto formatMode = [](createmode3 createmode) {
    switch (createmode) {
      case createmode3::UNCHECKED:
        return "UNCHECKED";
      case createmode3::GUARDED:
        return "GUARDED";
      case createmode3::EXCLUSIVE:
        return "EXCLUSIVE";
    }
    throw_<std::domain_error>(
        "unexpected createmode3 ", folly::to_underlying(createmode));
  };
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}, mode={}{}"),
          args.where.dir.ino,
          args.where.name,
          formatMode(args.how.tag),
          args.how.tag != createmode3::EXCLUSIVE
              ? fmt::format(
                    FMT_STRING(" attr=({})"),
                    formatSattr3(std::get<sattr3>(args.how.v)).str)
              : ""),
      args.where.dir.ino};
}

NfsArgsDetails formatMkdir(folly::io::Cursor deser) {
  auto args = XdrTrait<MKDIR3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}, attr=({})"),
          args.where.dir.ino,
          args.where.name,
          formatSattr3(args.attributes).str),
      args.where.dir.ino};
}

NfsArgsDetails formatSymlink(folly::io::Cursor deser) {
  auto args = XdrTrait<SYMLINK3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}, symlink={}, attr=({})"),
          args.where.dir.ino,
          args.where.name,
          args.symlink.symlink_data,
          formatSattr3(args.symlink.symlink_attributes).str),
      args.where.dir.ino};
}

NfsArgsDetails formatMknod(folly::io::Cursor deser) {
  auto args = XdrTrait<MKNOD3args>::deserialize(deser);
  auto formatFtype = [](const ftype3& type) {
    switch (type) {
      case ftype3::NF3REG:
        return "REG";
      case ftype3::NF3DIR:
        return "DIR";
      case ftype3::NF3BLK:
        return "BLK";
      case ftype3::NF3CHR:
        return "CHR";
      case ftype3::NF3LNK:
        return "LNK";
      case ftype3::NF3SOCK:
        return "SOCK";
      case ftype3::NF3FIFO:
        return "FIFO";
    }
    throw_<std::domain_error>("unexpected ftype3 ", folly::to_underlying(type));
  };
  auto formatWhat = [](const mknoddata3& data) {
    return std::visit(
        [](auto&& arg) -> std::string {
          using ArgType = std::decay_t<decltype(arg)>;
          if constexpr (std::is_same_v<ArgType, devicedata3>) {
            // TODO(xavierd): format the specdata3 too.
            return fmt::format(
                ", attr=({})", formatSattr3(arg.dev_attributes).str);
          } else if constexpr (std::is_same_v<ArgType, sattr3>) {
            return fmt::format(", attr=({})", formatSattr3(arg).str);
          } else {
            return "";
          }
        },
        data.v);
  };
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}, type={}{}"),
          args.where.dir.ino,
          args.where.name,
          formatFtype(args.what.tag),
          formatWhat(args.what)),
      args.where.dir.ino};
}

NfsArgsDetails formatRemove(folly::io::Cursor deser) {
  auto args = XdrTrait<REMOVE3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}"), args.object.dir.ino, args.object.name),
      args.object.dir.ino};
}

NfsArgsDetails formatRmdir(folly::io::Cursor deser) {
  auto args = XdrTrait<RMDIR3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, name={}"), args.object.dir.ino, args.object.name),
      args.object.dir.ino};
}

NfsArgsDetails formatRename(folly::io::Cursor deser) {
  auto args = XdrTrait<RENAME3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("fromDir={}, fromName={}, toDir={}, toName={}"),
          args.from.dir.ino,
          args.from.name,
          args.to.dir.ino,
          args.to.name),
      args.to.dir.ino};
}

NfsArgsDetails formatLink(folly::io::Cursor deser) {
  auto args = XdrTrait<LINK3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("ino={}, dir={}, name={}"),
          args.file.ino,
          args.link.dir.ino,
          args.link.name),
      args.file.ino};
}

NfsArgsDetails formatReaddir(folly::io::Cursor deser) {
  auto args = XdrTrait<READDIR3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING("dir={}, cookie={}, cookieverf={}, count={}"),
          args.dir.ino,
          args.cookie,
          args.cookieverf,
          args.count),
      args.dir.ino};
}

NfsArgsDetails formatReaddirplus(folly::io::Cursor deser) {
  auto args = XdrTrait<READDIRPLUS3args>::deserialize(deser);
  return {
      fmt::format(
          FMT_STRING(
              "dir={}, cookie={}, cookieverf={}, dircount={}, maxcount={}"),
          args.dir.ino,
          args.cookie,
          args.cookieverf,
          args.dircount,
          args.maxcount),
      args.dir.ino};
}

NfsArgsDetails formatFsstat(folly::io::Cursor deser) {
  auto args = XdrTrait<FSSTAT3args>::deserialize(deser);
  return {fmt::format(FMT_STRING("ino={}"), args.fsroot.ino), args.fsroot.ino};
}

NfsArgsDetails formatFsinfo(folly::io::Cursor deser) {
  auto args = XdrTrait<FSINFO3args>::deserialize(deser);
  return {fmt::format(FMT_STRING("ino={}"), args.fsroot.ino), args.fsroot.ino};
}

NfsArgsDetails formatPathconf(folly::io::Cursor deser) {
  auto args = XdrTrait<PATHCONF3args>::deserialize(deser);
  return {fmt::format(FMT_STRING("ino={}"), args.object.ino), args.object.ino};
}

NfsArgsDetails formatCommit(folly::io::Cursor /*deser*/) {
  // TODO(xavierd): Fill this in.
  return {""};
}

using Handler = ImmediateFuture<folly::Unit> (Nfsd3ServerProcessor::*)(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    NfsRequestContext& context);

/**
 * Format the passed in arguments. The Cursor must be passed as a copy to avoid
 * disrupting the actual handler.
 */
using FormatArgs = NfsArgsDetails (*)(folly::io::Cursor deser);
using AccessType = ProcessAccessLog::AccessType;

struct HandlerEntry {
  constexpr HandlerEntry() = default;
  constexpr HandlerEntry(
      folly::StringPiece n,
      Handler h,
      FormatArgs format,
      NfsStats::DurationPtr s,
      AccessType at = AccessType::FsChannelOther,
      SamplingGroup samplingGroup = SamplingGroup::DropAll)
      : name(n),
        handler(h),
        formatArgs(format),
        stat{s},
        accessType(at),
        samplingGroup{samplingGroup} {}

  folly::StringPiece name;
  Handler handler = nullptr;
  FormatArgs formatArgs = nullptr;
  NfsStats::DurationPtr stat = nullptr;
  AccessType accessType = AccessType::FsChannelOther;
  SamplingGroup samplingGroup = SamplingGroup::DropAll;
};

constexpr auto kNfs3dHandlers = [] {
  const auto Read = AccessType::FsChannelRead;
  const auto Write = AccessType::FsChannelWrite;

  std::array<HandlerEntry, 22> handlers;
  handlers[folly::to_underlying(nfsv3Procs::null)] = {
      "NULL", &Nfsd3ServerProcessor::null, formatNull, &NfsStats::nfsNull};
  handlers[folly::to_underlying(nfsv3Procs::getattr)] = {
      "GETATTR",
      &Nfsd3ServerProcessor::getattr,
      formatGetattr,
      &NfsStats::nfsGetattr,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::setattr)] = {
      "SETATTR",
      &Nfsd3ServerProcessor::setattr,
      formatSetattr,
      &NfsStats::nfsSetattr,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::lookup)] = {
      "LOOKUP",
      &Nfsd3ServerProcessor::lookup,
      formatLookup,
      &NfsStats::nfsLookup,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::access)] = {
      "ACCESS",
      &Nfsd3ServerProcessor::access,
      formatAccess,
      &NfsStats::nfsAccess,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::readlink)] = {
      "READLINK",
      &Nfsd3ServerProcessor::readlink,
      formatReadlink,
      &NfsStats::nfsReadlink,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::read)] = {
      "READ",
      &Nfsd3ServerProcessor::read,
      formatRead,
      &NfsStats::nfsRead,
      Read,
      SamplingGroup::Three};
  handlers[folly::to_underlying(nfsv3Procs::write)] = {
      "WRITE",
      &Nfsd3ServerProcessor::write,
      formatWrite,
      &NfsStats::nfsWrite,
      Write,
      SamplingGroup::Two};
  handlers[folly::to_underlying(nfsv3Procs::create)] = {
      "CREATE",
      &Nfsd3ServerProcessor::create,
      formatCreate,
      &NfsStats::nfsCreate,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::mkdir)] = {
      "MKDIR",
      &Nfsd3ServerProcessor::mkdir,
      formatMkdir,
      &NfsStats::nfsMkdir,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::symlink)] = {
      "SYMLINK",
      &Nfsd3ServerProcessor::symlink,
      formatSymlink,
      &NfsStats::nfsSymlink,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::mknod)] = {
      "MKNOD",
      &Nfsd3ServerProcessor::mknod,
      formatMknod,
      &NfsStats::nfsMknod,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::remove)] = {
      "REMOVE",
      &Nfsd3ServerProcessor::remove,
      formatRemove,
      &NfsStats::nfsRemove,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::rmdir)] = {
      "RMDIR",
      &Nfsd3ServerProcessor::rmdir,
      formatRmdir,
      &NfsStats::nfsRmdir,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::rename)] = {
      "RENAME",
      &Nfsd3ServerProcessor::rename,
      formatRename,
      &NfsStats::nfsRename,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::link)] = {
      "LINK",
      &Nfsd3ServerProcessor::link,
      formatLink,
      &NfsStats::nfsLink,
      Write};
  handlers[folly::to_underlying(nfsv3Procs::readdir)] = {
      "READDIR",
      &Nfsd3ServerProcessor::readdir,
      formatReaddir,
      &NfsStats::nfsReaddir,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::readdirplus)] = {
      "READDIRPLUS",
      &Nfsd3ServerProcessor::readdirplus,
      formatReaddirplus,
      &NfsStats::nfsReaddirplus,
      Read};
  handlers[folly::to_underlying(nfsv3Procs::fsstat)] = {
      "FSSTAT",
      &Nfsd3ServerProcessor::fsstat,
      formatFsstat,
      &NfsStats::nfsFsstat};
  handlers[folly::to_underlying(nfsv3Procs::fsinfo)] = {
      "FSINFO",
      &Nfsd3ServerProcessor::fsinfo,
      formatFsinfo,
      &NfsStats::nfsFsinfo};
  handlers[folly::to_underlying(nfsv3Procs::pathconf)] = {
      "PATHCONF",
      &Nfsd3ServerProcessor::pathconf,
      formatPathconf,
      &NfsStats::nfsPathconf};
  handlers[folly::to_underlying(nfsv3Procs::commit)] = {
      "COMMIT",
      &Nfsd3ServerProcessor::commit,
      formatCommit,
      &NfsStats::nfsCommit,
      Write};

  return handlers;
}();

namespace {
struct LiveRequest {
  LiveRequest(
      std::shared_ptr<TraceBus<NfsTraceEvent>> traceBus,
      std::atomic<size_t>& traceDetailedArguments,
      const HandlerEntry& handlerEntry,
      folly::io::Cursor& deser,
      uint32_t xid,
      uint32_t procNumber)
      : traceBus_{std::move(traceBus)}, xid_{xid}, procNumber_{procNumber} {
    if (traceDetailedArguments.load(std::memory_order_acquire)) {
      traceBus_->publish(NfsTraceEvent::start(
          xid, procNumber, handlerEntry.formatArgs(deser)));
    } else {
      traceBus_->publish(NfsTraceEvent::start(xid, procNumber));
    }
  }

  LiveRequest(LiveRequest&& that) noexcept = default;
  LiveRequest& operator=(LiveRequest&&) = delete;

  ~LiveRequest() {
    if (traceBus_) {
      traceBus_->publish(NfsTraceEvent::finish(xid_, procNumber_));
    }
  }

  std::shared_ptr<TraceBus<NfsTraceEvent>> traceBus_;
  uint32_t xid_;
  uint32_t procNumber_;
};

SamplingGroup nfsProcSamplingGroup(uint32_t procNumber) {
  XDCHECK(procNumber < kNfs3dHandlers.size())
      << "got invalid NFS procedure: " << procNumber;
  return kNfs3dHandlers[procNumber].samplingGroup;
}
} // namespace

ImmediateFuture<folly::Unit> Nfsd3ServerProcessor::dispatchRpc(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
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

  auto& handlerEntry = kNfs3dHandlers[procNumber];
  FB_LOGF(
      *straceLogger_,
      DBG7,
      "{}({})",
      handlerEntry.name,
      handlerEntry.formatArgs(deser).str);

  auto liveRequest = LiveRequest{
      traceBus_, traceDetailedArguments_, handlerEntry, deser, xid, procNumber};

  // TODO: Add requestMetrics for NFS.
  std::shared_ptr<RequestMetricsScope::LockedRequestWatchList> nullRequestWatch;
  auto context = std::make_unique<NfsRequestContext>(
      xid, handlerEntry.name, processAccessLog_);
  context->startRequest(
      dispatcher_->getStats(), handlerEntry.stat, nullRequestWatch);

  // The data that contextRef reference to is alive for the duration of the
  // handler function and is deleted when context unique_ptr goes out of the
  // scope at the `ensure` lambda.
  return makeImmediateFutureWith([&] {
           return (this->*handlerEntry.handler)(
               std::move(deser), std::move(ser), *context);
         })
      .thenTry([&handlerEntry](folly::Try<folly::Unit>&& res) {
        if (res.hasException()) {
          if (auto* err = res.exception().get_exception<RpcParsingError>()) {
            err->setProcedureContext(std::string{handlerEntry.name});
          }
        }
        return std::move(res);
      })
      .ensure([liveRequest = std::move(liveRequest),
               context = std::move(context)]() {});
}

void Nfsd3ServerProcessor::onShutdown(RpcStopData data) {
  // Note this triggers the Nfsd3 destruction which will also destroy
  // Nfsd3ServerProcessor. Don't do anything will the Nfsd3ServerProcessor
  // member variables after this!
  stopPromise_.setValue(std::move(data));
}

void Nfsd3ServerProcessor::clientConnected() {
  auto numberOfClients =
      numberOfClients_.fetch_add(1, std::memory_order_acq_rel);
  if (numberOfClients > 1) {
    structuredLogger_->logEvent(TooManyNfsClients{});
  }
}
} // namespace

Nfsd3::Nfsd3(
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool,
    std::unique_ptr<NfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<FsEventLogger> fsEventLogger,
    const std::shared_ptr<StructuredLogger>& structuredLogger,
    folly::Duration /*requestTimeout*/,
    std::shared_ptr<Notifier> /*notifier*/,
    CaseSensitivity caseSensitive,
    uint32_t iosize,
    size_t traceBusCapacity)
    : server_(RpcServer::create(
          std::make_shared<Nfsd3ServerProcessor>(
              std::move(dispatcher),
              straceLogger,
              structuredLogger,
              caseSensitive,
              iosize,
              stopPromise_,
              processAccessLog_,
              traceDetailedArguments_,
              traceBus_),
          evb,
          std::move(threadPool),
          structuredLogger)),
      processAccessLog_(std::move(processNameCache)),
      invalidationExecutor_{
          folly::SerialExecutor::create(folly::getGlobalCPUExecutor())},
      traceDetailedArguments_{0},
      traceBus_{TraceBus<NfsTraceEvent>::create("NfsTrace", traceBusCapacity)} {
  traceSubscriptionHandles_.push_back(traceBus_->subscribeFunction(
      "NFS request tracking",
      [this,
       fsEventLogger = std::move(fsEventLogger)](const NfsTraceEvent& event) {
        switch (event.getType()) {
          case NfsTraceEvent::START: {
            auto state = telemetryState_.wlock();
            // NFS client is allowed to retry requests and emplace could
            // therefore fail. We just ignore duplicated requests.
            (void)state->requests.emplace(
                event.getXid(),
                OutstandingRequest{event.getXid(), event.monotonicTime});
            break;
          }
          case NfsTraceEvent::FINISH: {
            std::chrono::nanoseconds durationNs{0};
            {
              auto state = telemetryState_.wlock();
              auto it = state->requests.find(event.getXid());
              if (it == state->requests.end()) {
                // Duplicated request, break early.
                break;
              }
              durationNs = std::chrono::duration_cast<std::chrono::nanoseconds>(
                  event.monotonicTime - it->second.requestStartTime);
              (void)state->requests.erase(it);
            }

            if (fsEventLogger) {
              auto procNumber = event.getProcNumber();
              fsEventLogger->log({
                  durationNs,
                  nfsProcSamplingGroup(procNumber),
                  nfsProcName(procNumber),
              });
            }
            break;
          }
        }
      }));
}

void Nfsd3::initialize(folly::SocketAddress addr, bool registerWithRpcbind) {
  server_->initialize(addr);
  if (registerWithRpcbind) {
    server_->registerService(kNfsdProgNumber, kNfsd3ProgVersion);
  }
}

void Nfsd3::initialize(folly::File&& connectedSocket) {
  XLOG(DBG7) << "Initializing nfsd3 with connected socket: "
             << connectedSocket.fd();
  server_->initialize(
      std::move(connectedSocket),
      RpcServer::InitialSocketType::CONNECTED_SOCKET);
}

void Nfsd3::invalidate(AbsolutePath path, mode_t mode) {
  invalidationExecutor_->add([path = std::move(path), mode]() {
    try {
      XLOG(DBG9) << "Invalidating: " << path.c_str() << " mode: " << mode;
      { chmod(path.c_str(), mode); }
      XLOG(DBG9) << "Finished invalidating: " << path.c_str();
    } catch (const std::exception& ex) {
      if (const auto* system_error =
              dynamic_cast<const std::system_error*>(&ex)) {
        if (isEnoent(*system_error)) {
          // A removed path would result in an ENOENT error, this is expected,
          // don't warn about it.
          return;
        }
      }
      XLOGF(ERR, "Couldn't invalidate {}: {}", path, folly::exceptionStr(ex));
    }
  });
}

folly::Future<folly::Unit> Nfsd3::flushInvalidations() {
  folly::Promise<folly::Unit> promise;
  auto result = promise.getFuture();
  invalidationExecutor_->add([promise = std::move(promise)]() mutable {
    // Since the invalidationExecutor_ is a SerialExecutor, this lambda will
    // run only when all the previously added open have completed.
    promise.setValue(folly::unit);
  });
  return result;
}

std::vector<Nfsd3::OutstandingRequest> Nfsd3::getOutstandingRequests() {
  std::vector<Nfsd3::OutstandingRequest> outstandingCalls;

  auto telemetryStateLockedPtr = telemetryState_.rlock();
  for (const auto& entry : telemetryStateLockedPtr->requests) {
    outstandingCalls.push_back(entry.second);
  }
  return outstandingCalls;
}

TraceDetailedArgumentsHandle Nfsd3::traceDetailedArguments() {
  auto handle =
      std::shared_ptr<void>(nullptr, [&copy = traceDetailedArguments_](void*) {
        copy.fetch_sub(1, std::memory_order_acq_rel);
      });
  traceDetailedArguments_.fetch_add(1, std::memory_order_acq_rel);
  return handle;
};

Nfsd3::~Nfsd3() {
  // TODO(xavierd): wait for the pending requests,
  // Note the socket will already have been torn down, as this is only destroyed
  // when the socket was closed.
}

folly::SemiFuture<Nfsd3::StopData> Nfsd3::getStopFuture() {
  return stopPromise_.getSemiFuture();
}

void Nfsd3::takeoverStop() {
  XLOG(DBG7) << "calling takeover stop on the nfs RpcServer";
  server_->takeoverStop().via(
      server_->getEventBase()); // we do this to make sure the takeover future
  // was completely scheduled
}

folly::StringPiece nfsProcName(uint32_t procNumber) {
  XDCHECK(procNumber < kNfs3dHandlers.size())
      << "got invalid NFS procedure: " << procNumber;
  return kNfs3dHandlers[procNumber].name;
}

ProcessAccessLog::AccessType nfsProcAccessType(uint32_t procNumber) {
  return procNumber < kNfs3dHandlers.size()
      ? kNfs3dHandlers[procNumber].accessType
      : ProcessAccessLog::AccessType::FsChannelOther;
}

} // namespace facebook::eden

#endif
