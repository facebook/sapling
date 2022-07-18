/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/FuseChannel.h"

#include <boost/cast.hpp>
#include <fmt/core.h>
#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <signal.h>
#include <chrono>
#include <type_traits>
#include "eden/common/utils/Synchronized.h"
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/FuseDispatcher.h"
#include "eden/fs/fuse/FuseRequestContext.h"
#include "eden/fs/telemetry/FsEventLogger.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/IDGen.h"
#include "eden/fs/utils/StaticAssert.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/Thread.h"

using namespace folly;
using std::string;

namespace facebook::eden {

namespace {

/**
 * For most FUSE requests, the protocol is simple: an optional request
 * parameters struct followed by zero or more null-terminated strings. Provide
 * handy range-checking parsers.
 */
struct FuseArg {
  /* implicit */ FuseArg(ByteRange arg) : range{arg} {}

  /**
   * Reads a trivial struct or primitive of type T from the ByteRange and
   * advances the internal pointer.
   *
   * Throws std::out_of_range if not enough space remaining.
   */
  template <typename T>
  const T& read() {
    static_assert(std::is_trivial_v<T>);
    XCHECK_EQ(0u, reinterpret_cast<uintptr_t>(range.data()) % alignof(T))
        << "unaligned struct data";
    const void* data = range.data();
    // Throws std::out_of_range if too small.
    range.advance(sizeof(T));
    return *static_cast<const T*>(data);
  }

  /**
   * Reads a null-terminated from the ByteRange.
   *
   * Throws std::out_of_range if not enough space remaining.
   */
  folly::StringPiece readz() {
    const char* data = reinterpret_cast<const char*>(range.data());
    size_t length = strnlen(data, range.size());
    if (UNLIKELY(length == range.size())) {
      throw_exception<std::out_of_range>(
          "no null terminator in remaining bytes");
    }
    range.advance(length);
    return StringPiece{data, data + length};
  }

 private:
  folly::ByteRange range;
};

namespace argrender {

using RenderFn = std::string (&)(FuseArg arg);

std::string default_render(FuseArg /*arg*/) {
  return {};
}

std::string single_string_render(FuseArg arg) {
  auto name = arg.readz();
  return name.str();
}

constexpr RenderFn lookup = single_string_render;
constexpr RenderFn forget = default_render;
constexpr RenderFn getattr = default_render;
constexpr RenderFn setattr = default_render;
constexpr RenderFn readlink = default_render;

std::string symlink(FuseArg arg) {
  auto name = arg.readz();
  auto target = arg.readz();
  return fmt::format("name={}, target={}", name, target);
}

std::string mknod(FuseArg arg) {
  auto& in = arg.read<fuse_mknod_in>();
  auto name = arg.readz();

  return fmt::format("{}, mode={:#o}, rdev={}", name, in.mode, in.rdev);
}

std::string mkdir(FuseArg arg) {
  auto& in = arg.read<fuse_mkdir_in>();
  auto name = arg.readz();
  auto mode = in.mode & ~in.umask;
  return fmt::format("{}, mode={:#o}", name, mode);
}

constexpr RenderFn unlink = single_string_render;
constexpr RenderFn rmdir = single_string_render;

std::string rename(FuseArg arg) {
  auto& in = arg.read<fuse_rename_in>();
  auto oldName = arg.readz();
  auto newName = arg.readz();
  return fmt::format("old={}, newdir={}, new={}", oldName, in.newdir, newName);
}

std::string link(FuseArg arg) {
  auto& in = arg.read<fuse_link_in>();
  auto newName = arg.readz();
  return fmt::format("oldParent={}, newName={}", in.oldnodeid, newName);
}

constexpr RenderFn open = default_render;

std::string read(FuseArg arg) {
  auto& in = arg.read<fuse_read_in>();
  return fmt::format("off={}, len={}", in.offset, in.size);
}

std::string write(FuseArg arg) {
  auto& in = arg.read<fuse_write_in>();
  return fmt::format("off={}, len={}", in.offset, in.size);
}

constexpr RenderFn statfs = default_render;
constexpr RenderFn release = default_render;
constexpr RenderFn fsync = default_render;

std::string setxattr(FuseArg arg) {
  auto& in = arg.read<fuse_setxattr_in>();
  (void)in;
  auto name = arg.readz();
  return fmt::format("name={}", name);
}

std::string getxattr(FuseArg arg) {
  auto& in = arg.read<fuse_getxattr_in>();
  (void)in;
  auto name = arg.readz();
  return fmt::format("name={}", name);
}

constexpr RenderFn listxattr = default_render;
constexpr RenderFn removexattr = default_render;
constexpr RenderFn flush = default_render;
constexpr RenderFn opendir = default_render;

std::string readdir(FuseArg arg) {
  auto& in = arg.read<fuse_read_in>();
  return fmt::format("offset={}", in.offset);
}

constexpr RenderFn releasedir = default_render;
constexpr RenderFn fsyncdir = default_render;

std::string access(FuseArg arg) {
  auto& in = arg.read<fuse_access_in>();
  return fmt::format("mask={}", in.mask);
}

std::string create(FuseArg arg) {
  auto& in = arg.read<fuse_create_in>();
  auto name = arg.readz();
  return fmt::format("name={}, mode={:#o}", name, in.mode);
}

constexpr RenderFn bmap = default_render;

std::string batchforget(FuseArg arg) {
  auto& in = arg.read<fuse_batch_forget_in>();
  // TODO: could print some specific inode values here
  return fmt::format("count={}", in.count);
}

std::string fallocate(FuseArg arg) {
  auto& in = arg.read<fuse_fallocate_in>();
  return fmt::format(
      "mode={}, offset={}, length={}", in.mode, in.offset, in.length);
}

} // namespace argrender

// These static asserts exist to make explicit the memory usage of the per-mount
// FUSE TraceBus. TraceBus uses 2 * capacity * sizeof(TraceEvent) memory usage,
// so limit total memory usage to around 4 MB per mount.
constexpr size_t kTraceBusCapacity = 25000;
static_assert(CheckSize<FuseTraceEvent, 72>());
static_assert(
    CheckEqual<1800000, kTraceBusCapacity * sizeof(FuseTraceEvent)>());

// This is the minimum size used by libfuse so we use it too!
constexpr size_t MIN_BUFSIZE = 0x21000;

using Handler = ImmediateFuture<folly::Unit> (FuseChannel::*)(
    FuseRequestContext& request,
    const fuse_in_header& header,
    folly::ByteRange arg);

using FuseArgRenderer = std::string (*)(FuseArg arg);

using AccessType = ProcessAccessLog::AccessType;

struct HandlerEntry {
  constexpr HandlerEntry() = default;
  /*implicit*/ constexpr HandlerEntry(StringPiece n) : name{n} {}
  constexpr HandlerEntry(StringPiece n, AccessType at)
      : name{n}, accessType{at} {}
  constexpr HandlerEntry(
      StringPiece n,
      Handler h,
      FuseArgRenderer r,
      ChannelThreadStats::StatPtr s,
      AccessType at = AccessType::FsChannelOther,
      SamplingGroup samplingGroup = SamplingGroup::DropAll)
      : name{n},
        handler{h},
        argRenderer{r},
        stat{s},
        samplingGroup{samplingGroup},
        accessType{at} {}

  std::string getShortName() const {
    if (name.startsWith("FUSE_")) {
      std::string rv;
      rv.reserve(name.size() - 5);
      for (const char* p = name.begin() + 5; p != name.end(); ++p) {
        char c = *p;
        if (c == '_') {
          continue;
        }
        rv.push_back((c >= 'A' && c <= 'Z') ? c - 'A' + 'a' : c);
      }
      return rv;
    } else {
      // We shouldn't hit CUSE ops, so be explicit and return the entire
      // capitalized name.
      return name.str();
    }
  }

  StringPiece name;
  Handler handler = nullptr;
  FuseArgRenderer argRenderer = nullptr;
  ChannelThreadStats::StatPtr stat = nullptr;
  SamplingGroup samplingGroup = SamplingGroup::DropAll;
  AccessType accessType = AccessType::FsChannelOther;
};

constexpr auto kFuseHandlers = [] {
  const auto Read = AccessType::FsChannelRead;
  const auto Write = AccessType::FsChannelWrite;

  // Rely on assignment out of bounds to a constexpr array giving a
  // compiler error.
  std::array<HandlerEntry, 64> handlers;
  handlers[FUSE_LOOKUP] = {
      "FUSE_LOOKUP",
      &FuseChannel::fuseLookup,
      &argrender::lookup,
      &ChannelThreadStats::lookup,
      Read,
      SamplingGroup::Four};
  handlers[FUSE_FORGET] = {
      "FUSE_FORGET",
      &FuseChannel::fuseForget,
      &argrender::forget,
      &ChannelThreadStats::forget};
  handlers[FUSE_GETATTR] = {
      "FUSE_GETATTR",
      &FuseChannel::fuseGetAttr,
      &argrender::getattr,
      &ChannelThreadStats::getattr,
      Read,
      SamplingGroup::Three};
  handlers[FUSE_SETATTR] = {
      "FUSE_SETATTR",
      &FuseChannel::fuseSetAttr,
      &argrender::setattr,
      &ChannelThreadStats::setattr,
      Write,
      SamplingGroup::Two};
  handlers[FUSE_READLINK] = {
      "FUSE_READLINK",
      &FuseChannel::fuseReadLink,
      &argrender::readlink,
      &ChannelThreadStats::readlink,
      Read};
  handlers[FUSE_SYMLINK] = {
      "FUSE_SYMLINK",
      &FuseChannel::fuseSymlink,
      &argrender::symlink,
      &ChannelThreadStats::symlink,
      Write};
  handlers[FUSE_MKNOD] = {
      "FUSE_MKNOD",
      &FuseChannel::fuseMknod,
      &argrender::mknod,
      &ChannelThreadStats::mknod,
      Write};
  handlers[FUSE_MKDIR] = {
      "FUSE_MKDIR",
      &FuseChannel::fuseMkdir,
      &argrender::mkdir,
      &ChannelThreadStats::mkdir,
      Write,
      SamplingGroup::One};
  handlers[FUSE_UNLINK] = {
      "FUSE_UNLINK",
      &FuseChannel::fuseUnlink,
      &argrender::unlink,
      &ChannelThreadStats::unlink,
      Write};
  handlers[FUSE_RMDIR] = {
      "FUSE_RMDIR",
      &FuseChannel::fuseRmdir,
      &argrender::rmdir,
      &ChannelThreadStats::rmdir,
      Write,
      SamplingGroup::One};
  handlers[FUSE_RENAME] = {
      "FUSE_RENAME",
      &FuseChannel::fuseRename,
      &argrender::rename,
      &ChannelThreadStats::rename,
      Write,
      SamplingGroup::One};
  handlers[FUSE_LINK] = {
      "FUSE_LINK",
      &FuseChannel::fuseLink,
      &argrender::link,
      &ChannelThreadStats::link,
      Write};
  handlers[FUSE_OPEN] = {
      "FUSE_OPEN",
      &FuseChannel::fuseOpen,
      &argrender::open,
      &ChannelThreadStats::open};
  handlers[FUSE_READ] = {
      "FUSE_READ",
      &FuseChannel::fuseRead,
      &argrender::read,
      &ChannelThreadStats::read,
      Read,
      SamplingGroup::Three};
  handlers[FUSE_WRITE] = {
      "FUSE_WRITE",
      &FuseChannel::fuseWrite,
      &argrender::write,
      &ChannelThreadStats::write,
      Write,
      SamplingGroup::Two};
  handlers[FUSE_STATFS] = {
      "FUSE_STATFS",
      &FuseChannel::fuseStatFs,
      &argrender::statfs,
      &ChannelThreadStats::statfs,
      Read};
  handlers[FUSE_RELEASE] = {
      "FUSE_RELEASE",
      &FuseChannel::fuseRelease,
      &argrender::release,
      &ChannelThreadStats::release};
  handlers[FUSE_FSYNC] = {
      "FUSE_FSYNC",
      &FuseChannel::fuseFsync,
      &argrender::fsync,
      &ChannelThreadStats::fsync,
      Write};
  handlers[FUSE_SETXATTR] = {
      "FUSE_SETXATTR",
      &FuseChannel::fuseSetXAttr,
      &argrender::setxattr,
      &ChannelThreadStats::setxattr,
      Write};
  handlers[FUSE_GETXATTR] = {
      "FUSE_GETXATTR",
      &FuseChannel::fuseGetXAttr,
      &argrender::getxattr,
      &ChannelThreadStats::getxattr,
      Read,
      SamplingGroup::Three};
  handlers[FUSE_LISTXATTR] = {
      "FUSE_LISTXATTR",
      &FuseChannel::fuseListXAttr,
      &argrender::listxattr,
      &ChannelThreadStats::listxattr,
      Read,
      SamplingGroup::Two};
  handlers[FUSE_REMOVEXATTR] = {
      "FUSE_REMOVEXATTR",
      &FuseChannel::fuseRemoveXAttr,
      &argrender::removexattr,
      &ChannelThreadStats::removexattr,
      Write};
  handlers[FUSE_FLUSH] = {
      "FUSE_FLUSH",
      &FuseChannel::fuseFlush,
      &argrender::flush,
      &ChannelThreadStats::flush};
  handlers[FUSE_INIT] = {"FUSE_INIT"};
  handlers[FUSE_OPENDIR] = {
      "FUSE_OPENDIR",
      &FuseChannel::fuseOpenDir,
      &argrender::opendir,
      &ChannelThreadStats::opendir};
  handlers[FUSE_READDIR] = {
      "FUSE_READDIR",
      &FuseChannel::fuseReadDir,
      &argrender::readdir,
      &ChannelThreadStats::readdir,
      Read,
      SamplingGroup::Three};
  handlers[FUSE_RELEASEDIR] = {
      "FUSE_RELEASEDIR",
      &FuseChannel::fuseReleaseDir,
      &argrender::releasedir,
      &ChannelThreadStats::releasedir};
  handlers[FUSE_FSYNCDIR] = {
      "FUSE_FSYNCDIR",
      &FuseChannel::fuseFsyncDir,
      &argrender::fsyncdir,
      &ChannelThreadStats::fsyncdir,
      Write};
  handlers[FUSE_GETLK] = {"FUSE_GETLK"};
  handlers[FUSE_SETLK] = {"FUSE_SETLK"};
  handlers[FUSE_SETLKW] = {"FUSE_SETLKW"};
  handlers[FUSE_ACCESS] = {
      "FUSE_ACCESS",
      &FuseChannel::fuseAccess,
      &argrender::access,
      &ChannelThreadStats::access,
      Read};
  handlers[FUSE_CREATE] = {
      "FUSE_CREATE",
      &FuseChannel::fuseCreate,
      &argrender::create,
      &ChannelThreadStats::create,
      Write,
      SamplingGroup::One};
  handlers[FUSE_INTERRUPT] = {"FUSE_INTERRUPT"};
  handlers[FUSE_BMAP] = {
      "FUSE_BMAP",
      &FuseChannel::fuseBmap,
      &argrender::bmap,
      &ChannelThreadStats::bmap};
  handlers[FUSE_DESTROY] = {"FUSE_DESTROY"};
  handlers[FUSE_IOCTL] = {"FUSE_IOCTL"};
  handlers[FUSE_POLL] = {"FUSE_POLL"};
  handlers[FUSE_NOTIFY_REPLY] = {"FUSE_NOTIFY_REPLY"};
  handlers[FUSE_BATCH_FORGET] = {
      "FUSE_BATCH_FORGET",
      &FuseChannel::fuseBatchForget,
      &argrender::batchforget,
      &ChannelThreadStats::forgetmulti};
  handlers[FUSE_FALLOCATE] = {
      "FUSE_FALLOCATE",
      &FuseChannel::fuseFallocate,
      &argrender::fallocate,
      &ChannelThreadStats::fallocate,
      Write};
#ifdef __linux__
  handlers[FUSE_READDIRPLUS] = {"FUSE_READDIRPLUS", Read};
  handlers[FUSE_RENAME2] = {"FUSE_RENAME2", Write};
  handlers[FUSE_LSEEK] = {"FUSE_LSEEK"};
  handlers[FUSE_COPY_FILE_RANGE] = {"FUSE_COPY_FILE_RANGE", Write};
  handlers[FUSE_SETUPMAPPING] = {"FUSE_SETUPMAPPING", Read};
  handlers[FUSE_REMOVEMAPPING] = {"FUSE_REMOVEMAPPING", Read};
#endif
#ifdef __APPLE__
  handlers[FUSE_SETVOLNAME] = {"FUSE_SETVOLNAME", Write};
  handlers[FUSE_GETXTIMES] = {"FUSE_GETXTIMES", Read};
  handlers[FUSE_EXCHANGE] = {"FUSE_EXCHANGE", Write};
#endif
  return handlers;
}();

// Separate to avoid bloating the FUSE opcode table; CUSE_INIT is 4096.
constexpr HandlerEntry kCuseInitHandler{"CUSE_INIT"};

constexpr const HandlerEntry* lookupFuseHandlerEntry(uint32_t opcode) {
  if (CUSE_INIT == opcode) {
    return &kCuseInitHandler;
  }
  if (opcode >= std::size(kFuseHandlers)) {
    return nullptr;
  }
  auto& entry = kFuseHandlers[opcode];
  return entry.name.empty() ? nullptr : &entry;
}

constexpr std::pair<uint32_t, const char*> kCapsLabels[] = {
    {FUSE_ASYNC_READ, "ASYNC_READ"},
    {FUSE_POSIX_LOCKS, "POSIX_LOCKS"},
    {FUSE_ATOMIC_O_TRUNC, "ATOMIC_O_TRUNC"},
    {FUSE_EXPORT_SUPPORT, "EXPORT_SUPPORT"},
    {FUSE_BIG_WRITES, "BIG_WRITES"},
    {FUSE_DONT_MASK, "DONT_MASK"},
    {FUSE_FLOCK_LOCKS, "FLOCK_LOCKS"},
#ifdef __linux__
    {FUSE_SPLICE_WRITE, "SPLICE_WRITE"},
    {FUSE_SPLICE_MOVE, "SPLICE_MOVE"},
    {FUSE_SPLICE_READ, "SPLICE_READ"},
    {FUSE_HAS_IOCTL_DIR, "IOCTL_DIR"},
    {FUSE_AUTO_INVAL_DATA, "AUTO_INVAL_DATA"},
    {FUSE_DO_READDIRPLUS, "DO_READDIRPLUS"},
    {FUSE_READDIRPLUS_AUTO, "READDIRPLUS_AUTO"},
    {FUSE_ASYNC_DIO, "ASYNC_DIO"},
    {FUSE_WRITEBACK_CACHE, "WRITEBACK_CACHE"},
    {FUSE_PARALLEL_DIROPS, "PARALLEL_DIROPS"},
    {FUSE_HANDLE_KILLPRIV, "HANDLE_KILLPRIV"},
    {FUSE_POSIX_ACL, "POSIX_ACL"},
    {FUSE_ABORT_ERROR, "ABORT_ERROR"},
    {FUSE_MAX_PAGES, "MAX_PAGES"},
    {FUSE_CACHE_SYMLINKS, "CACHE_SYMLINKS"},
    {FUSE_EXPLICIT_INVAL_DATA, "EXPLICIT_INVAL_DATA"},
#endif
#ifdef __APPLE__
    {FUSE_ALLOCATE, "ALLOCATE"},
    {FUSE_EXCHANGE_DATA, "EXCHANGE_DATA"},
    {FUSE_CASE_INSENSITIVE, "CASE_INSENSITIVE"},
    {FUSE_VOL_RENAME, "VOL_RENAME"},
    {FUSE_XTIMES, "XTIMES"},
#endif
#ifdef FUSE_NO_OPEN_SUPPORT
    {FUSE_NO_OPEN_SUPPORT, "NO_OPEN_SUPPORT"},
#endif
#ifdef FUSE_NO_OPENDIR_SUPPORT
    {FUSE_NO_OPENDIR_SUPPORT, "NO_OPENDIR_SUPPORT"},
#endif
};

std::string capsFlagsToLabel(uint32_t flags) {
  std::vector<const char*> bits;
  bits.reserve(std::size(kCapsLabels));
  for (const auto& [flag, name] : kCapsLabels) {
    if (flag == 0) {
      // Sometimes a define evaluates to zero; it's not useful so skip it
      continue;
    }
    if ((flags & flag) == flag) {
      bits.push_back(name);
      flags &= ~flag;
    }
  }
  std::string str;
  folly::join(" ", bits, str);
  if (flags == 0) {
    return str;
  }
  return fmt::format("{} unknown:0x{:x}", str, flags);
}

void sigusr2Handler(int /* signum */) {
  // Do nothing.
  // The purpose of this signal is only to interrupt the blocking read() calls
  // in processSession() and readInitPacket()
}

void installSignalHandler() {
  // We use SIGUSR2 to wake up our worker threads when we want to shut down.
  // Install a signal handler for this signal.  The signal handler itself is a
  // no-op, we simply want to use it to interrupt blocking read() calls.
  //
  // We will re-install this handler each time a FuseChannel object is called,
  // but that should be fine.
  //
  // This must be installed using sigaction() rather than signal(), so we can
  // ensure that the SA_RESTART flag is not ste.
  struct sigaction action = {};
  action.sa_handler = sigusr2Handler;
  sigemptyset(&action.sa_mask);
  action.sa_flags = 0; // We intentionally turn off SA_RESTART
  struct sigaction oldAction;
  folly::checkUnixError(
      sigaction(SIGUSR2, &action, &oldAction), "failed to set SIGUSR2 handler");
}

template <typename T>
iovec make_iovec(const T& t) {
  static_assert(std::is_standard_layout_v<T>);
  static_assert(std::is_trivial_v<T>);
  iovec iov{};
  iov.iov_base = const_cast<T*>(&t);
  iov.iov_len = sizeof(t);
  return iov;
}

SamplingGroup fuseOpcodeSamplingGroup(uint32_t opcode) {
  auto* entry = lookupFuseHandlerEntry(opcode);
  return entry ? entry->samplingGroup : SamplingGroup::DropAll;
}

} // namespace

StringPiece fuseOpcodeName(uint32_t opcode) {
  auto* entry = lookupFuseHandlerEntry(opcode);
  return entry ? entry->name : "<unknown>";
}

ProcessAccessLog::AccessType fuseOpcodeAccessType(uint32_t opcode) {
  auto* entry = lookupFuseHandlerEntry(opcode);
  return entry ? entry->accessType
               : ProcessAccessLog::AccessType::FsChannelOther;
}

FuseChannel::DataRange::DataRange(int64_t off, int64_t len)
    : offset(off), length(len) {}

FuseChannel::InvalidationEntry::InvalidationEntry(
    InodeNumber num,
    PathComponentPiece n)
    : type(InvalidationType::DIR_ENTRY), inode(num), name(n) {}

FuseChannel::InvalidationEntry::InvalidationEntry(
    InodeNumber num,
    int64_t offset,
    int64_t length)
    : type(InvalidationType::INODE), inode(num), range(offset, length) {}

FuseChannel::InvalidationEntry::InvalidationEntry(Promise<Unit> p)
    : type(InvalidationType::FLUSH),
      inode(kRootNodeId),
      promise(std::move(p)) {}

FuseChannel::InvalidationEntry::~InvalidationEntry() {
  switch (type) {
    case InvalidationType::INODE:
      range.~DataRange();
      return;
    case InvalidationType::DIR_ENTRY:
      name.~PathComponent();
      return;
    case InvalidationType::FLUSH:
      promise.~Promise();
      return;
  }
  XLOG(FATAL) << "unknown InvalidationEntry type: "
              << static_cast<uint64_t>(type);
}

FuseChannel::InvalidationEntry::
    InvalidationEntry(InvalidationEntry&& other) noexcept(
        std::is_nothrow_move_constructible_v<PathComponent>&&
            std::is_nothrow_move_constructible_v<folly::Promise<folly::Unit>>&&
                std::is_nothrow_move_constructible_v<DataRange>)
    : type(other.type), inode(other.inode) {
  switch (type) {
    case InvalidationType::INODE:
      new (&range) DataRange(std::move(other.range));
      return;
    case InvalidationType::DIR_ENTRY:
      new (&name) PathComponent(std::move(other.name));
      return;
    case InvalidationType::FLUSH:
      new (&promise) Promise<Unit>(std::move(other.promise));
      return;
  }
}

std::ostream& operator<<(
    std::ostream& os,
    const FuseChannel::InvalidationEntry& entry) {
  switch (entry.type) {
    case FuseChannel::InvalidationType::INODE:
      return os << "(inode " << entry.inode << ", offset " << entry.range.offset
                << ", length " << entry.range.length << ")";
    case FuseChannel::InvalidationType::DIR_ENTRY:
      return os << "(inode " << entry.inode << ", child \"" << entry.name
                << "\")";
    case FuseChannel::InvalidationType::FLUSH:
      return os << "(invalidation flush)";
  }
  return os << "(unknown invalidation type "
            << static_cast<uint64_t>(entry.type) << " inode " << entry.inode
            << ")";
}

void FuseChannel::replyError(const fuse_in_header& request, int errorCode) {
  fuse_out_header err;
  err.len = sizeof(err);
  err.error = -errorCode;
  err.unique = request.unique;
  XLOG(DBG7) << "replyError unique=" << err.unique << " error=" << errorCode
             << " " << folly::errnoStr(errorCode);
  auto res = write(fuseDevice_.fd(), &err, sizeof(err));
  if (res != sizeof(err)) {
    if (res < 0) {
      throwSystemError("replyError: error writing to fuse device");
    } else {
      throw std::runtime_error("unexpected short write to FUSE device");
    }
  }
}

void FuseChannel::sendReply(
    const fuse_in_header& request,
    folly::fbvector<iovec>&& vec) const {
  fuse_out_header out;
  out.unique = request.unique;
  out.error = 0;

  vec.insert(vec.begin(), make_iovec(out));

  sendRawReply(vec.data(), vec.size());
}

void FuseChannel::sendReply(
    const fuse_in_header& request,
    const folly::IOBuf& buf) const {
  fuse_out_header out;
  out.unique = request.unique;
  out.error = 0;

  folly::fbvector<iovec> vec;
  vec.reserve(1 + buf.countChainElements());
  vec.push_back(make_iovec(out));
  buf.appendToIov(&vec);

  sendRawReply(vec.data(), vec.size());
}

void FuseChannel::sendReply(
    const fuse_in_header& request,
    folly::ByteRange bytes) const {
  fuse_out_header out;
  out.unique = request.unique;
  out.error = 0;

  std::array<iovec, 2> iov;
  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);
  iov[1].iov_base = const_cast<uint8_t*>(bytes.data());
  iov[1].iov_len = bytes.size();

  sendRawReply(iov.data(), iov.size());
}

void FuseChannel::sendRawReply(const iovec iov[], size_t count) const {
  // Ensure that the length is set correctly
  XDCHECK_EQ(iov[0].iov_len, sizeof(fuse_out_header));
  const auto header = reinterpret_cast<fuse_out_header*>(iov[0].iov_base);
  header->len = 0;
  for (size_t i = 0; i < count; ++i) {
    header->len += iov[i].iov_len;
  }

  const auto res = writev(fuseDevice_.fd(), iov, count);
  const int err = errno;
  XLOG(DBG7) << "sendRawReply: unique=" << header->unique
             << " header->len=" << header->len << " wrote=" << res;

  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal.  We don't need to log this,
      // but will propagate it back to our caller.
    } else if (!isFuseDeviceValid(state_.rlock()->stopReason)) {
      XLOG(INFO) << "error writing to fuse device: session closed";
    } else {
      XLOG(WARNING) << "error writing to fuse device: " << folly::errnoStr(err);
    }
    throwSystemErrorExplicit(err, "error writing to fuse device");
  }
}

FuseChannel::FuseChannel(
    folly::File&& fuseDevice,
    AbsolutePathPiece mountPath,
    size_t numThreads,
    std::unique_ptr<FuseDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<FsEventLogger> fsEventLogger,
    folly::Duration requestTimeout,
    std::shared_ptr<Notifier> notifier,
    CaseSensitivity caseSensitive,
    bool requireUtf8Path,
    int32_t maximumBackgroundRequests,
    bool useWriteBackCache)
    : bufferSize_(std::max(size_t(getpagesize()) + 0x1000, MIN_BUFSIZE)),
      numThreads_(numThreads),
      dispatcher_(std::move(dispatcher)),
      straceLogger_(straceLogger),
      mountPath_(mountPath),
      requestTimeout_(requestTimeout),
      notifier_(std::move(notifier)),
      caseSensitive_{caseSensitive},
      requireUtf8Path_{requireUtf8Path},
      maximumBackgroundRequests_{maximumBackgroundRequests},
      useWriteBackCache_{useWriteBackCache},
      fuseDevice_(std::move(fuseDevice)),
      processAccessLog_(std::move(processNameCache)),
      traceDetailedArguments_(std::make_shared<std::atomic<size_t>>(0)),
      traceBus_(TraceBus<FuseTraceEvent>::create(
          "FuseTrace" + mountPath.stringPiece().str(),
          kTraceBusCapacity)) {
  XCHECK_GE(numThreads_, 1ul);
  installSignalHandler();

  traceSubscriptionHandles_.push_back(traceBus_->subscribeFunction(
      "FuseChannel request tracking",
      [this,
       fsEventLogger = std::move(fsEventLogger)](const FuseTraceEvent& event) {
        switch (event.getType()) {
          case FuseTraceEvent::START: {
            auto state = telemetryState_.wlock();
            auto [iter, inserted] = state->requests.emplace(
                event.getUnique(),
                OutstandingRequest{
                    event.getUnique(),
                    event.getRequest(),
                    event.monotonicTime});
            XCHECK(inserted) << "duplicate fuse start event";
            break;
          }
          case FuseTraceEvent::FINISH: {
            std::chrono::nanoseconds durationNs{0};
            {
              auto state = telemetryState_.wlock();
              auto it = state->requests.find(event.getUnique());
              XCHECK(it != state->requests.end())
                  << "duplicate fuse finish event";
              durationNs = std::chrono::duration_cast<std::chrono::nanoseconds>(
                  event.monotonicTime - it->second.requestStartTime);
              state->requests.erase(it);
            }

            if (fsEventLogger) {
              auto opcode = event.getRequest().opcode;
              fsEventLogger->log({
                  durationNs,
                  fuseOpcodeSamplingGroup(opcode),
                  fuseOpcodeName(opcode),
              });
            }
            break;
          }
        }
      }));
}

FuseChannel::~FuseChannel() {
  XCHECK_EQ(1, traceBus_.use_count())
      << "This shared_ptr should not be copied; see attached comment.";
}

Future<FuseChannel::StopFuture> FuseChannel::initialize() {
  // Start one worker thread which will perform the initialization,
  // and will then start the remaining worker threads and signal success
  // once initialization completes.
  return folly::makeFutureWith([&] {
    auto state = state_.wlock();
    state->workerThreads.reserve(numThreads_);
    state->workerThreads.emplace_back([this] { initWorkerThread(); });
    return initPromise_.getFuture();
  });
}

FuseChannel::StopFuture FuseChannel::initializeFromTakeover(
    fuse_init_out connInfo) {
  connInfo_ = connInfo;
  dispatcher_->initConnection(connInfo);
  XLOG(DBG1) << "Takeover using max_write=" << connInfo_->max_write
             << ", max_readahead=" << connInfo_->max_readahead
             << ", want=" << capsFlagsToLabel(connInfo_->flags);
  startWorkerThreads();
  return sessionCompletePromise_.getFuture();
}

void FuseChannel::startWorkerThreads() {
  auto state = state_.wlock();

  // After aquiring the state_ lock check to see if we have been asked to shut
  // down.  If so just return without doing anything.
  //
  // This can happen if the FuseChannel is destroyed very shortly after we
  // finish processing the INIT request.  In this case we don't want to start
  // the remaining worker threads if the destructor is trying to stop and join
  // them.
  if (state->stopReason != StopReason::RUNNING) {
    return;
  }

  try {
    state->workerThreads.reserve(numThreads_);
    while (state->workerThreads.size() < numThreads_) {
      state->workerThreads.emplace_back([this] { fuseWorkerThread(); });
    }

    invalidationThread_ = std::thread([this] { invalidationThread(); });
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error starting FUSE worker threads: " << exceptionStr(ex);
    // Request any threads we did start to stop now.
    requestSessionExit(state, StopReason::INIT_FAILED);
    stopInvalidationThread();
    throw;
  }
}

void FuseChannel::destroy() {
  std::vector<std::thread> threads;
  {
    auto state = state_.wlock();
    requestSessionExit(state, StopReason::DESTRUCTOR);
    threads.swap(state->workerThreads);
  }

  for (auto& thread : threads) {
    if (std::this_thread::get_id() == thread.get_id()) {
      XLOG(FATAL) << "cannot destroy a FuseChannel from inside one of "
                     "its own worker threads";
    }
    thread.join();
  }

  // Check to see if there are still outstanding requests.
  // If so, delay actual deletion of the FuseChannel object until the
  // last request completes.
  bool allDone = false;
  {
    auto state = state_.wlock();
    if (state->pendingRequests == 0) {
      allDone = true;
    } else {
      state->destroyPending = true;
    }
  }
  if (allDone) {
    delete this;
  }
}

void FuseChannel::invalidateInode(InodeNumber ino, off_t off, off_t len) {
  // Add the entry to invalidationQueue_ and wake up the invalidation thread to
  // send it.
  invalidationQueue_.lock()->queue.emplace_back(ino, off, len);
  invalidationCV_.notify_one();
}

void FuseChannel::invalidateEntry(InodeNumber parent, PathComponentPiece name) {
  // Add the entry to invalidationQueue_ and wake up the invalidation thread to
  // send it.
  invalidationQueue_.lock()->queue.emplace_back(parent, name);
  invalidationCV_.notify_one();
}

void FuseChannel::invalidateInodes(folly::Range<InodeNumber*> range) {
  {
    auto queue = invalidationQueue_.lock();
    std::transform(
        range.begin(),
        range.end(),
        std::back_insert_iterator(queue->queue),
        [](const auto& inodeNum) { return InvalidationEntry(inodeNum, 0, 0); });
  }
  if (range.begin() != range.end()) {
    invalidationCV_.notify_one();
  }
}
folly::Future<folly::Unit> FuseChannel::flushInvalidations() {
  // Add a promise to the invalidation queue, which the invalidation thread
  // will fulfill once it reaches that element in the queue.
  Promise<Unit> promise;
  auto result = promise.getFuture();
  {
    auto state = invalidationQueue_.lock();
    if (state->stop) {
      // In the case of a concurrent unmount with a checkout, the unmount could
      // win the race and thus have shutdown the invalidation thread. This is
      // not an issue as the mount is gone at this point, let's thus return
      // immediately.
      return folly::unit;
    }
    state->queue.emplace_back(std::move(promise));
  }
  invalidationCV_.notify_one();
  return result;
}

/**
 * Send an element from the invalidation queue.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidation(InvalidationEntry& entry) {
  // We catch any exceptions that occur and simply log an error message.
  // There is not much else we can do in this situation.
  XLOG(DBG6) << "sending invalidation request: " << entry;
  try {
    switch (entry.type) {
      case InvalidationType::INODE:
        sendInvalidateInode(
            entry.inode, entry.range.offset, entry.range.length);
        return;
      case InvalidationType::DIR_ENTRY:
        sendInvalidateEntry(entry.inode, entry.name);
        return;
      case InvalidationType::FLUSH:
        // Fulfill the promise to indicate that all previous entries in the
        // invalidation queue have been completed.
        entry.promise.setValue();
        return;
    }
    EDEN_BUG() << "unknown invalidation entry type "
               << static_cast<uint64_t>(entry.type);
  } catch (const std::system_error& ex) {
    // Log ENOENT errors as a debug message.  This can happen for inode numbers
    // that we allocated on our own and haven't actually told the kernel about
    // yet.
    if (isEnoent(ex)) {
      XLOG(DBG3) << "received ENOENT when sending invalidation request: "
                 << entry;
    } else {
      XLOG(ERR) << "error sending invalidation request: " << entry << ": "
                << folly::exceptionStr(ex);
    }
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error sending invalidation request: " << entry << ": "
              << folly::exceptionStr(ex);
  }
}

/**
 * Send a FUSE_NOTIFY_INVAL_INODE message to the kernel.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidateInode(
    InodeNumber ino,
    int64_t off,
    int64_t len) {
  XLOG(DBG3) << "sendInvalidateInode(ino=" << ino << ", off=" << off
             << ", len=" << len << ")";
  fuse_notify_inval_inode_out notify;
  notify.ino = ino.get();
  notify.off = off;
  notify.len = len;

  fuse_out_header out;
  out.unique = 0;
  out.error = FUSE_NOTIFY_INVAL_INODE;

  std::array<iovec, 2> iov;

  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);

  iov[1].iov_base = &notify;
  iov[1].iov_len = sizeof(notify);

  try {
    sendRawReply(iov.data(), iov.size());
    XLOG(DBG7) << "sendInvalidateInode(ino=" << ino << ", off=" << off
               << ", len=" << len << ") OK!";
  } catch (const std::system_error& exc) {
    // Ignore ENOENT.  This can happen for inode numbers that we allocated on
    // our own and haven't actually told the kernel about yet.
    if (!isEnoent(exc)) {
      XLOG(ERR) << "sendInvalidateInode(ino=" << ino << ", off=" << off
                << ", len=" << len << ") failed: " << exc.what();
      throwSystemErrorExplicit(
          exc.code().value(), "error invalidating FUSE inode ", ino);
    } else {
      XLOG(DBG6) << "sendInvalidateInode(ino=" << ino << ", off=" << off
                 << ", len=" << len << ") failed with ENOENT";
    }
  }
}

/**
 * Send a FUSE_NOTIFY_INVAL_ENTRY message to the kernel.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidateEntry(
    InodeNumber parent,
    PathComponentPiece name) {
  XLOG(DBG3) << "sendInvalidateEntry(parent=" << parent << ", name=" << name
             << ")";

  auto namePiece = name.stringPiece();

  fuse_notify_inval_entry_out notify = {};
  notify.parent = parent.get();
  notify.namelen = namePiece.size();

  fuse_out_header out;
  out.unique = 0;
  out.error = FUSE_NOTIFY_INVAL_ENTRY;

  std::array<iovec, 4> iov;

  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);

  iov[1].iov_base = &notify;
  iov[1].iov_len = sizeof(notify);

  iov[2].iov_base = const_cast<char*>(namePiece.data());
  iov[2].iov_len = namePiece.size();

  // libfuse adds an extra 1 count to the size that it sends to the kernel,
  // presumably because it is assuming that the string is already NUL
  // terminated.  That is misleading because the API provides a size parameter
  // that implies that the string doesn't require termination.  We deal with
  // this more safely here by adding a vec element holding a NUL byte.
  iov[3].iov_base = const_cast<char*>("\x00");
  iov[3].iov_len = 1;

  try {
    sendRawReply(iov.data(), iov.size());
  } catch (const std::system_error& exc) {
    // Ignore ENOENT.  This can happen for inode numbers that we allocated on
    // our own and haven't actually told the kernel about yet.
    if (!isEnoent(exc)) {
      throwSystemErrorExplicit(
          exc.code().value(),
          "error invalidating FUSE entry ",
          name,
          " in directory inode ",
          parent);
    } else {
      XLOG(DBG3) << "sendInvalidateEntry(parent=" << parent << ", name=" << name
                 << ") failed with ENOENT";
    }
  }
}

std::vector<FuseChannel::OutstandingRequest>
FuseChannel::getOutstandingRequests() {
  std::vector<FuseChannel::OutstandingRequest> outstandingCalls;

  auto telemetryStateLockedPtr = telemetryState_.rlock();
  for (const auto& entry : telemetryStateLockedPtr->requests) {
    outstandingCalls.push_back(entry.second);
  }
  return outstandingCalls;
}

TraceDetailedArgumentsHandle FuseChannel::traceDetailedArguments() const {
  // We could implement something fancier here that just copies the shared_ptr
  // into a handle struct that increments upon taking ownership and decrements
  // on destruction, but this code path is quite rare, so do the expedient
  // thing.
  auto handle =
      std::shared_ptr<void>(nullptr, [copy = traceDetailedArguments_](void*) {
        copy->fetch_sub(1, std::memory_order_acq_rel);
      });
  traceDetailedArguments_->fetch_add(1, std::memory_order_acq_rel);
  return handle;
};

void FuseChannel::requestSessionExit(StopReason reason) {
  requestSessionExit(state_.wlock(), reason);
}

void FuseChannel::requestSessionExit(
    const Synchronized<State>::LockedPtr& state,
    StopReason reason) {
  // We have already been asked to stop before.
  if (state->stopReason != StopReason::RUNNING) {
    // Update state->stopReason only if the old stop reason left the FUSE
    // device in a still usable state but the new reason does not.
    if (isFuseDeviceValid(state->stopReason) &&
        !isFuseDeviceValid(state->stopReason)) {
      state->stopReason = reason;
    }
    return;
  }

  // This was the first time requestSessionExit has been called.
  // Record the reason we are stopping and then notify worker threads to
  // stop.
  state->stopReason = reason;

  // Update stop_ so that worker threads will break out of their loop.
  stop_.store(true, std::memory_order_relaxed);

  // Send a signal to knock our workers out of their blocking read() syscalls
  // TODO: This code is slightly racy, since threads could receive the signal
  // immediately before entering read().  In the long run it would be nicer to
  // have the worker threads use epoll and then use an eventfd to signal them
  // to stop.
  for (auto& thr : state->workerThreads) {
    if (thr.joinable() && thr.get_id() != std::this_thread::get_id()) {
      pthread_kill(thr.native_handle(), SIGUSR2);
    }
  }
}

void FuseChannel::setThreadSigmask() {
  // Make sure our thread will receive SIGUSR2
  sigset_t sigset;
  sigemptyset(&sigset);
  sigaddset(&sigset, SIGUSR2);

  sigset_t oldset;
  sigemptyset(&oldset);

  folly::checkPosixError(pthread_sigmask(SIG_UNBLOCK, &sigset, &oldset));
}

void FuseChannel::initWorkerThread() noexcept {
  try {
    setThreadSigmask();
    setThreadName(to<std::string>("fuse", mountPath_.basename()));

    // Read the INIT packet
    readInitPacket();

    // Start the other FUSE worker threads.
    startWorkerThreads();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error performing FUSE channel initialization: "
              << exceptionStr(ex);
    // Indicate that initialization failed.
    initPromise_.setException(
        folly::exception_wrapper(std::current_exception(), ex));
    return;
  }

  // Signal that initialization is complete.
  initPromise_.setValue(sessionCompletePromise_.getSemiFuture());

  // Continue to run like a normal FUSE worker thread.
  fuseWorkerThread();
}

void FuseChannel::fuseWorkerThread() noexcept {
  disablePthreadCancellation();
  setThreadName(to<std::string>("fuse", mountPath_.basename()));
  setThreadSigmask();
  *(liveRequestWatches_.get()) =
      std::make_shared<RequestMetricsScope::LockedRequestWatchList>();

  try {
    processSession();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "unexpected error in FUSE worker thread: " << exceptionStr(ex);
    // Request that all other FUSE threads exit.
    // This will cause us to stop processing the mount and signal our session
    // complete future.
    requestSessionExit(StopReason::WORKER_EXCEPTION);
    // Fall through and continue with the normal thread exit code.
  }

  // Record that we have shut down.
  {
    auto state = state_.wlock();
    ++state->stoppedThreads;
    XDCHECK(!state->destroyPending) << "destroyPending cannot be set while "
                                       "worker threads are still running";

    // If we are the last thread to stop and there are no more requests
    // outstanding then invoke sessionComplete().  If we are the last thread
    // but there are still outstanding requests we will invoke
    // sessionComplete() when we process the final stage of the request
    // processing for the last request.
    if (state->stoppedThreads == numThreads_ && state->pendingRequests == 0) {
      sessionComplete(std::move(state));
    }
  }
}

void FuseChannel::invalidationThread() noexcept {
  setThreadName(to<std::string>("inval", mountPath_.basename()));

  // We send all FUSE_NOTIFY_INVAL_ENTRY and FUSE_NOTIFY_INVAL_INODE requests
  // in a dedicated thread.  These requests will block in the kernel until it
  // can obtain the inode lock on the inode in question.
  //
  // It is possible that the kernel-level inode lock is already held by another
  // thread that is waiting on one of our own user-space locks.  To avoid
  // deadlock, we therefore need to make sure that we are never holding any
  // Eden locks when sending these invalidation requests.
  //
  // For example, a process calling unlink(parent_dir, "foo") will acquire the
  // inode lock for parent_dir in the kernel, and the kernel will then send an
  // unlink request to Eden.  This unlink request will require the mount
  // point's rename lock to proceed.  If a checkout is currently in progress it
  // currently owns the rename lock, and will generate invalidation requests.
  // We need to make sure the checkout operation does not block waiting on the
  // invalidation requests to complete, since otherwise this would deadlock.
  while (true) {
    // Wait for entries to process
    std::vector<InvalidationEntry> entries;
    {
      auto lockedQueue = invalidationQueue_.lock();
      while (lockedQueue->queue.empty()) {
        if (lockedQueue->stop) {
          return;
        }
        invalidationCV_.wait(lockedQueue.as_lock());
      }
      lockedQueue->queue.swap(entries);
    }

    // Process all of the entries we found
    for (auto& entry : entries) {
      sendInvalidation(entry);
    }
    entries.clear();
  }
}

void FuseChannel::stopInvalidationThread() {
  // Check that the thread is joinable just in case we were destroyed
  // before the invalidation thread was started.
  if (!invalidationThread_.joinable()) {
    return;
  }

  invalidationQueue_.lock()->stop = true;
  invalidationCV_.notify_one();
  invalidationThread_.join();
}

void FuseChannel::readInitPacket() {
  struct {
    fuse_in_header header;
    fuse_init_in init;
    // Starting in kernel 5.4 in
    // https://github.com/torvalds/linux/commit/1fb027d7596464d3fad3ed59f70f43807ef926c6
    // we have to request at least 8KB even for the init request
    char padding_[FUSE_MIN_READ_BUFFER];
  } init;

  // Loop until we receive the INIT packet, or until we are stopped.
  while (true) {
    if (stop_.load(std::memory_order_relaxed)) {
      throw_<std::runtime_error>(
          "FuseChannel for \"",
          mountPath_,
          "\" stopped while waiting for INIT packet");
    }

    auto res = read(fuseDevice_.fd(), &init, sizeof(init));
    if (res < 0) {
      int errnum = errno;
      if (stop_.load(std::memory_order_relaxed)) {
        throw_<std::runtime_error>(
            "FuseChannel for \"",
            mountPath_,
            "\" stopped while waiting for INIT packet");
      }

      if (errnum == EINTR || errnum == EAGAIN || errnum == ENOENT) {
        // These are all variations on being interrupted; let's
        // continue and retry.
        continue;
      }
      if (errnum == ENODEV) {
        throw FuseDeviceUnmountedDuringInitialization(mountPath_);
      }
      throw_<std::runtime_error>(
          "error reading from FUSE device for \"",
          mountPath_,
          "\" while expecting INIT request: ",
          folly::errnoStr(errnum));
    }
    if (res == 0) {
      // This is generally caused by the unit tests closing a fake fuse
      // channel.  When we are actually connected to the kernel we normally
      // expect to see an ENODEV error rather than EOF.
      throw FuseDeviceUnmountedDuringInitialization(mountPath_);
    }

    // Error out if the kernel sends less data than we expected.
    // We currently don't error out for now if we receive more data: maybe this
    // could happen for future kernel versions that speak a newer FUSE protocol
    // with extra fields in fuse_init_in?
    if (static_cast<size_t>(res) < sizeof(init) - sizeof(init.padding_)) {
      throw_<std::runtime_error>(
          "received partial FUSE_INIT packet on mount \"",
          mountPath_,
          "\": size=",
          res);
    }

    break;
  }

  if (init.header.opcode != FUSE_INIT) {
    replyError(init.header, EPROTO);
    throw_<std::runtime_error>(
        "expected to receive FUSE_INIT for \"",
        mountPath_,
        "\" but got ",
        fuseOpcodeName(init.header.opcode),
        " (",
        init.header.opcode,
        ")");
  }

  fuse_init_out connInfo = {};
  connInfo.major = init.init.major;
  connInfo.minor = init.init.minor;
  connInfo.max_write = bufferSize_ - 4096;
  connInfo.max_readahead = init.init.max_readahead;

  int32_t max_background = maximumBackgroundRequests_;
  if (max_background > 65535) {
    max_background = 65535;
  } else if (max_background < 0) {
    max_background = 0;
  }
  // The libfuse documentation says this only applies to background
  // requests like readahead prefetches and direct I/O, but we have
  // empirically observed that, on Linux, without setting this value,
  // `rg -j 200` limits the number of active FUSE requests to 16.
  connInfo.max_background = static_cast<uint32_t>(max_background);
  // Allow the kernel to default connInfo.congestion_threshold. Linux
  // picks 3/4 of max_background.

  const auto capable = init.init.flags;
  auto& want = connInfo.flags;

  // TODO: follow up and look at the new flags; particularly
  // FUSE_DO_READDIRPLUS, FUSE_READDIRPLUS_AUTO. FUSE_SPLICE_XXX are interesting
  // too, but may not directly benefit eden today.
  //
  // FUSE_ATOMIC_O_TRUNC is a nice optimization when the kernel supports it
  // and the FUSE daemon requires handling open/release for stateful file
  // handles. But FUSE_NO_OPEN_SUPPORT is superior, so edenfs has no need for
  // FUSE_ATOMIC_O_TRUNC. Also, on older kernels, it triggers a kernel bug.
  // See test_mmap_is_null_terminated_after_truncate_and_write_to_overlay
  // in mmap_test.py.

  // We can handle reads concurrently with any other type of request.
  want |= FUSE_ASYNC_READ;
  // We handle writes of any size.
  want |= FUSE_BIG_WRITES;

#ifdef __linux__
  // We don't support setuid and setgid mode bits anyway.
  want |= FUSE_HANDLE_KILLPRIV;
  // Allow the kernel to cache ACL xattrs, even though we will fail all setxattr
  // calls.
  want |= FUSE_POSIX_ACL;
  // We're happy to let the kernel cache readlink responses.
  want |= FUSE_CACHE_SYMLINKS;
  // We can handle almost any request in parallel.
  want |= FUSE_PARALLEL_DIROPS;
#endif

#ifdef FUSE_WRITEBACK_CACHE
  if (useWriteBackCache_) {
    // Writes go to the cache then write back to the underlying fs.
    want |= FUSE_WRITEBACK_CACHE;
  }
#else
  (void)useWriteBackCache_;
#endif

#ifdef FUSE_NO_OPEN_SUPPORT
  // File handles are stateless so the kernel does not need to send open() and
  // release().
  want |= FUSE_NO_OPEN_SUPPORT;
#endif
#ifdef FUSE_NO_OPENDIR_SUPPORT
  // File handles are stateless so the kernel does not need to send
  // open() and release().
  want |= FUSE_NO_OPENDIR_SUPPORT;
#endif
#ifdef FUSE_CASE_INSENSITIVE
  if (caseSensitive_ == CaseSensitivity::Insensitive) {
    want |= FUSE_CASE_INSENSITIVE;
  }
#else
  (void)caseSensitive_;
#endif

  // Only return the capabilities the kernel supports.
  want &= capable;

  XLOG(DBG1) << "Speaking fuse protocol kernel=" << init.init.major << "."
             << init.init.minor << " local=" << FUSE_KERNEL_VERSION << "."
             << FUSE_KERNEL_MINOR_VERSION << " on mount \"" << mountPath_
             << "\", max_write=" << connInfo.max_write
             << ", max_readahead=" << connInfo.max_readahead
             << ", capable=" << capsFlagsToLabel(capable)
             << ", want=" << capsFlagsToLabel(want);

  if (init.init.major != FUSE_KERNEL_VERSION) {
    replyError(init.header, EPROTO);
    throw_<std::runtime_error>(
        "Unsupported FUSE kernel version ",
        init.init.major,
        ".",
        init.init.minor,
        " while initializing \"",
        mountPath_,
        "\"");
  }

  // Update connInfo_
  // We have not started the other worker threads yet, so this is safe
  // to update without synchronization.
  connInfo_ = connInfo;

  // Send the INIT reply before informing the FuseDispatcher or signalling
  // initPromise_, so that the kernel will put the mount point in use and will
  // not block further filesystem access on us while running the FuseDispatcher
  // callback code.
#ifdef __linux__
  static_assert(
      FUSE_KERNEL_MINOR_VERSION > 22,
      "Your kernel headers are too old to build Eden.");
  if (init.init.minor > 22) {
    sendReply(init.header, connInfo);
  } else {
    // If the protocol version predates the expansion of fuse_init_out, only
    // send the start of the packet.
    static_assert(FUSE_COMPAT_22_INIT_OUT_SIZE <= sizeof(connInfo));
    sendReply(
        init.header,
        ByteRange{
            reinterpret_cast<const uint8_t*>(&connInfo),
            FUSE_COMPAT_22_INIT_OUT_SIZE});
  }
#elif defined(__APPLE__)
  static_assert(
      FUSE_KERNEL_MINOR_VERSION == 19,
      "osxfuse: API/ABI likely changed, may need something like the"
      " linux code above to send the correct response to the kernel");
  sendReply(init.header, connInfo);
#endif

  dispatcher_->initConnection(connInfo);
}

void FuseChannel::processSession() {
  std::vector<char> buf(bufferSize_);
  // Save this for the sanity check later in the loop to avoid
  // additional syscalls on each loop iteration.
  auto myPid = getpid();

  while (!stop_.load(std::memory_order_relaxed)) {
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(fuseDevice_.fd(), buf.data(), buf.size());
    if (UNLIKELY(res < 0)) {
      int error = errno;
      if (stop_.load(std::memory_order_relaxed)) {
        break;
      }

      if (error == EINTR || error == EAGAIN) {
        // If we got interrupted by a signal while reading the next
        // fuse command, we will simply retry and read the next thing.
        continue;
      } else if (error == ENOENT) {
        // According to comments in the libfuse code:
        // ENOENT means the operation was interrupted; it's safe to restart
        continue;
      } else if (error == ENODEV) {
        // ENODEV means the filesystem was unmounted
        folly::call_once(unmountLogFlag_, [this] {
          XLOG(DBG3) << "received unmount event ENODEV on mount " << mountPath_;
        });
        requestSessionExit(StopReason::UNMOUNTED);
        break;
      } else {
        XLOG(WARNING) << "error reading from fuse channel: "
                      << folly::errnoStr(error);
        requestSessionExit(StopReason::FUSE_READ_ERROR);
        break;
      }
    }

    const auto arg_size = static_cast<size_t>(res);
    if (arg_size < sizeof(struct fuse_in_header)) {
      if (arg_size == 0) {
        // This code path is hit when a fake FUSE channel is closed in our unit
        // tests.  On real FUSE channels we should get ENODEV to indicate that
        // the FUSE channel was shut down.  However, in our unit tests that use
        // fake FUSE connections we cannot send an ENODEV error, and so we just
        // close the channel instead.
        requestSessionExit(StopReason::UNMOUNTED);
      } else {
        // We got a partial FUSE header.  This shouldn't ever happen unless
        // there is a bug in the FUSE kernel code.
        XLOG(ERR) << "read truncated message from kernel fuse device: len="
                  << arg_size;
        requestSessionExit(StopReason::FUSE_TRUNCATED_REQUEST);
      }
      return;
    }

    const auto* header = reinterpret_cast<fuse_in_header*>(buf.data());
    const ByteRange arg{
        reinterpret_cast<const uint8_t*>(header + 1),
        arg_size - sizeof(fuse_in_header)};

    XLOG(DBG7) << "fuse request opcode=" << header->opcode << " "
               << fuseOpcodeName(header->opcode) << " unique=" << header->unique
               << " len=" << header->len << " nodeid=" << header->nodeid
               << " uid=" << header->uid << " gid=" << header->gid
               << " pid=" << header->pid;

    // On Linux, if security caps are enabled and the FUSE filesystem implements
    // xattr support, every FUSE_WRITE opcode is preceded by FUSE_GETXATTR for
    // "security.capability". Until we discover a way to tell the kernel that
    // they will always return nothing in an Eden mount, short-circuit that path
    // as efficiently and as early as possible.
    //
    // On some systems, the kernel also frequently requests
    // POSIX ACL xattrs, so fast track those too, if only to make strace
    // logs easier to follow.
    if (header->opcode == FUSE_GETXATTR) {
      const auto getxattr =
          reinterpret_cast<const fuse_getxattr_in*>(arg.data());

      // Evaluate strlen before the comparison loop below.
      const StringPiece namePiece{reinterpret_cast<const char*>(getxattr + 1)};
      static constexpr StringPiece kFastTracks[] = {
          "security.capability",
          "system.posix_acl_access",
          "system.posix_acl_default"};

      // Unclear whether one strlen and matching compares is better than
      // strcmps, but it's probably in the noise.
      bool matched = false;
      for (auto fastTrack : kFastTracks) {
        if (namePiece == fastTrack) {
          replyError(*header, ENODATA);
          matched = true;
          break;
        }
      }
      if (matched) {
        continue;
      }
    }

    // Sanity check to ensure that the request wasn't from ourself.
    //
    // We should never make requests to ourself via normal filesytem
    // operations going through the kernel.  Otherwise we risk deadlocks if the
    // kernel calls us while holding an inode lock, and we then end up making a
    // filesystem call that need the same inode lock.  We will then not be able
    // to resolve this deadlock on kernel inode locks without rebooting the
    // system.
    if (UNLIKELY(static_cast<pid_t>(header->pid) == myPid)) {
      replyError(*header, EIO);
      XLOG(CRITICAL) << "Received FUSE request from our own pid: opcode="
                     << header->opcode << " nodeid=" << header->nodeid
                     << " pid=" << header->pid;
      continue;
    }

    auto* handlerEntry = lookupFuseHandlerEntry(header->opcode);
    processAccessLog_.recordAccess(
        header->pid,
        handlerEntry ? handlerEntry->accessType : AccessType::FsChannelOther);

    switch (header->opcode) {
      case FUSE_INIT:
        replyError(*header, EPROTO);
        throw std::runtime_error(
            "received FUSE_INIT after we have been initialized!?");

      case FUSE_GETLK:
      case FUSE_SETLK:
      case FUSE_SETLKW:
        // Deliberately not handling locking; this causes
        // the kernel to do it for us
        XLOG(DBG7) << fuseOpcodeName(header->opcode);
        replyError(*header, ENOSYS);
        break;

#ifdef __linux__
      case FUSE_LSEEK:
        // We only support stateless file handles, so lseek() is meaningless
        // for us.  Returning ENOSYS causes the kernel to implement it for us,
        // and will cause it to stop sending subsequent FUSE_LSEEK requests.
        XLOG(DBG7) << "FUSE_LSEEK";
        replyError(*header, ENOSYS);
        break;
#endif

      case FUSE_POLL:
        // We do not currently implement FUSE_POLL.
        XLOG(DBG7) << "FUSE_POLL";
        replyError(*header, ENOSYS);
        break;

      case FUSE_INTERRUPT: {
        // no reply is required
        XLOG(DBG7) << "FUSE_INTERRUPT";
        // Ignore it: we don't have a reliable way to guarantee
        // that interrupting functions correctly.
        // In addition, the kernel (certainly on macOS) may recycle
        // ids too quickly for us to safely track by `unique` id.
        break;
      }

      case FUSE_DESTROY:
        XLOG(DBG7) << "FUSE_DESTROY";
        dispatcher_->destroy();
        // FUSE on linux doesn't care whether we reply to FUSE_DESTROY
        // but the macOS implementation blocks the unmount syscall until
        // we have responded, which in turn blocks our attempt to gracefully
        // unmount, so we respond here.  It doesn't hurt Linux to respond
        // so we do it for both platforms.
        replyError(*header, 0);
        break;

      case FUSE_NOTIFY_REPLY:
        XLOG(DBG7) << "FUSE_NOTIFY_REPLY";
        // Don't strictly need to do anything here, but may want to
        // turn the kernel notifications in Futures and use this as
        // a way to fulfil the promise
        break;

      case FUSE_IOCTL:
        // Rather than the default ENOSYS, we need to return ENOTTY
        // to indicate that the requested ioctl is not supported
        replyError(*header, ENOTTY);
        break;

      default: {
        if (handlerEntry && handlerEntry->handler) {
          auto requestId = generateUniqueID();
          if (handlerEntry->argRenderer &&
              traceDetailedArguments_->load(std::memory_order_acquire)) {
            traceBus_->publish(FuseTraceEvent::start(
                requestId, *header, handlerEntry->argRenderer(arg)));
          } else {
            traceBus_->publish(FuseTraceEvent::start(requestId, *header));
          }

          // This is a shared_ptr because, due to timeouts, the internal request
          // lifetime may not match the FUSE request lifetime, so we capture it
          // in both. I'm sure this could be improved with some cleverness.
          auto request =
              RequestContext::makeSharedRequestContext<FuseRequestContext>(
                  this, *header);

          ++state_.wlock()->pendingRequests;

          auto headerCopy = *header;

          FB_LOG(*straceLogger_, DBG7, ([&]() -> std::string {
            std::string rendered;
            if (handlerEntry->argRenderer) {
              rendered = handlerEntry->argRenderer(arg);
            }
            return fmt::format(
                "{}({}{}{})",
                handlerEntry->getShortName(),
                headerCopy.nodeid,
                rendered.empty() ? "" : ", ",
                rendered);
          })());

          request
              ->catchErrors(
                  folly::makeFutureWith([&] {
                    request->startRequest(
                        dispatcher_->getStats(),
                        handlerEntry->stat,
                        *(liveRequestWatches_.get()));
                    return (this->*handlerEntry->handler)(
                               *request, request->getReq(), arg)
                        .semi()
                        .via(&folly::QueuedImmediateExecutor::instance());
                  }).ensure([request] {
                    }).within(requestTimeout_),
                  notifier_.get())
              .ensure([this, request, requestId, headerCopy] {
                traceBus_->publish(FuseTraceEvent::finish(
                    requestId, headerCopy, request->getResult()));

                // We may be complete; check to see if all requests are
                // done and whether there are any threads remaining.
                auto state = state_.wlock();
                XCHECK_NE(state->pendingRequests, 0u)
                    << "pendingRequests double decrement";
                if (--state->pendingRequests == 0 &&
                    state->stoppedThreads == numThreads_) {
                  sessionComplete(std::move(state));
                }
              });
          break;
        }

        const auto opcode = header->opcode;
        tryRlockCheckBeforeUpdate<folly::Unit>(
            unhandledOpcodes_,
            [&](const auto& unhandledOpcodes) -> std::optional<folly::Unit> {
              if (unhandledOpcodes.find(opcode) != unhandledOpcodes.end()) {
                return folly::unit;
              }
              return std::nullopt;
            },
            [&](auto& unhandledOpcodes) -> folly::Unit {
              XLOG(WARN) << "unhandled fuse opcode " << opcode << "("
                         << fuseOpcodeName(opcode) << ")";
              unhandledOpcodes->insert(opcode);
              return folly::unit;
            });

        try {
          replyError(*header, ENOSYS);
        } catch (const std::system_error& exc) {
          XLOG(ERR) << "Failed to write error response to fuse: " << exc.what();
          requestSessionExit(StopReason::FUSE_WRITE_ERROR);
          return;
        }
        break;
      }
    }
  }
}

void FuseChannel::sessionComplete(folly::Synchronized<State>::LockedPtr state) {
  // Check to see if we should delete ourself after fulfilling
  // sessionCompletePromise_
  bool destroy = state->destroyPending;

  // Build the StopData to return
  StopData data;
  data.reason = state->stopReason;
  if (isFuseDeviceValid(data.reason) && connInfo_.has_value()) {
    data.fuseDevice = std::move(fuseDevice_);
    data.fuseSettings = connInfo_.value();
  }

  // Unlock the state before the remaining steps
  state.unlock();

  // Stop the invalidation thread.  We do not do this when requestSessionExit()
  // is called since we want to continue to allow invalidation requests to be
  // processed until all outstanding requests complete.
  stopInvalidationThread();

  // Fulfill sessionCompletePromise
  sessionCompletePromise_.setValue(std::move(data));

  // Destroy ourself if desired
  if (destroy) {
    delete this;
  }
}

ImmediateFuture<folly::Unit> FuseChannel::fuseRead(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto read = reinterpret_cast<const fuse_read_in*>(arg.data());

  XLOG(DBG7) << "FUSE_READ";

  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->read(ino, read->size, read->offset, request)
      .thenValue([&request](BufVec&& buf) { request.sendReply(*buf); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseWrite(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto write = reinterpret_cast<const fuse_write_in*>(arg.data());
  auto bufPtr = reinterpret_cast<const char*>(write + 1);
  if (connInfo_->minor < 9) {
    bufPtr =
        reinterpret_cast<const char*>(arg.data()) + FUSE_COMPAT_WRITE_IN_SIZE;
  }
  XLOG(DBG7) << "FUSE_WRITE " << write->size << " @" << write->offset;

  auto ino = InodeNumber{header.nodeid};
  return dispatcher_
      ->write(
          ino, folly::StringPiece{bufPtr, write->size}, write->offset, request)
      .thenValue([&request](size_t written) {
        fuse_write_out out = {};
        out.size = written;
        request.sendReply(out);
      });
}

namespace {
PathComponentPiece extractPathComponent(StringPiece s, bool requireUtf8Path) {
  try {
    return PathComponentPiece(s);
  } catch (const PathComponentNotUtf8& ex) {
    if (requireUtf8Path) {
      throw std::system_error(EILSEQ, std::system_category(), ex.what());
    }

    return PathComponentPiece(s, detail::SkipPathSanityCheck());
  }
}
} // namespace

ImmediateFuture<folly::Unit> FuseChannel::fuseLookup(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto name = extractPathComponent(
      reinterpret_cast<const char*>(arg.data()), requireUtf8Path_);
  const auto parent = InodeNumber{header.nodeid};

  XLOG(DBG7) << "FUSE_LOOKUP parent=" << parent << " name=" << name;

  return dispatcher_->lookup(header.unique, parent, name, request)
      .thenValue([&request](fuse_entry_out entry) {
        request.sendReplyWithInode(entry.nodeid, entry);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseForget(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  auto forget = reinterpret_cast<const fuse_forget_in*>(arg.data());
  XLOG(DBG7) << "FUSE_FORGET inode=" << header.nodeid
             << " nlookup=" << forget->nlookup;
  dispatcher_->forget(InodeNumber{header.nodeid}, forget->nlookup);
  request.replyNone();
  return folly::unit;
}

ImmediateFuture<folly::Unit> FuseChannel::fuseGetAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange /*arg*/) {
  XLOG(DBG7) << "FUSE_GETATTR inode=" << header.nodeid;
  return dispatcher_->getattr(InodeNumber{header.nodeid}, request)
      .thenValue([&request](FuseDispatcher::Attr attr) {
        request.sendReply(attr.asFuseAttr());
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseSetAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto setattr = reinterpret_cast<const fuse_setattr_in*>(arg.data());
  XLOG(DBG7) << "FUSE_SETATTR inode=" << header.nodeid;
  return dispatcher_->setattr(InodeNumber{header.nodeid}, *setattr, request)
      .thenValue([&request](FuseDispatcher::Attr attr) {
        request.sendReply(attr.asFuseAttr());
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseReadLink(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange /*arg*/) {
  XLOG(DBG7) << "FUSE_READLINK inode=" << header.nodeid;
  bool kernelCachesReadlink = false;
#ifdef FUSE_CACHE_SYMLINKS
  kernelCachesReadlink = connInfo_->flags & FUSE_CACHE_SYMLINKS;
#endif
  InodeNumber ino{header.nodeid};
  return dispatcher_->readlink(ino, kernelCachesReadlink, request)
      .thenValue([&request](std::string&& str) {
        request.sendReply(folly::StringPiece(str));
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseSymlink(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg.data());
  XLOG(DBG7) << "FUSE_SYMLINK";
  const auto name = extractPathComponent(nameStr, requireUtf8Path_);
  const StringPiece link{nameStr + name.stringPiece().size() + 1};

  InodeNumber parent{header.nodeid};
  return dispatcher_->symlink(parent, name, link, request)
      .thenValue([&request](fuse_entry_out entry) {
        request.sendReplyWithInode(entry.nodeid, entry);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseMknod(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto nod = reinterpret_cast<const fuse_mknod_in*>(arg.data());
  auto nameStr = reinterpret_cast<const char*>(nod + 1);

  if (connInfo_->minor >= 12) {
    // Kernel passes umask in fuse_mknod_in, but unless FUSE_CAP_DONT_MASK is
    // set, the kernel has already masked it out in mode.
    // https://sourceforge.net/p/fuse/mailman/message/22844100/
  } else {
    // Else: no umask or padding fields available
    nameStr =
        reinterpret_cast<const char*>(arg.data()) + FUSE_COMPAT_MKNOD_IN_SIZE;
  }

  const auto name = extractPathComponent(nameStr, requireUtf8Path_);
  XLOG(DBG7) << "FUSE_MKNOD " << name;

  InodeNumber parent{header.nodeid};
  return dispatcher_->mknod(parent, name, nod->mode, nod->rdev, request)
      .thenValue([&request](fuse_entry_out entry) {
        request.sendReplyWithInode(entry.nodeid, entry);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseMkdir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto dir = reinterpret_cast<const fuse_mkdir_in*>(arg.data());
  const auto nameStr = reinterpret_cast<const char*>(dir + 1);
  const auto name = extractPathComponent(nameStr, requireUtf8Path_);

  XLOG(DBG7) << "FUSE_MKDIR " << name;

  // Kernel passes umask in fuse_mkdir_in, but unless FUSE_CAP_DONT_MASK is
  // set, the kernel has already masked it out in mode.
  // https://sourceforge.net/p/fuse/mailman/message/22844100/

  XLOG(DBG7) << "mode = " << dir->mode << "; umask = " << dir->umask;

  InodeNumber parent{header.nodeid};
  mode_t mode = dir->mode & ~dir->umask;
  return dispatcher_->mkdir(parent, name, mode, request)
      .thenValue([&request](fuse_entry_out entry) {
        request.sendReplyWithInode(entry.nodeid, entry);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseUnlink(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg.data());
  const auto name = extractPathComponent(nameStr, requireUtf8Path_);

  XLOG(DBG7) << "FUSE_UNLINK " << name;

  InodeNumber parent{header.nodeid};
  return dispatcher_->unlink(parent, name, request)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseRmdir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg.data());
  const auto name = extractPathComponent(nameStr, requireUtf8Path_);

  XLOG(DBG7) << "FUSE_RMDIR " << name;
  InodeNumber parent{header.nodeid};
  return dispatcher_->rmdir(parent, name, request)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseRename(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto rename = reinterpret_cast<const fuse_rename_in*>(arg.data());
  auto oldNameStr = reinterpret_cast<const char*>(rename + 1);
  StringPiece oldName{oldNameStr};
  StringPiece newName{oldNameStr + oldName.size() + 1};

  if (folly::kIsApple) {
    if (oldName.size() == 0 || newName.size() == 0) {
      // This is gross.  macFUSE appears to have changed the ABI of the FUSE
      // protocol but not bumped the protocol version, so we don't have a great
      // way to handle running on macFUSE or osxfuse.  Once everybody is on
      // macFUSE, this grossness can be removed by updating
      // fuse_kernel_osxfuse.h to its upstream version.
      //
      // The rename request appears to have an additional field that is zeroed
      // out for a regular rename.  That effectively renders oldName as zero
      // sized because we end up pointing at the NUL terminator, and thus
      // newName is also an empty string. Those are impossible names to have, so
      // let's try reinterpreting the struct as this:
      struct macfuse_rename_in {
        __u64 newdir;
        __u64 undocumented;
      };

      const auto macfuse_rename =
          reinterpret_cast<const macfuse_rename_in*>(arg.data());

      oldNameStr = reinterpret_cast<const char*>(macfuse_rename + 1);
      oldName = StringPiece{oldNameStr};
      newName = StringPiece{oldNameStr + oldName.size() + 1};
    }
  }

  InodeNumber parent{header.nodeid};
  InodeNumber newParent{rename->newdir};
  XLOG(DBG7) << "FUSE_RENAME " << oldName << " -> " << newName;
  return dispatcher_
      ->rename(
          parent,
          extractPathComponent(oldName, requireUtf8Path_),
          newParent,
          extractPathComponent(newName, requireUtf8Path_),
          request)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseLink(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto link = reinterpret_cast<const fuse_link_in*>(arg.data());
  const auto nameStr = reinterpret_cast<const char*>(link + 1);
  const auto newName = extractPathComponent(nameStr, requireUtf8Path_);

  XLOG(DBG7) << "FUSE_LINK " << newName;

  InodeNumber ino{link->oldnodeid};
  InodeNumber newParent{header.nodeid};
  return dispatcher_->link(ino, newParent, newName)
      .thenValue([&request](fuse_entry_out entry) {
        request.sendReplyWithInode(entry.nodeid, entry);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseOpen(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg.data());
  XLOG(DBG7) << "FUSE_OPEN";
  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->open(ino, open->flags).thenValue([&request](uint64_t fh) {
    fuse_open_out out = {};
    out.open_flags |= FOPEN_KEEP_CACHE;
    out.fh = fh;
    request.sendReply(out);
  });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseStatFs(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange /*arg*/) {
  XLOG(DBG7) << "FUSE_STATFS";
  return dispatcher_->statfs(InodeNumber{header.nodeid})
      .thenValue([&request](struct fuse_kstatfs&& info) {
        fuse_statfs_out out = {};
        out.st = info;
        request.sendReply(out);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseRelease(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  XLOG(DBG7) << "FUSE_RELEASE";
  auto ino = InodeNumber{header.nodeid};
  auto release = reinterpret_cast<const fuse_release_in*>(arg.data());
  return dispatcher_->release(ino, release->fh)
      .thenValue([&request](folly::Unit) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseFsync(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg.data());
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNC";

  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->fsync(ino, datasync).thenValue([&request](auto&&) {
    request.replyError(0);
  });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseSetXAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto setxattr = reinterpret_cast<const fuse_setxattr_in*>(arg.data());
  const auto nameStr = reinterpret_cast<const char*>(setxattr + 1);
  const StringPiece attrName{nameStr};
  const auto bufPtr = nameStr + attrName.size() + 1;
  const StringPiece value(bufPtr, setxattr->size);

  XLOG(DBG7) << "FUSE_SETXATTR";

  return dispatcher_
      ->setxattr(InodeNumber{header.nodeid}, attrName, value, setxattr->flags)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseGetXAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto getxattr = reinterpret_cast<const fuse_getxattr_in*>(arg.data());
  const auto nameStr = reinterpret_cast<const char*>(getxattr + 1);
  const StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_GETXATTR";
  InodeNumber ino{header.nodeid};
  return dispatcher_->getxattr(ino, attrName, request)
      .thenValue([&request, size = getxattr->size](const std::string& attr) {
        if (size == 0) {
          fuse_getxattr_out out = {};
          out.size = attr.size();
          request.sendReply(out);
        } else if (size < attr.size()) {
          request.replyError(ERANGE);
        } else {
          request.sendReply(StringPiece(attr));
        }
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseListXAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto listattr = reinterpret_cast<const fuse_getxattr_in*>(arg.data());
  XLOG(DBG7) << "FUSE_LISTXATTR";
  InodeNumber ino{header.nodeid};
  return dispatcher_->listxattr(ino).thenValue(
      [&request, size = listattr->size](std::vector<std::string> attrs) {
        // Initialize count to include the \0 for each
        // entry.
        size_t count = attrs.size();
        for (const auto& attr : attrs) {
          count += attr.size();
        }

        if (size == 0) {
          // caller is asking for the overall size
          fuse_getxattr_out out = {};
          out.size = count;
          request.sendReply(out);
        } else if (size < count) {
          XLOG(DBG7) << "LISTXATTR input size is " << size << " and count is "
                     << count;
          request.replyError(ERANGE);
        } else {
          std::string buf;
          buf.reserve(count);
          for (const auto& attr : attrs) {
            buf.append(attr);
            buf.push_back(0);
          }
          XLOG(DBG7) << "LISTXATTR: " << buf;
          request.sendReply(folly::StringPiece(buf));
        }
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseRemoveXAttr(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg.data());
  const StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_REMOVEXATTR";
  return dispatcher_->removexattr(InodeNumber{header.nodeid}, attrName)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseFlush(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto flush = reinterpret_cast<const fuse_flush_in*>(arg.data());
  XLOG(DBG7) << "FUSE_FLUSH";

  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->flush(ino, flush->lock_owner)
      .thenValue([&request](auto&&) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseOpenDir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg.data());
  XLOG(DBG7) << "FUSE_OPENDIR";
  auto ino = InodeNumber{header.nodeid};
  auto minorVersion = connInfo_->minor;
  return dispatcher_->opendir(ino, open->flags)
      .thenValue([&request, minorVersion](uint64_t fh) {
        fuse_open_out out = {};
#ifdef FOPEN_CACHE_DIR
        if (minorVersion >= 28) {
          // Opt into readdir caching.
          out.open_flags |= FOPEN_KEEP_CACHE | FOPEN_CACHE_DIR;
        }
#else
        (void)minorVersion;
#endif
        out.fh = fh;
        request.sendReply(out);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseReadDir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  auto read = reinterpret_cast<const fuse_read_in*>(arg.data());
  XLOG(DBG7) << "FUSE_READDIR";
  auto ino = InodeNumber{header.nodeid};
  return dispatcher_
      ->readdir(ino, FuseDirList{read->size}, read->offset, read->fh, request)
      .thenValue([&request](FuseDirList&& list) {
        const auto buf = list.getBuf();
        request.sendReply(StringPiece{buf});
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseReleaseDir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  XLOG(DBG7) << "FUSE_RELEASEDIR";
  auto ino = InodeNumber{header.nodeid};
  auto release = reinterpret_cast<const fuse_release_in*>(arg.data());
  return dispatcher_->releasedir(ino, release->fh)
      .thenValue([&request](folly::Unit) { request.replyError(0); });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseFsyncDir(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg.data());
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNCDIR";

  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->fsyncdir(ino, datasync).thenValue([&request](auto&&) {
    request.replyError(0);
  });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseAccess(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto access = reinterpret_cast<const fuse_access_in*>(arg.data());
  XLOG(DBG7) << "FUSE_ACCESS";
  InodeNumber ino{header.nodeid};
  return dispatcher_->access(ino, access->mask).thenValue([&request](auto&&) {
    request.replyError(0);
  });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseCreate(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto create = reinterpret_cast<const fuse_create_in*>(arg.data());
  const auto name = extractPathComponent(
      reinterpret_cast<const char*>(create + 1), requireUtf8Path_);
  XLOG(DBG7) << "FUSE_CREATE " << name;
  auto ino = InodeNumber{header.nodeid};
  return dispatcher_->create(ino, name, create->mode, create->flags, request)
      .thenValue([&request](fuse_entry_out entry) {
        fuse_open_out out = {};
        out.open_flags |= FOPEN_KEEP_CACHE;

        XLOG(DBG7) << "CREATE fh=" << out.fh << " flags=" << out.open_flags;

        folly::fbvector<iovec> vec;

        // 3 to avoid realloc when sendReply prepends a header to the iovec
        vec.reserve(3);
        vec.push_back(make_iovec(entry));
        vec.push_back(make_iovec(out));

        request.sendReplyWithInode(entry.nodeid, std::move(vec));
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseBmap(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto bmap = reinterpret_cast<const fuse_bmap_in*>(arg.data());
  XLOG(DBG7) << "FUSE_BMAP";
  return dispatcher_
      ->bmap(InodeNumber{header.nodeid}, bmap->blocksize, bmap->block)
      .thenValue([&request](uint64_t resultIdx) {
        fuse_bmap_out out;
        out.block = resultIdx;
        request.sendReply(out);
      });
}

ImmediateFuture<folly::Unit> FuseChannel::fuseBatchForget(
    FuseRequestContext& request,
    const fuse_in_header& /*header*/,
    ByteRange arg) {
  const auto forgets =
      reinterpret_cast<const fuse_batch_forget_in*>(arg.data());
  auto item = reinterpret_cast<const fuse_forget_one*>(forgets + 1);
  const auto end = item + forgets->count;
  XLOG(DBG7) << "FUSE_BATCH_FORGET";

  while (item != end) {
    dispatcher_->forget(InodeNumber{item->nodeid}, item->nlookup);
    ++item;
  }
  request.replyNone();
  return folly::unit;
}

ImmediateFuture<folly::Unit> FuseChannel::fuseFallocate(
    FuseRequestContext& request,
    const fuse_in_header& header,
    ByteRange arg) {
  const auto* allocate = reinterpret_cast<const fuse_fallocate_in*>(arg.data());
  XLOG(DBG7) << "FUSE_FALLOCATE";

  // We only care to avoid the glibc fallback implementation for
  // posix_fallocate, so don't even pretend to support all the fancy extra modes
  // in Linux's fallocate(2).
  if (allocate->mode != 0) {
    request.replyError(ENOSYS);
    return folly::unit;
  }

  // ... but definitely don't let glibc fall back on its posix_fallocate
  // emulation, which writes one byte per 512 byte chunk in the entire file,
  // which is extremely expensive in an EdenFS checkout.
  return dispatcher_
      ->fallocate(
          InodeNumber{header.nodeid},
          allocate->offset,
          allocate->length,
          request)
      .thenValue([&request](auto) { request.replyError(0); });
}

FuseDeviceUnmountedDuringInitialization::
    FuseDeviceUnmountedDuringInitialization(AbsolutePathPiece mountPath)
    : std::runtime_error{folly::to<string>(
          "FUSE mount \"",
          mountPath,
          "\" was unmounted before we received the INIT packet"_sp)} {}

size_t FuseChannel::getRequestMetric(
    RequestMetricsScope::RequestMetric metric) const {
  std::vector<size_t> counters;
  for (auto& thread_watches : liveRequestWatches_.accessAllThreads()) {
    counters.emplace_back(
        RequestMetricsScope::getMetricFromWatches(metric, *thread_watches));
  }
  return RequestMetricsScope::aggregateMetricCounters(metric, counters);
}

} // namespace facebook::eden

#endif
