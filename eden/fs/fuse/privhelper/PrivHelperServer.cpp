/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/privhelper/PrivHelperServer.h"

#include <boost/algorithm/string/predicate.hpp>
#include <eden/fs/utils/PathFuncs.h>
#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <folly/init/Init.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/LoggerDB.h>
#include <folly/logging/xlog.h>
#include <signal.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/types.h>
#include <unistd.h>
#include <chrono>
#include <set>

#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h> // @manual
#include <IOKit/kext/KextManager.h> // @manual
#include <fuse_ioctl.h> // @manual
#include <fuse_mount.h> // @manual
#include <grp.h> // @manual
#include <sys/sysctl.h> // @manual
#endif

using folly::checkUnixError;
using folly::IOBuf;
using folly::throwSystemError;
using folly::io::Appender;
using folly::io::Cursor;
using folly::io::RWPrivateCursor;
using std::string;

namespace facebook {
namespace eden {

PrivHelperServer::PrivHelperServer() {}

PrivHelperServer::~PrivHelperServer() {}

void PrivHelperServer::init(folly::File&& socket, uid_t uid, gid_t gid) {
  // Call folly::init()
  // For simplicity and safety we always use a fixed set of arguments rather
  // than processing user-supplied arguments since we are running with root
  // privileges.
  std::array<const char*, 3> privhelperArgv = {{
      "edenfs_privhelper",
      "--logging",
      "WARN:default, eden=DBG2; "
      "default=stream:stream=stderr,async=false",
  }};
  char** argv = const_cast<char**>(privhelperArgv.data());
  int argc = privhelperArgv.size();
  folly::init(&argc, &argv, false);

  initPartial(std::move(socket), uid, gid);
}

void PrivHelperServer::initPartial(folly::File&& socket, uid_t uid, gid_t gid) {
  // Make sure init() is only called once.
  CHECK_EQ(uid_, std::numeric_limits<uid_t>::max());
  CHECK_EQ(gid_, std::numeric_limits<gid_t>::max());

  // eventBase_ is a unique_ptr only so that we can delay constructing it until
  // init() is called.  We want to avoid creating it in the constructor since
  // the constructor is often called before we fork, and the EventBase
  // NotificationQueue code checks to ensure that it isn't used across a fork.
  eventBase_ = std::make_unique<folly::EventBase>();
  conn_ = UnixSocket::makeUnique(eventBase_.get(), std::move(socket));
  uid_ = uid;
  gid_ = gid;

  folly::checkPosixError(chdir("/"), "privhelper failed to chdir(/)");
}

#ifdef __APPLE__
namespace {

// Fetches the value of a sysctl by name.
// The result is assumed to be a string.
std::string getSysCtlByName(const char* name, size_t size) {
  std::string buffer(size, 0);
  size_t returnedSize = size - 1;
  auto ret = sysctlbyname(name, &buffer[0], &returnedSize, nullptr, 0);
  if (ret != 0) {
    folly::throwSystemError("failed to retrieve sysctl ", name);
  }
  buffer.resize(returnedSize);
  return buffer;
}

std::pair<int, int> determineMacOsVersion() {
  auto version = getSysCtlByName("kern.osproductversion", 64);

  int major, minor, patch;
  if (sscanf(version.c_str(), "%d.%d.%d", &major, &minor, &patch) < 2) {
    folly::throwSystemErrorExplicit(
        EINVAL, "failed to parse kern.osproductversion string ", version);
  }

  return std::make_pair(major, minor);
}

std::string computeKextPath() {
  auto version = determineMacOsVersion();
  return folly::to<std::string>(
      OSXFUSE_EXTENSIONS_PATH,
      "/",
      version.first,
      ".",
      version.second,
      "/",
      OSXFUSE_KEXT_NAME);
}

// Returns true if the system already knows about the fuse filesystem stuff
bool isFuseKextLoaded() {
  struct vfsconf vfc;
  return getvfsbyname(OSXFUSE_NAME, &vfc) == 0;
}

void ensureFuseKextIsLoaded() {
  if (isFuseKextLoaded()) {
    return;
  }
  auto kextPathString = computeKextPath();

  CFStringRef kextPath = CFStringCreateWithCString(
      kCFAllocatorDefault, kextPathString.c_str(), kCFStringEncodingUTF8);
  SCOPE_EXIT {
    CFRelease(kextPath);
  };

  CFURLRef kextUrl = CFURLCreateWithFileSystemPath(
      kCFAllocatorDefault, kextPath, kCFURLPOSIXPathStyle, true);
  SCOPE_EXIT {
    CFRelease(kextUrl);
  };

  auto ret = KextManagerLoadKextWithURL(kextUrl, NULL);

  if (ret != kOSReturnSuccess) {
    folly::throwSystemErrorExplicit(
        ENOENT, "Failed to load ", kextPathString, ": error code ", ret);
  }

  // libfuse uses a sysctl to update the kext's idea of the admin group,
  // so we do too!
  auto adminGroup = getgrnam(MACOSX_ADMIN_GROUP_NAME);
  if (adminGroup) {
    int gid = adminGroup->gr_gid;
    sysctlbyname(OSXFUSE_SYSCTL_TUNABLES_ADMIN, NULL, NULL, &gid, sizeof(gid));
  }
}

// The osxfuse kernel doesn't automatically assign a device, so we have
// to loop through the different units and attempt to allocate them,
// one by one.  Returns the fd and its unit number on success, throws
// an exception on error.
std::pair<folly::File, int> allocateFuseDevice() {
  ensureFuseKextIsLoaded();

  int fd = -1;
  const int nDevices = OSXFUSE_NDEVICES;
  int dindex;
  for (dindex = 0; dindex < nDevices; dindex++) {
    auto devName = folly::to<std::string>("/dev/osxfuse", dindex);
    fd = folly::openNoInt(devName.c_str(), O_RDWR | O_CLOEXEC);
    if (fd >= 0) {
      return std::make_pair(folly::File{fd, true}, dindex);
    }

    if (errno == EBUSY) {
      continue;
    }
    if (errno == ENODEV || errno == ENOENT) {
      throwSystemError(
          "failed to open ",
          devName,
          ": make sure the osxfuse kernel module is loaded");
    } else {
      throwSystemError("failed to open ", devName);
    }
  }

  throwSystemError(
      "unable to allocate an osxfuse device, "
      "either all instances are busy or the kernel module is not loaded");
}

template <typename T, std::size_t Size>
void checkedSnprintf(
    T (&buf)[Size],
    FOLLY_PRINTF_FORMAT const char* format,
    ...) FOLLY_PRINTF_FORMAT_ATTR(2, 3);

template <typename T, std::size_t Size>
void checkedSnprintf(
    T (&buf)[Size],
    FOLLY_PRINTF_FORMAT const char* format,
    ...) {
  va_list ap;
  va_start(ap, format);
  SCOPE_EXIT {
    va_end(ap);
  };

  auto rc = vsnprintf(buf, Size, format, ap);

  if (rc <= 0 || static_cast<size_t>(rc) >= Size) {
    throw std::runtime_error(folly::to<std::string>(
        "string exceeds buffer size in snprintf.  Format string was ", format));
  }
}

} // namespace
#endif

folly::File PrivHelperServer::fuseMount(const char* mountPath) {
#ifdef __APPLE__
  auto [fuseDev, dindex] = allocateFuseDevice();

  fuse_mount_args args{};
  auto canonicalPath = ::realpath(mountPath, NULL);
  if (!canonicalPath) {
    folly::throwSystemError("failed to realpath ", mountPath);
  }
  SCOPE_EXIT {
    free(canonicalPath);
  };
  if (strlen(canonicalPath) >= sizeof(args.mntpath) - 1) {
    folly::throwSystemErrorExplicit(
        EINVAL, "mount path ", canonicalPath, " is too large for args.mntpath");
  }
  strcpy(args.mntpath, canonicalPath);

  // The most important part of the osxfuse mount protocol is to prove
  // to the mount() syscall that we own an opened unit.  We do this by
  // copying the rdev from the fd and by performing a magic ioctl to
  // get a magic cookie and putting both of those values into the
  // fuse_mount_args struct.
  struct stat st;
  checkUnixError(fstat(fuseDev.fd(), &st));
  args.rdev = st.st_rdev;

  checkUnixError(
      ioctl(fuseDev.fd(), FUSEDEVIOCGETRANDOM, &args.random),
      "failed negotiation with ioctl FUSEDEVIOCGETRANDOM");

  // We get to set some metadata for for mounted volume
  checkedSnprintf(args.fsname, "eden@" OSXFUSE_DEVICE_BASENAME "%d", dindex);
  args.altflags |= FUSE_MOPT_FSNAME;

  auto mountPathBaseName = basename(canonicalPath);
  checkedSnprintf(
      args.volname,
      "%.*s",
      int(mountPathBaseName.size()),
      mountPathBaseName.data());
  args.altflags |= FUSE_MOPT_VOLNAME;

  checkedSnprintf(args.fstypename, "%s", "eden");
  args.altflags |= FUSE_MOPT_FSTYPENAME;

  // And some misc other options...

  args.blocksize = FUSE_DEFAULT_BLOCKSIZE;
  args.altflags |= FUSE_MOPT_BLOCKSIZE;

  // The daemon timeout is a hard timeout for fuse request processing.
  // If the timeout is reached, the kernel will shut down the fuse
  // connection.
  auto daemon_timeout_seconds =
      std::chrono::duration_cast<std::chrono::seconds>(fuseTimeout_).count();
  if (daemon_timeout_seconds > FUSE_MAX_DAEMON_TIMEOUT) {
    args.daemon_timeout = FUSE_MAX_DAEMON_TIMEOUT;
  } else {
    args.daemon_timeout = daemon_timeout_seconds;
  }
  XLOG(ERR) << "setting daemon_timeout to " << args.daemon_timeout;
  args.altflags |= FUSE_MOPT_DAEMON_TIMEOUT;

  // maximum iosize for reading or writing.  We want to allow a much
  // larger default than osxfuse normally provides so that clients
  // can minimize the number of read(2)/write(2) calls needed to
  // write a given chunk of data.
  args.iosize = 1024 * 1024;
  args.altflags |= FUSE_MOPT_IOSIZE;

  // We want normal unix permissions semantics; do not blanket deny
  // access to !owner.  Do not send access(2) calls to userspace.
  args.altflags |= FUSE_MOPT_ALLOW_OTHER | FUSE_MOPT_DEFAULT_PERMISSIONS;

  // SIP causes a number of getxattr requests for properties named
  // com.apple.rootless to be generated as part of stat(2)ing files.
  // setting NO_APPLEXATTR makes the kext handle those attribute gets
  // by returning an error, and avoids sending the request to userspace.
  args.altflags |= FUSE_MOPT_NO_APPLEXATTR;

  int mountFlags = MNT_NOSUID;
  checkUnixError(
      mount(OSXFUSE_NAME, args.mntpath, mountFlags, &args), "failed to mount");
  return std::move(fuseDev);

#else
  // We manually call open() here rather than using the folly::File()
  // constructor just so we can emit a slightly more helpful message on error.
  const char* devName = "/dev/fuse";
  const int fd = folly::openNoInt(devName, O_RDWR | O_CLOEXEC);
  if (fd < 0) {
    if (errno == ENODEV || errno == ENOENT) {
      throwSystemError(
          "failed to open ",
          devName,
          ": make sure the fuse kernel module is loaded");
    } else {
      throwSystemError("failed to open ", devName);
    }
  }
  folly::File fuseDev(fd, true);

  // Prepare the flags and options to pass to mount(2).
  // We currently don't allow these to be customized by the unprivileged
  // requester.  We could add this functionality in the future if we have a
  // need for it, but we would need to validate their changes are safe.
  const int rootMode = S_IFDIR;
  auto mountOpts = folly::sformat(
      "allow_other,default_permissions,"
      "rootmode={:o},user_id={},group_id={},fd={}",
      rootMode,
      uid_,
      gid_,
      fuseDev.fd());

  // The mount flags.
  // We do not use MS_NODEV.  MS_NODEV prevents mount points from being created
  // inside our filesystem.  We currently use bind mounts to point the buck-out
  // directory to an alternate location outside of eden.
  const int mountFlags = MS_NOSUID;
  const char* type = "fuse";
  int rc = mount("edenfs", mountPath, type, mountFlags, mountOpts.c_str());
  checkUnixError(rc, "failed to mount");
  return fuseDev;
#endif
}

void PrivHelperServer::bindMount(
    const char* clientPath,
    const char* mountPath) {
#ifdef __APPLE__
  throw std::runtime_error("this system does not support bind mounts");
#else
  const int rc =
      mount(clientPath, mountPath, /*type*/ nullptr, MS_BIND, /*data*/ nullptr);
  checkUnixError(
      rc, "failed to bind mount `", clientPath, "` over `", mountPath, "`");
#endif
}

void PrivHelperServer::fuseUnmount(const char* mountPath) {
#ifdef __APPLE__
  auto rc = unmount(mountPath, MNT_FORCE);
#else
  // UMOUNT_NOFOLLOW prevents us from following symlinks.
  // This is needed for security, to ensure that we are only unmounting mount
  // points that we originally mounted.  (The processUnmountMsg() call checks
  // to ensure that the path requested matches one that we know about.)
  //
  // MNT_FORCE asks Linux to remove this mount even if it is still "busy"--if
  // there are other processes with open file handles, or in case we failed to
  // unmount some of the bind mounts contained inside it for some reason.
  // This helps ensure that the unmount actually succeeds.
  // This is the same behavior as "umount --force".
  //
  // MNT_DETACH asks Linux to remove the mount from the filesystem immediately.
  // This is the same behavior as "umount --lazy".
  // This is required for the unmount to succeed in some cases, particularly if
  // something has gone wrong and a bind mount still exists inside this mount
  // for some reason.
  //
  // In the future it might be nice to provide smarter unmount options,
  // such as unmounting only if the mount point is not currently in use.
  // However for now we always do forced unmount.  This helps ensure that
  // edenfs does not get stuck waiting on unmounts to complete when shutting
  // down.
  const int umountFlags = UMOUNT_NOFOLLOW | MNT_FORCE | MNT_DETACH;
  const auto rc = umount2(mountPath, umountFlags);
#endif
  if (rc != 0) {
    const int errnum = errno;
    // EINVAL simply means the path is no longer mounted.
    // This can happen if it was already manually unmounted by a
    // separate process.
    if (errnum != EINVAL) {
      XLOG(WARNING) << "error unmounting " << mountPath << ": "
                    << folly::errnoStr(errnum);
    }
  }
}

UnixSocket::Message PrivHelperServer::processTakeoverStartupMsg(
    Cursor& cursor) {
  string mountPath;
  std::vector<string> bindMounts;
  PrivHelperConn::parseTakeoverStartupRequest(cursor, mountPath, bindMounts);
  XLOG(DBG3) << "takeover startup for \"" << mountPath << "\"; "
             << bindMounts.size() << " bind mounts";

  mountPoints_.insert(mountPath);
  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processMountMsg(Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseMountRequest(cursor, mountPath);
  XLOG(DBG3) << "mount \"" << mountPath << "\"";

  auto fuseDev = fuseMount(mountPath.c_str());
  mountPoints_.insert(mountPath);

  return makeResponse(std::move(fuseDev));
}

UnixSocket::Message PrivHelperServer::processUnmountMsg(Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseUnmountRequest(cursor, mountPath);
  XLOG(DBG3) << "unmount \"" << mountPath << "\"";

  const auto it = mountPoints_.find(mountPath);
  if (it == mountPoints_.end()) {
    throw std::domain_error(
        folly::to<string>("No FUSE mount found for ", mountPath));
  }

  fuseUnmount(mountPath.c_str());
  mountPoints_.erase(mountPath);
  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processTakeoverShutdownMsg(
    Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseTakeoverShutdownRequest(cursor, mountPath);
  XLOG(DBG3) << "takeover shutdown \"" << mountPath << "\"";

  const auto it = mountPoints_.find(mountPath);
  if (it == mountPoints_.end()) {
    throw std::domain_error(
        folly::to<string>("No FUSE mount found for ", mountPath));
  }

  mountPoints_.erase(mountPath);
  return makeResponse();
}

std::string PrivHelperServer::findMatchingMountPrefix(folly::StringPiece path) {
  for (const auto& mountPoint : mountPoints_) {
    if (boost::starts_with(path, mountPoint + "/")) {
      return mountPoint;
    }
  }
  throw std::domain_error(folly::to<string>("No FUSE mount found for ", path));
}

UnixSocket::Message PrivHelperServer::processBindMountMsg(Cursor& cursor) {
  string clientPath;
  string mountPath;
  PrivHelperConn::parseBindMountRequest(cursor, clientPath, mountPath);
  XLOG(DBG3) << "bind mount \"" << mountPath << "\"";

  // findMatchingMountPrefix will throw if mountPath doesn't match
  // any known mount.  We perform this check so that we're not a
  // vector for mounting things in arbitrary places.
  auto key = findMatchingMountPrefix(mountPath);

  bindMount(clientPath.c_str(), mountPath.c_str());
  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processBindUnMountMsg(Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseBindUnMountRequest(cursor, mountPath);
  XLOG(DBG3) << "bind unmount \"" << mountPath << "\"";

  // findMatchingMountPrefix will throw if mountPath doesn't match
  // any known mount.  We perform this check so that we're not a
  // vector for arbitrarily unmounting things.
  findMatchingMountPrefix(mountPath);

  bindUnmount(mountPath.c_str());

  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processSetLogFileMsg(
    folly::io::Cursor& cursor,
    UnixSocket::Message& request) {
  XLOG(DBG3) << "set log file";
  PrivHelperConn::parseSetLogFileRequest(cursor);
  if (request.files.size() != 1) {
    throw std::runtime_error(folly::to<string>(
        "expected to receive 1 file descriptor with setLogFile() request ",
        "received ",
        request.files.size()));
  }

  setLogFile(std::move(request.files[0]));

  return makeResponse();
}

void PrivHelperServer::setLogFile(folly::File&& logFile) {
  // Replace stdout and stderr with the specified file descriptor
  folly::checkUnixError(dup2(logFile.fd(), STDOUT_FILENO));
  folly::checkUnixError(dup2(logFile.fd(), STDERR_FILENO));
}

UnixSocket::Message PrivHelperServer::processSetDaemonTimeout(
    folly::io::Cursor& cursor,
    UnixSocket::Message& /* request */) {
  XLOG(DBG3) << "set daemon timeout";
  std::chrono::nanoseconds duration;
  PrivHelperConn::parseSetDaemonTimeoutRequest(cursor, duration);

  setDaemonTimeout(duration);

  return makeResponse();
}

void PrivHelperServer::setDaemonTimeout(std::chrono::nanoseconds duration) {
  fuseTimeout_ = duration;
}

namespace {
/// Get the file system ID, or an errno value on error
folly::Expected<unsigned long, int> getFSID(const char* path) {
  struct statvfs data;
  if (statvfs(path, &data) != 0) {
    return folly::makeUnexpected(errno);
  }
  return folly::makeExpected<int>(data.f_fsid);
}
} // namespace

void PrivHelperServer::bindUnmount(const char* mountPath) {
  // Check the current filesystem information for this path,
  // so we can confirm that it has been unmounted afterwards.
  const auto origFSID = getFSID(mountPath);

  fuseUnmount(mountPath);

  // Empirically, the unmount may not be complete when umount2() returns.
  // To work around this, we repeatedly invoke statvfs() on the bind mount
  // until it fails or returns a different filesystem ID.
  //
  // Give up after 2 seconds even if the unmount does not appear complete.
  constexpr auto timeout = std::chrono::seconds(2);
  const auto endTime = std::chrono::steady_clock::now() + timeout;
  while (true) {
    const auto fsid = getFSID(mountPath);
    if (!fsid.hasValue()) {
      // Assume the file system is unmounted if the statvfs() call failed.
      break;
    }
    if (origFSID.hasValue() && origFSID.value() != fsid.value()) {
      // The unmount has succeeded if the filesystem ID is different now.
      break;
    }

    if (std::chrono::steady_clock::now() > endTime) {
      XLOG(WARNING) << "error unmounting " << mountPath
                    << ": mount did not go away after successful unmount call";
      break;
    }
    sched_yield();
  }
}

void PrivHelperServer::run() {
  // Ignore SIGINT and SIGTERM.
  // We should only exit when our parent process does.
  // (Normally if someone hits Ctrl-C in their terminal this will send SIGINT
  // to both our parent process and to us.  The parent process should exit due
  // to this signal.  We don't want to exit immediately--we want to wait until
  // the parent exits and then umount all outstanding mount points before we
  // exit.)
  if (signal(SIGINT, SIG_IGN) == SIG_ERR) {
    XLOG(FATAL) << "error setting SIGINT handler in privhelper process"
                << folly::errnoStr(errno);
  }
  if (signal(SIGTERM, SIG_IGN) == SIG_ERR) {
    XLOG(FATAL) << "error setting SIGTERM handler in privhelper process"
                << folly::errnoStr(errno);
  }

  conn_->setReceiveCallback(this);
  eventBase_->loop();

  // We terminate the event loop when the socket has been closed.
  // This normally means the parent process exited, so we can clean up and exit
  // too.
  XLOG(DBG5) << "privhelper process exiting";

  // Unmount all active mount points
  cleanupMountPoints();
}

void PrivHelperServer::messageReceived(UnixSocket::Message&& message) noexcept {
  try {
    processAndSendResponse(std::move(message));
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error processing privhelper request: "
              << folly::exceptionStr(ex);
  }
}

void PrivHelperServer::processAndSendResponse(UnixSocket::Message&& message) {
  Cursor cursor{&message.data};
  const auto xid = cursor.readBE<uint32_t>();
  const auto msgType =
      static_cast<PrivHelperConn::MsgType>(cursor.readBE<uint32_t>());
  auto responseType = msgType;

  UnixSocket::Message response;
  try {
    response = processMessage(msgType, cursor, message);
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error processing privhelper request: "
              << folly::exceptionStr(ex);
    responseType = PrivHelperConn::RESP_ERROR;
    response = makeResponse();
    Appender appender(&response.data, 1024);
    PrivHelperConn::serializeErrorResponse(appender, ex);
  }

  // Put the transaction ID and message type in the response.
  // The makeResponse() APIs ensure there is enough headroom for the header.
  if (response.data.headroom() >= PrivHelperConn::kHeaderSize) {
    response.data.prepend(PrivHelperConn::kHeaderSize);
  } else {
    // This is unexpected, but go ahead and allocate more room just in case this
    // ever does occur.
    XLOG(WARN) << "insufficient headroom for privhelper response header: "
               << "making more space";
    auto body = std::make_unique<IOBuf>(std::move(response.data));
    response.data = IOBuf(IOBuf::CREATE, PrivHelperConn::kHeaderSize);
    response.data.append(PrivHelperConn::kHeaderSize);
    response.data.prependChain(std::move(body));
  }

  static_assert(
      PrivHelperConn::kHeaderSize == 2 * sizeof(uint32_t),
      "This code needs to be updated if we ever change the header format");
  RWPrivateCursor respCursor(&response.data);
  respCursor.writeBE<uint32_t>(xid);
  respCursor.writeBE<uint32_t>(responseType);

  conn_->send(std::move(response));
}

UnixSocket::Message PrivHelperServer::makeResponse() {
  // 1024 bytes is enough for most responses.  If the response is longer
  // we will allocate more room later.
  constexpr size_t kDefaultBufferSize = 1024;

  UnixSocket::Message msg;
  msg.data = IOBuf(IOBuf::CREATE, kDefaultBufferSize);

  // Leave enough headroom for the response header that includes the transaction
  // ID and message type.
  msg.data.advance(PrivHelperConn::kHeaderSize);
  return msg;
}

UnixSocket::Message PrivHelperServer::makeResponse(folly::File&& file) {
  auto response = makeResponse();
  response.files.push_back(std::move(file));
  return response;
}

UnixSocket::Message PrivHelperServer::processMessage(
    PrivHelperConn::MsgType msgType,
    Cursor& cursor,
    UnixSocket::Message& request) {
  switch (msgType) {
    case PrivHelperConn::REQ_MOUNT_FUSE:
      return processMountMsg(cursor);
    case PrivHelperConn::REQ_MOUNT_BIND:
      return processBindMountMsg(cursor);
    case PrivHelperConn::REQ_UNMOUNT_FUSE:
      return processUnmountMsg(cursor);
    case PrivHelperConn::REQ_TAKEOVER_SHUTDOWN:
      return processTakeoverShutdownMsg(cursor);
    case PrivHelperConn::REQ_TAKEOVER_STARTUP:
      return processTakeoverStartupMsg(cursor);
    case PrivHelperConn::REQ_SET_LOG_FILE:
      return processSetLogFileMsg(cursor, request);
    case PrivHelperConn::REQ_UNMOUNT_BIND:
      return processBindUnMountMsg(cursor);
    case PrivHelperConn::REQ_SET_DAEMON_TIMEOUT:
      return processSetDaemonTimeout(cursor, request);
    case PrivHelperConn::MSG_TYPE_NONE:
    case PrivHelperConn::RESP_ERROR:
      break;
  }

  throw std::runtime_error(
      folly::to<std::string>("unexpected privhelper message type: ", msgType));
}

void PrivHelperServer::eofReceived() noexcept {
  eventBase_->terminateLoopSoon();
}

void PrivHelperServer::socketClosed() noexcept {
  eventBase_->terminateLoopSoon();
}

void PrivHelperServer::receiveError(
    const folly::exception_wrapper& ew) noexcept {
  XLOG(ERR) << "receive error in privhelper server: " << ew;
  eventBase_->terminateLoopSoon();
}

void PrivHelperServer::cleanupMountPoints() {
  for (const auto& mountPoint : mountPoints_) {
    try {
      fuseUnmount(mountPoint.c_str());
    } catch (const std::exception& ex) {
      XLOG(ERR) << "error unmounting \"" << mountPoint
                << "\": " << folly::exceptionStr(ex);
    }
  }

  mountPoints_.clear();
}

} // namespace eden
} // namespace facebook
