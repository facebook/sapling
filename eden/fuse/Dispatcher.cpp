/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Dispatcher.h"
#include <folly/Exception.h>
#include <folly/Format.h>
#include <folly/MoveWrapper.h>
#include <wangle/concurrent/GlobalExecutor.h>
#include "DirHandle.h"
#include "FileHandle.h"
#include "MountPoint.h"
#include "RequestData.h"
#include "SessionDeleter.h"

using namespace folly;
using namespace std::chrono;

namespace facebook {
namespace eden {
namespace fusell {

Dispatcher::Attr::Attr() : timeout(1.0) {
  auto& req = RequestData::get();
  auto mount = req.getChannel().getMountPoint();
  st = mount->initStatData();
}

Dispatcher::~Dispatcher() {}

void Dispatcher::initConnection(fuse_conn_info& conn) {}

FileHandleMap& Dispatcher::getFileHandles() {
  return fileHandles_;
}

std::shared_ptr<FileHandleBase> Dispatcher::getGenericFileHandle(uint64_t fh) {
  return fileHandles_.getGenericFileHandle(fh);
}

std::shared_ptr<FileHandle> Dispatcher::getFileHandle(uint64_t fh) {
  return fileHandles_.getFileHandle(fh);
}
std::shared_ptr<DirHandle> Dispatcher::getDirHandle(uint64_t dh) {
  return fileHandles_.getDirHandle(dh);
}

static std::string flagsToLabel(
    const std::unordered_map<int32_t, const char*>& labels, uint32_t flags) {
  std::vector<const char*> bits;
  for (auto& it : labels) {
    if (it.first == 0) {
      // Sometimes a define evaluates to zero; it's not useful so skip it
      continue;
    }
    if ((flags & it.first) == it.first) {
      bits.push_back(it.second);
      flags &= ~it.first;
    }
  }
  std::string str;
  folly::join(" ", bits, str);
  if (flags == 0) {
    return str;
  }
  return folly::format("{} unknown:0x{:x}", str, flags).str();
}

static std::unordered_map<int32_t, const char*> capsLabels = {
    {FUSE_CAP_ASYNC_READ, "ASYNC_READ"},
    {FUSE_CAP_POSIX_LOCKS, "POSIX_LOCKS"},
    {FUSE_CAP_ATOMIC_O_TRUNC, "ATOMIC_O_TRUNC"},
    {FUSE_CAP_EXPORT_SUPPORT, "EXPORT_SUPPORT"},
    {FUSE_CAP_BIG_WRITES, "BIG_WRITES"},
    {FUSE_CAP_DONT_MASK, "DONT_MASK"},
#ifdef FUSE_CAP_SPLICE_WRITE
    {FUSE_CAP_SPLICE_WRITE, "SPLICE_WRITE"},
    {FUSE_CAP_SPLICE_MOVE, "SPLICE_MOVE"},
    {FUSE_CAP_SPLICE_READ, "SPLICE_READ"},
    {FUSE_CAP_FLOCK_LOCKS, "FLOCK_LOCKS"},
    {FUSE_CAP_IOCTL_DIR, "IOCTL_DIR"},
#endif
#ifdef __APPLE__
    {FUSE_CAP_ALLOCATE, "ALLOCATE"},
    {FUSE_CAP_EXCHANGE_DATA, "EXCHANGE_DATA"},
    {FUSE_CAP_CASE_INSENSITIVE, "CASE_INSENSITIVE"},
    {FUSE_CAP_VOL_RENAME, "VOL_RENAME"},
    {FUSE_CAP_XTIMES, "XTIMES"},
#endif
};

void Dispatcher::disp_init(void* userdata, struct fuse_conn_info* conn) {
  auto disp = reinterpret_cast<Dispatcher*>(userdata);

  conn->want |= conn->capable & (
#ifdef FUSE_CAP_IOCTL_DIR
                                    FUSE_CAP_IOCTL_DIR |
#endif
                                    FUSE_CAP_ATOMIC_O_TRUNC |
                                    FUSE_CAP_BIG_WRITES | FUSE_CAP_ASYNC_READ);

  disp->initConnection(*conn);
  disp->connInfo_ = *conn;
  disp->stats_ = EdenStats();

  LOG(INFO) << "Speaking fuse protocol " << conn->proto_major << "."
            << conn->proto_minor << ", async_read=" << conn->async_read
            << ", max_write=" << conn->max_write
            << ", max_readahead=" << conn->max_readahead
            << ", capable=" << flagsToLabel(capsLabels, conn->capable)
            << ", want=" << flagsToLabel(capsLabels, conn->want);
}

void Dispatcher::destroy() {}

static void disp_destroy(void* userdata) {
  static_cast<Dispatcher*>(userdata)->destroy();
}

folly::Future<fuse_entry_param> Dispatcher::lookup(
    fuse_ino_t parent,
    PathComponentPiece name) {
  throwSystemErrorExplicit(ENOENT);
}

static void disp_lookup(fuse_req_t req, fuse_ino_t parent, const char* name) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(request.startRequest(dispatcher->getStats().lookup)
                               .then([=, &request] {
                                 return dispatcher->lookup(
                                     parent, PathComponentPiece(name));
                               })
                               .then([](fuse_entry_param&& param) {
                                 RequestData::get().replyEntry(param);
                               }));
}

folly::Future<folly::Unit> Dispatcher::forget(fuse_ino_t ino,
                                              unsigned long nlookup) {
  return Unit{};
}

static void disp_forget(fuse_req_t req, fuse_ino_t ino, unsigned long nlookup) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.catchErrors(
      request.startRequest(dispatcher->getStats().forget)
          .then([=, &request] { return dispatcher->forget(ino, nlookup); })
          .then([]() { RequestData::get().replyNone(); }));
}

#if FUSE_MINOR_VERSION >= 9
static void disp_forget_multi(fuse_req_t req, size_t count, fuse_forget_data *forgets) {
  auto& request = RequestData::create(req);
  std::vector<fuse_forget_data> forget(forgets, forgets + count);
  auto* dispatcher = request.getDispatcher();
  request.catchErrors(request.startRequest(dispatcher->getStats().forgetmulti)
                          .then([ =, &request, forget = std::move(forget) ] {
                            for (auto& f : forget) {
                              dispatcher->forget(f.ino, f.nlookup);
                            }
                            return Unit{};
                          })
                          .then([]() { RequestData::get().replyNone(); }));
}
#endif

folly::Future<Dispatcher::Attr> Dispatcher::getattr(fuse_ino_t ino) {
  throwSystemErrorExplicit(ENOENT);
}

static void disp_getattr(fuse_req_t req,
                         fuse_ino_t ino,
                         struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();

  if (fi) {
    request.setRequestFuture(
        request.startRequest(dispatcher->getStats().getattr)
            .then([ =, &request, fi = *fi ] {
              return dispatcher->getGenericFileHandle(fi.fh)->getattr();
            })
            .then([](Dispatcher::Attr&& attr) {
              RequestData::get().replyAttr(attr.st, attr.timeout);
            }));

  } else {
    request.setRequestFuture(
        request.startRequest(dispatcher->getStats().getattr)
            .then([=, &request] { return dispatcher->getattr(ino); })
            .then([](Dispatcher::Attr&& attr) {
              RequestData::get().replyAttr(attr.st, attr.timeout);
            }));
  }
}

folly::Future<Dispatcher::Attr> Dispatcher::setattr(fuse_ino_t ino,
                                                    const struct stat& attr,
                                                    int to_set) {
  FUSELL_NOT_IMPL();
}

static void disp_setattr(fuse_req_t req,
                         fuse_ino_t ino,
                         struct stat* attr,
                         int to_set,
                         struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();

  if (fi) {
    request.setRequestFuture(
        request.startRequest(dispatcher->getStats().setattr)
            .then([ =, &request, fi = *fi ]() {
              return dispatcher->getGenericFileHandle(fi.fh)->setattr(
                  *attr, to_set);
            })
            .then([](Dispatcher::Attr&& attr) {
              RequestData::get().replyAttr(attr.st, attr.timeout);
            }));

  } else {
    request.setRequestFuture(
        request.startRequest(dispatcher->getStats().setattr)
            .then([=, &request]() {
              return dispatcher->setattr(ino, *attr, to_set);
            })
            .then([](Dispatcher::Attr&& attr) {
              RequestData::get().replyAttr(attr.st, attr.timeout);
            }));
  }
}

folly::Future<std::string> Dispatcher::readlink(fuse_ino_t ino) {
  FUSELL_NOT_IMPL();
}

static void disp_readlink(fuse_req_t req, fuse_ino_t ino) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().readlink)
          .then([=, &request] { return dispatcher->readlink(ino); })
          .then([](std::string&& str) {
            RequestData::get().replyReadLink(str);
          }));
}

folly::Future<fuse_entry_param> Dispatcher::mknod(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev) {
  FUSELL_NOT_IMPL();
}

static void disp_mknod(fuse_req_t req,
                       fuse_ino_t parent,
                       const char* name,
                       mode_t mode,
                       dev_t rdev) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().mknod)
          .then([=, &request] {
            return dispatcher->mknod(
                parent, PathComponentPiece(name), mode, rdev);
          })
          .then([](fuse_entry_param&& param) {
            RequestData::get().replyEntry(param);
          }));
}

folly::Future<fuse_entry_param>
    Dispatcher::mkdir(fuse_ino_t, PathComponentPiece, mode_t) {
  FUSELL_NOT_IMPL();
}

static void disp_mkdir(fuse_req_t req,
                       fuse_ino_t parent,
                       const char* name,
                       mode_t mode) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(request.startRequest(dispatcher->getStats().mkdir)
                               .then([=, &request] {
                                 return dispatcher->mkdir(
                                     parent, PathComponentPiece(name), mode);
                               })
                               .then([](fuse_entry_param&& param) {
                                 RequestData::get().replyEntry(param);
                               }));
}

folly::Future<folly::Unit> Dispatcher::unlink(fuse_ino_t, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

static void disp_unlink(fuse_req_t req, fuse_ino_t parent, const char* name) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().unlink)
          .then([=, &request] {
            return dispatcher->unlink(parent, PathComponentPiece(name));
          })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<folly::Unit> Dispatcher::rmdir(fuse_ino_t, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

static void disp_rmdir(fuse_req_t req, fuse_ino_t parent, const char* name) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().rmdir)
          .then([=, &request] {
            return dispatcher->rmdir(parent, PathComponentPiece(name));
          })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<fuse_entry_param>
    Dispatcher::symlink(PathComponentPiece, fuse_ino_t, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

static void disp_symlink(fuse_req_t req,
                         const char* link,
                         fuse_ino_t parent,
                         const char* name) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().symlink)
          .then([=, &request] {
            return dispatcher->symlink(
                PathComponentPiece(link), parent, PathComponentPiece(name));
          })
          .then([](fuse_entry_param&& param) {
            RequestData::get().replyEntry(param);
          }));
}

folly::Future<folly::Unit> Dispatcher::rename(
    fuse_ino_t,
    PathComponentPiece,
    fuse_ino_t,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

static void disp_rename(fuse_req_t req,
                        fuse_ino_t parent,
                        const char* name,
                        fuse_ino_t newparent,
                        const char* newname) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(request.startRequest(dispatcher->getStats().rename)
                               .then([=, &request] {
                                 return dispatcher->rename(
                                     parent,
                                     PathComponentPiece(name),
                                     newparent,
                                     PathComponentPiece(newname));
                               })
                               .then(
                                   []() { RequestData::get().replyError(0); }));
}

folly::Future<fuse_entry_param>
    Dispatcher::link(fuse_ino_t, fuse_ino_t, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

static void disp_link(fuse_req_t req,
                      fuse_ino_t ino,
                      fuse_ino_t newparent,
                      const char* newname) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().link)
          .then([=, &request] {
            return dispatcher->link(
                ino, newparent, PathComponentPiece(newname));
          })
          .then([](fuse_entry_param&& param) {
            RequestData::get().replyEntry(param);
          }));
}

folly::Future<std::shared_ptr<FileHandle>> Dispatcher::open(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  FUSELL_NOT_IMPL();
}

static void disp_open(fuse_req_t req,
                      fuse_ino_t ino,
                      struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().open)
          .then([ =, &request, fi = *fi ] { return dispatcher->open(ino, fi); })
          .then([ req, dispatcher, orig_info = *fi ](
              std::shared_ptr<FileHandle> fh) {
            if (!fh) {
              throw std::runtime_error("Dispatcher::open failed to set fh");
            }
            fuse_file_info fi = orig_info;
            fi.direct_io = fh->usesDirectIO();
            fi.keep_cache = fh->preserveCache();
#if FUSE_MINOR_VERSION >= 8
            fi.nonseekable = !fh->isSeekable();
#endif
            fi.fh = dispatcher->getFileHandles().recordHandle(std::move(fh));
            if (!RequestData::get().replyOpen(fi)) {
              // Was interrupted, tidy up.
              dispatcher->getFileHandles().forgetGenericHandle(fi.fh);
            }
          }));
}

static void disp_read(fuse_req_t req,
                      fuse_ino_t ino,
                      size_t size,
                      off_t off,
                      struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(request.startRequest(dispatcher->getStats().read)
                               .then([ =, &request, fi = *fi ] {
                                 auto fh = dispatcher->getFileHandle(fi.fh);
                                 return fh->read(size, off);
                               })
                               .then([](BufVec&& buf) {
                                 auto iov = buf.getIov();
                                 RequestData::get().replyIov(
                                     iov.data(), iov.size());
                               }));
}

static void disp_write(fuse_req_t req,
                       fuse_ino_t ino,
                       const char* buf,
                       size_t size,
                       off_t off,
                       struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().write)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getFileHandle(fi.fh);

            return fh->write(folly::StringPiece(buf, size), off);
          })
          .then([](size_t wrote) { RequestData::get().replyWrite(wrote); }));
}

static void disp_flush(fuse_req_t req,
                       fuse_ino_t ino,
                       struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().flush)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getFileHandle(fi.fh);

            return fh->flush(fi.lock_owner);
          })
          .then([]() { RequestData::get().replyError(0); }));
}

static void disp_release(fuse_req_t req,
                         fuse_ino_t ino,
                         struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().release)
          .then([ =, &request, fi = *fi ] {
            dispatcher->getFileHandles().forgetGenericHandle(fi.fh);
            RequestData::get().replyError(0);
          }));
}

static void disp_fsync(fuse_req_t req,
                       fuse_ino_t ino,
                       int datasync,
                       struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().fsync)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getFileHandle(fi.fh);
            return fh->fsync(datasync);
          })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<std::shared_ptr<DirHandle>> Dispatcher::opendir(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  FUSELL_NOT_IMPL();
}

static void disp_opendir(fuse_req_t req,
                         fuse_ino_t ino,
                         struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().opendir)
          .then([ =, &request, fi = *fi ] {
            return dispatcher->opendir(ino, fi);
          })
          .then([ dispatcher, orig_info = *fi ](std::shared_ptr<DirHandle> dh) {
            if (!dh) {
              throw std::runtime_error("Dispatcher::opendir failed to set dh");
            }
            fuse_file_info fi = orig_info;
            fi.fh = dispatcher->getFileHandles().recordHandle(std::move(dh));
            if (!RequestData::get().replyOpen(fi)) {
              // Was interrupted, tidy up
              dispatcher->getFileHandles().forgetGenericHandle(fi.fh);
            }
          }));
}

static void disp_readdir(fuse_req_t req,
                         fuse_ino_t ino,
                         size_t size,
                         off_t off,
                         struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(request.startRequest(dispatcher->getStats().readdir)
                               .then([ =, &request, fi = *fi ] {
                                 auto dh = dispatcher->getDirHandle(fi.fh);
                                 return dh->readdir(DirList(size), off);
                               })
                               .then([](DirList&& list) {
                                 auto buf = list.getBuf();
                                 RequestData::get().replyBuf(
                                     buf.data(), buf.size());
                               }));
}

static void disp_releasedir(fuse_req_t req,
                            fuse_ino_t ino,
                            struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().releasedir)
          .then([ =, &request, fi = *fi ] {
            dispatcher->getFileHandles().forgetGenericHandle(fi.fh);
            RequestData::get().replyError(0);
          }));
}

static void disp_fsyncdir(fuse_req_t req,
                          fuse_ino_t ino,
                          int datasync,
                          struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().fsyncdir)
          .then([ =, &request, fi = *fi ] {
            auto dh = dispatcher->getDirHandle(fi.fh);
            return dh->fsyncdir(datasync);
          })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<struct statvfs> Dispatcher::statfs(fuse_ino_t ino) {
  struct statvfs info;
  memset(&info, 0, sizeof(info));

  // Suggest a large blocksize to software that looks at that kind of thing
  info.f_bsize = getConnInfo().max_readahead;

  return info;
}

static void disp_statfs(fuse_req_t req, fuse_ino_t ino) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().statfs)
          .then([=, &request] { return dispatcher->statfs(ino); })
          .then([](struct statvfs&& info) {
            RequestData::get().replyStatfs(info);
          }));
}

folly::Future<folly::Unit> Dispatcher::setxattr(fuse_ino_t ino,
                                                folly::StringPiece name,
                                                folly::StringPiece value,
                                                int flags) {
  FUSELL_NOT_IMPL();
}

static void disp_setxattr(fuse_req_t req,
                          fuse_ino_t ino,
                          const char* name,
                          const char* value,
                          size_t size,
                          int flags
#ifdef __APPLE__
                          ,
                          uint32_t position
#endif
                          ) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();

#ifdef __APPLE__
  if (position != 0) {
    request.replyError(EINVAL);
    return;
  }
#endif

  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().setxattr)
          .then([=, &request] {
            return dispatcher->setxattr(
                ino, name, folly::StringPiece(value, size), flags);
          })
          .then([]() { RequestData::get().replyError(0); }));
}

const int Dispatcher::kENOATTR =
#ifndef ENOATTR
    ENODATA // Linux
#else
    ENOATTR
#endif
    ;

folly::Future<std::string> Dispatcher::getxattr(fuse_ino_t ino,
                                                folly::StringPiece name) {
  throwSystemErrorExplicit(kENOATTR);
}

static void disp_getxattr(fuse_req_t req,
                          fuse_ino_t ino,
                          const char* name,
                          size_t size
#ifdef __APPLE__
                          ,
                          uint32_t position
#endif
                          ) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();

#ifdef __APPLE__
  if (position != 0) {
    request.replyError(EINVAL);
    return;
  }
#endif

  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().getxattr)
          .then([=, &request] { return dispatcher->getxattr(ino, name); })
          .then([size](std::string attr) {
            auto& request = RequestData::get();
            if (size == 0) {
              request.replyXattr(attr.size());
            } else if (size < attr.size()) {
              request.replyError(ERANGE);
            } else {
              request.replyBuf(attr.data(), attr.size());
            }
          }));
}

folly::Future<std::vector<std::string>> Dispatcher::listxattr(fuse_ino_t ino) {
  return std::vector<std::string>();
}

static void disp_listxattr(fuse_req_t req, fuse_ino_t ino, size_t size) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().listxattr)
          .then([=, &request] { return dispatcher->listxattr(ino); })
          .then([size](std::vector<std::string>&& attrs) {
            auto& request = RequestData::get();

            // Initialize count to include the \0 for each
            // entry.
            size_t count = attrs.size();
            for (auto& attr : attrs) {
              count += attr.size();
            }

            if (size == 0) {
              request.replyXattr(count);
            } else if (size < count) {
              request.replyError(ERANGE);
            } else {
              std::string buf;
              folly::join('\0', attrs, buf);
              buf.push_back('\0');
              DCHECK(count == buf.size());
              request.replyBuf(buf.data(), count);
            }
          }));
}

folly::Future<folly::Unit> Dispatcher::removexattr(fuse_ino_t ino,
                                                   folly::StringPiece name) {
  FUSELL_NOT_IMPL();
}

static void disp_removexattr(fuse_req_t req, fuse_ino_t ino, const char* name) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().removexattr)
          .then([=, &request] { return dispatcher->removexattr(ino, name); })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<folly::Unit> Dispatcher::access(fuse_ino_t ino, int mask) {
  // Note that if you mount with the "default_permissions" kernel mount option,
  // the kernel will perform all permissions checks for you, and will never
  // invoke access() directly.
  //
  // Implementing access() is only needed when not using the
  // "default_permissions" option.
  FUSELL_NOT_IMPL();
}

static void disp_access(fuse_req_t req, fuse_ino_t ino, int mask) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().access)
          .then([=, &request] { return dispatcher->access(ino, mask); })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<Dispatcher::Create>
Dispatcher::create(fuse_ino_t, PathComponentPiece, mode_t, int) {
  FUSELL_NOT_IMPL();
}

static void disp_create(fuse_req_t req,
                        fuse_ino_t parent,
                        const char* name,
                        mode_t mode,
                        struct fuse_file_info* fi) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().create)
          .then([ =, &request, fi = *fi ] {
            return dispatcher->create(
                parent, PathComponentPiece(name), mode, fi.flags);
          })
          .then([ dispatcher, orig_info = *fi ](Dispatcher::Create info) {
            fuse_file_info fi = orig_info;
            fi.direct_io = info.fh->usesDirectIO();
            fi.keep_cache = info.fh->preserveCache();
#if FUSE_MINOR_VERSION >= 8
            fi.nonseekable = !info.fh->isSeekable();
#endif
            fi.fh =
                dispatcher->getFileHandles().recordHandle(std::move(info.fh));
            if (!RequestData::get().replyCreate(info.entry, fi)) {
              // Interrupted, tidy up
              dispatcher->getFileHandles().forgetGenericHandle(fi.fh);
            }
          }));
}

static void disp_getlk(fuse_req_t req,
                       fuse_ino_t ino,
                       struct fuse_file_info* fi,
                       struct flock* lock) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().getlk)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getFileHandle(fi.fh);
            return fh->getlk(*lock, fi.lock_owner);
          })
          .then([](struct flock lock) { RequestData::get().replyLock(lock); }));
}

static void disp_setlk(fuse_req_t req,
                       fuse_ino_t ino,
                       struct fuse_file_info* fi,
                       struct flock* lock,
                       int sleep) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().setlk)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getFileHandle(fi.fh);

            return fh->setlk(*lock, sleep, fi.lock_owner);
          })
          .then([]() { RequestData::get().replyError(0); }));
}

folly::Future<uint64_t> Dispatcher::bmap(fuse_ino_t ino,
                                         size_t blocksize,
                                         uint64_t idx) {
  FUSELL_NOT_IMPL();
}

static void disp_bmap(fuse_req_t req,
                      fuse_ino_t ino,
                      size_t blocksize,
                      uint64_t idx) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().bmap)
          .then([=, &request] { return dispatcher->bmap(ino, blocksize, idx); })
          .then([](uint64_t idx) { RequestData::get().replyBmap(idx); }));
}

#if FUSE_MINOR_VERSION >= 8
static void disp_ioctl(fuse_req_t req,
                       fuse_ino_t ino,
                       int cmd,
                       void* arg,
                       struct fuse_file_info* fi,
                       unsigned flags,
                       const void* in_buf,
                       size_t in_bufsz,
                       size_t out_bufsz) {

  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();

  if (flags & FUSE_IOCTL_UNRESTRICTED) {
    // We only support restricted ioctls
    request.replyError(-EPERM);
    return;
  }

  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().ioctl)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getGenericFileHandle(fi.fh);

            return fh->ioctl(cmd,
                             arg,
                             folly::ByteRange((uint8_t*)in_buf, in_bufsz),
                             out_bufsz);
          })
          .then([](FileHandleBase::Ioctl&& result) {
            auto iov = result.buf.getIov();
            RequestData::get().replyIoctl(
                result.result, iov.data(), iov.size());
          }));
}
#endif

#if FUSE_MINOR_VERSION >= 8
static void disp_poll(fuse_req_t req,
                      fuse_ino_t ino,
                      struct fuse_file_info* fi,
                      struct fuse_pollhandle* ph) {
  auto& request = RequestData::create(req);
  auto* dispatcher = request.getDispatcher();
  request.setRequestFuture(
      request.startRequest(dispatcher->getStats().poll)
          .then([ =, &request, fi = *fi ] {
            auto fh = dispatcher->getGenericFileHandle(fi.fh);

            std::unique_ptr<PollHandle> poll_handle;
            if (ph) {
              poll_handle = std::make_unique<PollHandle>(ph);
            }

            return fh->poll(std::move(poll_handle));
          })
          .then(
              [](unsigned revents) { RequestData::get().replyPoll(revents); }));
}
#endif

static const fuse_lowlevel_ops dispatcher_ops = {
    .init = Dispatcher::disp_init,
    .destroy = disp_destroy,
    .lookup = disp_lookup,
    .forget = disp_forget,
    .getattr = disp_getattr,
    .setattr = disp_setattr,
    .readlink = disp_readlink,
    .mknod = disp_mknod,
    .mkdir = disp_mkdir,
    .unlink = disp_unlink,
    .rmdir = disp_rmdir,
    .symlink = disp_symlink,
    .rename = disp_rename,
    .link = disp_link,
    .open = disp_open,
    .read = disp_read,
    .write = disp_write,
    .flush = disp_flush,
    .release = disp_release,
    .fsync = disp_fsync,
    .opendir = disp_opendir,
    .readdir = disp_readdir,
    .releasedir = disp_releasedir,
    .fsyncdir = disp_fsyncdir,
    .statfs = disp_statfs,
    .setxattr = disp_setxattr,
    .getxattr = disp_getxattr,
    .listxattr = disp_listxattr,
    .removexattr = disp_removexattr,
    .access = disp_access,
    .create = disp_create,
    .getlk = disp_getlk,
    .setlk = disp_setlk,
    .bmap = disp_bmap,
#if FUSE_MINOR_VERSION >= 8
    .ioctl = disp_ioctl,
    .poll = disp_poll,
#endif
#if FUSE_MINOR_VERSION >= 9
    .forget_multi = disp_forget_multi,
#endif
};

const fuse_conn_info& Dispatcher::getConnInfo() const { return connInfo_; }

Channel& Dispatcher::getChannel() const {
  DCHECK(chan_ != nullptr) << "Channel not yet assigned!?";
  return *chan_;
}

EdenStats& Dispatcher::getStats() {
  return stats_;
}

const EdenStats& Dispatcher::getStats() const {
  return stats_;
}

std::unique_ptr<fuse_session, SessionDeleter> Dispatcher::makeSession(
    Channel& channel,
    bool debug) {
  chan_ = &channel;

  // libfuse may decide to mutate these arguments when we call fuse_lowlevel_new
  // so we use fuse_opt_add_arg() to mutate it.  Start with a well-defined
  // initial state.
  fuse_args fargs{0, nullptr, 0};
  SCOPE_EXIT {
    // Ensure that the allocations associated with fargs are released when
    // we exit this function.
    fuse_opt_free_args(&fargs);
  };

  // Each of these calls will duplicate the input string and expand the storage
  // in fargs.
  fuse_opt_add_arg(&fargs, "fuse");
  fuse_opt_add_arg(&fargs, "-o");
  fuse_opt_add_arg(&fargs, "allow_root");
  if (debug) {
    fuse_opt_add_arg(&fargs, "-d");
  }

  auto sess =
      fuse_lowlevel_new(&fargs, &dispatcher_ops, sizeof(dispatcher_ops), this);
  if (!sess) {
    throw std::runtime_error("failed to create session");
  }
  return std::unique_ptr<fuse_session, SessionDeleter>(sess,
                                                       SessionDeleter(chan_));
}
}
}
}
