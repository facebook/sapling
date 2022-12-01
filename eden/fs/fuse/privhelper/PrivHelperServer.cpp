/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/privhelper/PrivHelperServer.h"
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"

#include <boost/algorithm/string/predicate.hpp>
#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/Utility.h>
#include <folly/init/Init.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/LoggerDB.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include <folly/system/ThreadName.h>
#include <signal.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/types.h>
#include <chrono>
#include <set>
#include "eden/fs/fuse/privhelper/NfsMountRpc.h"
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SysctlUtil.h"
#include "eden/fs/utils/Throw.h"

#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h> // @manual
#include <IOKit/kext/KextManager.h> // @manual
#include <eden/fs/utils/Pipe.h>
#include <eden/fs/utils/SpawnedProcess.h>
#include <fuse_ioctl.h> // @manual
#include <fuse_mount.h> // @manual
#include <grp.h> // @manual
#include <sys/ioccom.h> // @manual
#include <sys/sysctl.h> // @manual
#endif

using folly::checkUnixError;
using folly::IOBuf;
using folly::throwSystemError;
using folly::io::Appender;
using folly::io::Cursor;
using folly::io::RWPrivateCursor;
using std::string;

namespace facebook::eden {

PrivHelperServer::PrivHelperServer() {}

PrivHelperServer::~PrivHelperServer() {}

void PrivHelperServer::init(folly::File&& socket, uid_t uid, gid_t gid) {
  initPartial(std::move(socket), uid, gid);
}

void PrivHelperServer::initPartial(folly::File&& socket, uid_t uid, gid_t gid) {
  // Make sure init() is only called once.
  XCHECK_EQ(uid_, std::numeric_limits<uid_t>::max());
  XCHECK_EQ(gid_, std::numeric_limits<gid_t>::max());

  // Set our thread name to to make it easier to distinguish
  // the privhelper process from the main EdenFS process.  Setting the thread
  // name for the main thread also changes the process name reported
  // /proc/PID/comm (and therefore by ps).
  //
  // Note that the process name is limited to 15 bytes on Linux, so our process
  // name shows up only as "edenfs_privhelp"
  folly::setThreadName("edenfs_privhelper");

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

std::pair<int, int> determineMacOsVersion() {
  auto version = getSysCtlByName("kern.osproductversion", 64);

  int major, minor, patch;
  if (sscanf(version.c_str(), "%d.%d.%d", &major, &minor, &patch) < 2) {
    folly::throwSystemErrorExplicit(
        EINVAL, "failed to parse kern.osproductversion string ", version);
  }

  return std::make_pair(major, minor);
}

std::string computeOSXFuseKextPath() {
  auto version = determineMacOsVersion();
  // Starting from Big Sur (macOS 11), we no longer need to look for the second
  // number since it is now a _real_ minor version number.
  if (version.first >= 11) {
    return folly::to<std::string>(
        OSXFUSE_EXTENSIONS_PATH, "/", version.first, "/", OSXFUSE_KEXT_NAME);
  }
  return folly::to<std::string>(
      OSXFUSE_EXTENSIONS_PATH,
      "/",
      version.first,
      ".",
      version.second,
      "/",
      OSXFUSE_KEXT_NAME);
}

std::string computeEdenFsKextPath() {
  auto version = determineMacOsVersion();
  return folly::to<std::string>(
      "/Library/Filesystems/eden.fs/Contents/Extensions/",
      version.first,
      ".",
      version.second,
      "/edenfs.kext");
}

// Returns true if the system already knows about the fuse filesystem stuff
bool shouldLoadOSXFuseKext() {
  struct vfsconf vfc;
  return getvfsbyname("osxfuse", &vfc) != 0;
}

bool shouldLoadEdenFsKext() {
  struct vfsconf vfc;
  return getvfsbyname("edenfs", &vfc) != 0;
}

constexpr folly::StringPiece kNfsExtensionPath =
    "/System/Library/Extensions/nfs.kext";

bool shouldLoadNfsKext() {
  if (access(kNfsExtensionPath.str().c_str(), F_OK) != 0) {
    XLOGF(
        DBG3,
        "Kernel extension does not exist at '{}', skipping",
        kNfsExtensionPath);
    return false;
  }

  struct vfsconf vfc;
  return getvfsbyname("nfs", &vfc) != 0;
}

bool tryLoadKext(const std::string& kextPathString) {
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
    XLOG(ERR) << "Failed to load " << kextPathString << ": error code " << ret;
    // Soft error: we might be able to continue with MacFuse
    return false;
  }

  return true;
}

void updateOSXFuseAdminGroup() {
  // libfuse uses a sysctl to update the kext's idea of the admin group,
  // so we do too!
  auto adminGroup = getgrnam(MACOSX_ADMIN_GROUP_NAME);
  if (adminGroup) {
    int gid = adminGroup->gr_gid;
    sysctlbyname(OSXFUSE_SYSCTL_TUNABLES_ADMIN, NULL, NULL, &gid, sizeof(gid));
  }
}

bool loadNfsKext() {
  return tryLoadKext(kNfsExtensionPath.str());
}

// The osxfuse kernel doesn't automatically assign a device, so we have
// to loop through the different units and attempt to allocate them,
// one by one.  Returns the fd and its unit number on success, throws
// an exception on error.
std::pair<folly::File, int> allocateFuseDevice(bool useDevEdenFs) {
  if (useDevEdenFs) {
    if (shouldLoadEdenFsKext()) {
      tryLoadKext(computeEdenFsKextPath());
      updateOSXFuseAdminGroup();
    }
  } else if (shouldLoadOSXFuseKext()) {
    tryLoadKext(computeOSXFuseKextPath());
    updateOSXFuseAdminGroup();
  }

  int fd = -1;
  const int nDevices = OSXFUSE_NDEVICES;
  int dindex;
  for (dindex = 0; dindex < nDevices; dindex++) {
    auto devName = folly::to<std::string>(
        useDevEdenFs ? "/dev/edenfs" : "/dev/osxfuse", dindex);
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
void checkThenPlaceInBuffer(T (&buf)[Size], folly::StringPiece data) {
  if (data.size() >= Size) {
    throw_<std::runtime_error>(fmt::format(
        "string exceeds buffer size in snprintf.  result was {}", data));
  }

  memcpy(buf, data.data(), data.size());
  buf[data.size()] = '\0';
}

// Mount osxfuse (3.x)
folly::File mountOSXFuse(
    const char* mountPath,
    bool readOnly,
    std::chrono::nanoseconds fuseTimeout,
    bool useDevEdenFs) {
  auto [fuseDev, dindex] = allocateFuseDevice(useDevEdenFs);

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
  checkThenPlaceInBuffer(
      args.fsname,
      fmt::format(
          "eden@{}{}",
          useDevEdenFs ? "edenfs" : OSXFUSE_DEVICE_BASENAME,
          dindex));
  args.altflags |= FUSE_MOPT_FSNAME;

  auto mountPathBaseName = basename(canonicalPath);
  checkThenPlaceInBuffer(args.volname, mountPathBaseName);
  args.altflags |= FUSE_MOPT_VOLNAME;

  checkThenPlaceInBuffer(args.fstypename, "eden");
  args.altflags |= FUSE_MOPT_FSTYPENAME;

  // And some misc other options...

  args.blocksize = FUSE_DEFAULT_BLOCKSIZE;
  args.altflags |= FUSE_MOPT_BLOCKSIZE;

  // The daemon timeout is a hard timeout for fuse request processing.
  // If the timeout is reached, the kernel will shut down the fuse
  // connection.
  auto daemon_timeout_seconds =
      std::chrono::duration_cast<std::chrono::seconds>(fuseTimeout).count();
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

  int mountFlags = MNT_NOSUID;
  if (readOnly) {
    mountFlags |= MNT_RDONLY;
  }

  // The mount() syscall can internally attempt to interrogate the filesystem
  // before it returns to us here.  We can't respond to those requests
  // until we have passed the device back to the dispatcher so we're forced
  // to do a little asynchronous dance and run the mount in a separate
  // thread.
  // We'd like to be able to catch invalid parameters detected by mount;
  // those are likely to be immediately returned to us, so we commit a
  // minor crime here and allow the mount thread to set the errno into
  // a shared value.
  // Then we can wait for a short grace period to see if that got populated
  // with an error and propagate that.
  auto shared_errno = std::make_shared<std::atomic<int>>(0);

  auto thr =
      std::thread([args, mountFlags, useDevEdenFs, shared_errno]() mutable {
        auto devName = useDevEdenFs ? "edenfs" : OSXFUSE_NAME;
        auto res = mount(devName, args.mntpath, mountFlags, &args);
        if (res != 0) {
          *shared_errno = errno;
          XLOG(ERR) << "failed to mount " << args.mntpath << " using "
                    << devName << ": " << folly::errnoStr(*shared_errno);
        }
      });
  thr.detach();

  std::this_thread::sleep_for(std::chrono::milliseconds(50));
  if (*shared_errno) {
    folly::throwSystemErrorExplicit(
        *shared_errno, "mount failed for ", args.mntpath);
  }

  return std::move(fuseDev);
}

// Mount MacFuse (4.x)
// MacFuse is a closed-source fork of osxfuse.  In 4.x the mount procedure
// became opaque behind a loader utility that performs the actual mount syscall
// using an undocumented and backwards incompatible mount protocol to prior
// versions. This function uses that utility to perform the mount procedure.
folly::File mountMacFuse(
    const char* mountPath,
    bool readOnly,
    std::chrono::nanoseconds fuseTimeout) {
  if (readOnly) {
    folly::throwSystemErrorExplicit(
        EINVAL, "MacFUSE doesn't support read-only mounts");
  }

  // mount_macfuse will send the fuse device descriptor back to us
  // over a unix domain socket; we create the connected pair here
  // and pass the descriptor to mount_macfuse via the _FUSE_COMMFD
  // environment variable below.
  SocketPair socketPair;
  SpawnedProcess::Options opts;

  auto commFd = opts.inheritDescriptor(std::move(socketPair.write));
  // mount_macfuse refuses to do anything unless this is set
  opts.environment().set("_FUSE_CALL_BY_LIB", "1");
  // Tell it which unix socket to use to pass back the device
  opts.environment().set("_FUSE_COMMFD", folly::to<std::string>(commFd));
  // Tell it to use version 2 of the mount protocol
  opts.environment().set("_FUSE_COMMVERS", "2");
  // It is unclear what purpose passing the daemon path serves, but
  // libfuse does this, and thus we do also.
  opts.environment().set("_FUSE_DAEMON_PATH", executablePath().asString());

  AbsolutePath canonicalPath = realpath(mountPath);

  // These options are equivalent to those that are explained in more
  // detail in mountOSXFuse() above.
  std::vector<std::string> args = {
      "/Library/Filesystems/macfuse.fs/Contents/Resources/mount_macfuse",
      "-ofsname=eden",
      fmt::format("-ovolname={}", canonicalPath.basename()),
      "-ofstypename=eden",
      fmt::format("-oblocksize={}", FUSE_DEFAULT_BLOCKSIZE),
      fmt::format(
          "-odaemon_timeout={}",
          std::chrono::duration_cast<std::chrono::seconds>(fuseTimeout)
              .count()),
      fmt::format("-oiosize={}", 1024 * 1024),
      "-oallow_other",
      "-odefault_permissions",
      canonicalPath.asString(),
  };

  // Start the helper...
  SpawnedProcess mounter(args, std::move(opts));
  // ... but wait for it in another thread.
  // We MUST NOT try to wait for it directly here as the mount protocol
  // requires FUSE_INIT to be replied to before the mount_macfuse can
  // return, and attempting to disrupt that can effectively deadlock
  // macOS to the point that you need to powercycle!
  // We move the process wait into a separate thread so that it can
  // take its time to wait on the child process.
  auto thr =
      std::thread([proc = std::move(mounter)]() mutable { proc.wait(); });
  // we can't wait for the thread for the same reason, so detach it.
  thr.detach();

  // Now, prepare to receive the fuse device descriptor via our socketpair.
  struct iovec iov;
  char buf[1];
  char ccmsg[CMSG_SPACE(sizeof(int))];

  iov.iov_base = buf;
  iov.iov_len = sizeof(buf);

  struct msghdr msg {};
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  msg.msg_control = ccmsg;
  msg.msg_controllen = sizeof(ccmsg);

  while (1) {
    auto rv = recvmsg(socketPair.read.fd(), &msg, 0);
    if (rv == -1 && errno == EINTR) {
      continue;
    }
    if (rv == -1) {
      folly::throwSystemErrorExplicit(
          errno, "failed to recvmsg the fuse device descriptor from MacFUSE");
    }
    if (rv == 0) {
      folly::throwSystemErrorExplicit(
          ECONNRESET,
          "failed to recvmsg the fuse device descriptor from MacFUSE");
    }
    break;
  }

  auto cmsg = CMSG_FIRSTHDR(&msg);
  if (cmsg->cmsg_type != SCM_RIGHTS) {
    folly::throwSystemErrorExplicit(
        EINVAL,
        "MacFUSE didn't send SCM_RIGHTS message while transferring fuse device descriptor");
  }

  // Got it; copy the bytes into something with the right type
  int fuseDevice;
  memcpy(&fuseDevice, CMSG_DATA(cmsg), sizeof(fuseDevice));

  // and take ownership!
  // The caller will complete the FUSE_INIT handshake.
  return folly::File{fuseDevice, true};
}

} // namespace
#endif

folly::File PrivHelperServer::fuseMount(const char* mountPath, bool readOnly) {
#ifdef __APPLE__
  if (useDevEdenFs_) {
    return mountOSXFuse(mountPath, readOnly, fuseTimeout_, useDevEdenFs_);
  }

  try {
    return mountMacFuse(mountPath, readOnly, fuseTimeout_);
  } catch (const std::exception& macFuseExc) {
    XLOG(ERR) << "Failed to mount using MacFuse, trying OSXFuse ("
              << folly::exceptionStr(macFuseExc) << ")";
    return mountOSXFuse(mountPath, readOnly, fuseTimeout_, useDevEdenFs_);
  }
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
  auto mountOpts = fmt::format(
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
  int mountFlags = MS_NOSUID;
  if (readOnly) {
    mountFlags |= MS_RDONLY;
  }
  const char* type = "fuse";
  // The colon indicates to coreutils/gnulib that this is a remote
  // mount so it will not be displayed by `df --local`.
  int rc = mount("edenfs:", mountPath, type, mountFlags, mountOpts.c_str());
  checkUnixError(rc, "failed to mount");
  return fuseDev;
#endif
}

void PrivHelperServer::nfsMount(
    std::string mountPath,
    folly::SocketAddress mountdAddr,
    folly::SocketAddress nfsdAddr,
    bool readOnly,
    uint32_t iosize,
    bool useReaddirplus) {
#ifdef __APPLE__
  if (shouldLoadNfsKext()) {
    XLOG(DBG3) << "Apple nfs.kext is not loaded. Attempting to load.";
    loadNfsKext();
  }

  // Hold the attribute list set below.
  auto attrsBuf = folly::IOBufQueue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender attrSer{&attrsBuf, 1024};

  // Holds the NFS_MATTR_* flags. Each set flag will have a corresponding
  // structure serialized in the attribute list, the order of serialization
  // must follow the increasing order of their associated flags.
  uint32_t mattrFlags = 0;

  // Check if we should enable readdirplus. If so, set readdirplus to 0.
  uint32_t readdirplus_flag = 0;
  if (useReaddirplus) {
    readdirplus_flag = NFS_MFLAG_RDIRPLUS;
  }

  // Make the client use any source port, enable/disable rdirplus, soft but make
  // the mount interruptible. While in theory we would want the mount to be
  // soft, macOS force a maximum timeout of 60s, which in some case is too short
  // for files to be fetched, thus disable it.
  mattrFlags |= NFS_MATTR_FLAGS;
  nfs_mattr_flags flags{
      NFS_MATTR_BITMAP_LEN,
      NFS_MFLAG_RESVPORT | NFS_MFLAG_RDIRPLUS | NFS_MFLAG_SOFT | NFS_MFLAG_INTR,
      NFS_MATTR_BITMAP_LEN,
      NFS_MFLAG_INTR | readdirplus_flag};
  XdrTrait<nfs_mattr_flags>::serialize(attrSer, flags);

  mattrFlags |= NFS_MATTR_NFS_VERSION;
  XdrTrait<nfs_mattr_nfs_version>::serialize(attrSer, 3);

  mattrFlags |= NFS_MATTR_READ_SIZE;
  XdrTrait<nfs_mattr_rsize>::serialize(attrSer, iosize);

  mattrFlags |= NFS_MATTR_WRITE_SIZE;
  XdrTrait<nfs_mattr_wsize>::serialize(attrSer, iosize);

  mattrFlags |= NFS_MATTR_LOCK_MODE;
  XdrTrait<nfs_mattr_lock_mode>::serialize(
      attrSer, nfs_lock_mode::NFS_LOCK_MODE_LOCAL);

  auto mountdFamily = mountdAddr.getFamily();
  auto nfsdFamily = nfsdAddr.getFamily();
  if (mountdFamily != nfsdFamily) {
    throwf<std::runtime_error>(
        "The mountd and nfsd socket must be of the same type: mountd=\"{}\", nfsd=\"{}\"",
        mountdAddr.describe(),
        nfsdAddr.describe());
  }

  mattrFlags |= NFS_MATTR_SOCKET_TYPE;
  nfs_mattr_socket_type socketType;
  switch (nfsdFamily) {
    case AF_INET:
      socketType = "tcp4";
      break;
    case AF_INET6:
      socketType = "tcp6";
      break;
    case AF_UNIX:
      socketType = "ticotsord";
      break;
    default:
      throwf<std::runtime_error>("Unknown socket family: {}", nfsdFamily);
  }
  XdrTrait<nfs_mattr_socket_type>::serialize(attrSer, socketType);

  if (nfsdAddr.isFamilyInet()) {
    mattrFlags |= NFS_MATTR_NFS_PORT;
    XdrTrait<nfs_mattr_nfs_port>::serialize(attrSer, nfsdAddr.getPort());

    mattrFlags |= NFS_MATTR_MOUNT_PORT;
    XdrTrait<nfs_mattr_mount_port>::serialize(attrSer, mountdAddr.getPort());
  }

  mattrFlags |= NFS_MATTR_FS_LOCATIONS;
  auto path = canonicalPath(mountPath);
  auto componentIterator = path.components();
  std::vector<std::string> components;
  for (const auto component : componentIterator) {
    components.push_back(std::string(component.value()));
  }
  nfs_fs_server server{"edenfs", {}, std::nullopt};
  if (nfsdAddr.isFamilyInet()) {
    server.nfss_address.push_back(nfsdAddr.getAddressStr());
  } else {
    server.nfss_address.push_back(nfsdAddr.getPath());
  }
  nfs_fs_location location{{server}, components};
  nfs_mattr_fs_locations locations{{location}, std::nullopt};
  XdrTrait<nfs_mattr_fs_locations>::serialize(attrSer, locations);

  mattrFlags |= NFS_MATTR_MNTFLAGS;
  // These are non-NFS specific and will be also passed directly to mount(2)
  nfs_mattr_mntflags mountFlags = MNT_NOSUID;
  if (readOnly) {
    mountFlags |= MNT_RDONLY;
  }
  XdrTrait<nfs_mattr_mntflags>::serialize(attrSer, mountFlags);

  mattrFlags |= NFS_MATTR_MNTFROM;
  nfs_mattr_mntfrom serverName = "edenfs:";
  XdrTrait<nfs_mattr_mntfrom>::serialize(attrSer, serverName);

  if (nfsdAddr.getFamily() == AF_UNIX) {
    mattrFlags |= NFS_MATTR_LOCAL_NFS_PORT;
    XdrTrait<std::string>::serialize(attrSer, nfsdAddr.getPath());

    mattrFlags |= NFS_MATTR_LOCAL_MOUNT_PORT;
    XdrTrait<std::string>::serialize(attrSer, mountdAddr.getPath());
  }

  auto mountBuf = folly::IOBufQueue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&mountBuf, 1024);

  nfs_mattr mattr{NFS_MATTR_BITMAP_LEN, mattrFlags, attrsBuf.move()};

  nfs_mount_args args{
      /*args_version*/ 88,
      /*args_length*/ 0,
      /*xdr_args_version*/ NFS_XDRARGS_VERSION_0,
      /*nfs_mount_attrs*/ std::move(mattr),
  };

  auto argsLength = XdrTrait<nfs_mount_args>::serializedSize(args);
  args.args_length = folly::to_narrow(argsLength);

  XdrTrait<nfs_mount_args>::serialize(ser, args);

  auto buf = mountBuf.move();
  buf->coalesce();

  XLOGF(
      DBG1,
      "Mounting {} via NFS with opts: mountaddr={},addr={},rsize={},wsize={},vers=3",
      mountPath,
      mountdAddr.describe(),
      nfsdAddr.describe(),
      iosize,
      iosize);

  int rc = mount("nfs", mountPath.c_str(), mountFlags, (void*)buf->data());
  checkUnixError(rc, "failed to mount");

  /*
   * The fsctl syscall is completely undocumented, but it does contain a way to
   * override the f_fstypename returned by statfs. This allows watchman to
   * properly detects the filesystem as EdenFS and not NFS (watchman refuses to
   * watch an NFS filesystem).
   */
  typedef char fstypename_t[MFSTYPENAMELEN];
#define FSIOC_SET_FSTYPENAME_OVERRIDE _IOW('A', 10, fstypename_t)
#define FSCTL_SET_FSTYPENAME_OVERRIDE IOCBASECMD(FSIOC_SET_FSTYPENAME_OVERRIDE)

  rc = fsctl(
      mountPath.c_str(), FSCTL_SET_FSTYPENAME_OVERRIDE, (void*)"edenfs:", 0);
  if (rc != 0) {
    unmount(mountPath.c_str());
    checkUnixError(rc, "failed to fsctl");
  }

#else
  if (!mountdAddr.isFamilyInet() || !nfsdAddr.isFamilyInet()) {
    folly::throwSystemErrorExplicit(
        EINVAL,
        fmt::format(
            "only inet addresses are supported: mountdAddr=\"{}\", nfsdAddr=\"{}\"",
            mountdAddr.describe(),
            nfsdAddr.describe()));
  }
  // Prepare the flags and options to pass to mount(2).
  // Since each mount point will have its own NFS server, we need to manually
  // specify it.
  folly::StringPiece noReaddirplusStr = ",nordirplus,";
  if (useReaddirplus) {
    noReaddirplusStr = ",";
  }
  auto mountOpts = fmt::format(
      "addr={},vers=3,proto=tcp,port={},mountvers=3,mountproto=tcp,mountport={},"
      "noresvport,nolock{}soft,retrans=0,rsize={},wsize={}",
      nfsdAddr.getAddressStr(),
      nfsdAddr.getPort(),
      mountdAddr.getPort(),
      noReaddirplusStr,
      iosize,
      iosize);

  // The mount flags.
  // We do not use MS_NODEV.  MS_NODEV prevents mount points from being created
  // inside our filesystem.  We currently use bind mounts to point the buck-out
  // directory to an alternate location outside of eden.
  int mountFlags = MS_NOSUID;
  if (readOnly) {
    mountFlags |= MS_RDONLY;
  }
  auto source = fmt::format("edenfs:{}", mountPath);
  XLOGF(DBG1, "Mounting {} va NFS with opts: {}", source, mountOpts);

  int rc = mount(
      source.c_str(), mountPath.c_str(), "nfs", mountFlags, mountOpts.c_str());
  checkUnixError(rc, "failed to mount");
#endif
}

void PrivHelperServer::bindMount(
    const char* clientPath,
    const char* mountPath) {
#ifdef __APPLE__
  (void)clientPath;
  (void)mountPath;
  throw std::runtime_error("this system does not support bind mounts");
#else
  const int rc =
      mount(clientPath, mountPath, /*type*/ nullptr, MS_BIND, /*data*/ nullptr);
  checkUnixError(
      rc, "failed to bind mount `", clientPath, "` over `", mountPath, "`");
#endif
}

void PrivHelperServer::unmount(const char* mountPath) {
#ifdef __APPLE__
  auto rc = ::unmount(mountPath, MNT_FORCE);
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

  sanityCheckMountPoint(mountPath);

  mountPoints_.insert(mountPath);
  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processMountMsg(Cursor& cursor) {
  string mountPath;
  bool readOnly;
  PrivHelperConn::parseMountRequest(cursor, mountPath, readOnly);
  XLOG(DBG3) << "mount \"" << mountPath << "\"";

  sanityCheckMountPoint(mountPath);

  auto fuseDev = fuseMount(mountPath.c_str(), readOnly);
  mountPoints_.insert(mountPath);

  return makeResponse(std::move(fuseDev));
}

UnixSocket::Message PrivHelperServer::processMountNfsMsg(Cursor& cursor) {
  string mountPath;
  folly::SocketAddress mountdAddr, nfsdAddr;
  bool readOnly, useReaddirplus;
  uint32_t iosize;
  PrivHelperConn::parseMountNfsRequest(
      cursor,
      mountPath,
      mountdAddr,
      nfsdAddr,
      readOnly,
      iosize,
      useReaddirplus);
  XLOG(DBG3) << "mount.nfs \"" << mountPath << "\"";

  sanityCheckMountPoint(mountPath);

  nfsMount(mountPath, mountdAddr, nfsdAddr, readOnly, iosize, useReaddirplus);
  mountPoints_.insert(mountPath);

  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processUnmountMsg(Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseUnmountRequest(cursor, mountPath);
  XLOG(DBG3) << "unmount \"" << mountPath << "\"";

  const auto it = mountPoints_.find(mountPath);
  if (it == mountPoints_.end()) {
    throw_<std::domain_error>("No FUSE mount found for ", mountPath);
  }

  unmount(mountPath.c_str());
  mountPoints_.erase(mountPath);
  return makeResponse();
}

UnixSocket::Message PrivHelperServer::processNfsUnmountMsg(Cursor& cursor) {
  string mountPath;
  PrivHelperConn::parseNfsUnmountRequest(cursor, mountPath);
  XLOG(DBG3) << "unmount \"" << mountPath << "\"";

  const auto it = mountPoints_.find(mountPath);
  if (it == mountPoints_.end()) {
    throw_<std::domain_error>("No NFS mount found for ", mountPath);
  }

  unmount(mountPath.c_str());
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
    throw_<std::domain_error>("No mount found for ", mountPath);
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
  throw_<std::domain_error>("No FUSE mount found for ", path);
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
    throw_<std::runtime_error>(
        "expected to receive 1 file descriptor with setLogFile() request ",
        "received ",
        request.files.size());
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

UnixSocket::Message PrivHelperServer::processSetUseEdenFs(
    folly::io::Cursor& cursor,
    UnixSocket::Message& /* request */) {
  XLOG(DBG3) << "set use /dev/edenfs";
  PrivHelperConn::parseSetUseEdenFsRequest(cursor, useDevEdenFs_);

  return makeResponse();
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

  unmount(mountPath);

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
  PrivHelperConn::PrivHelperPacket packet = PrivHelperConn::parsePacket(cursor);

  UnixSocket::Message response;
  try {
    response = processMessage(packet, cursor, message);
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error processing privhelper request: "
              << folly::exceptionStr(ex);
    packet.metadata.msg_type = PrivHelperConn::RESP_ERROR;
    response = makeResponse();
    Appender appender(&response.data, 1024);
    PrivHelperConn::serializeErrorResponse(appender, ex);
  }

  // Put the version, transaction ID, and message type in the response.
  // The makeResponse() APIs ensure there is enough headroom for the packet.
  // Version info is important, since we may fall back to an older version
  // in order to process a request.
  size_t packetSize = sizeof(packet);
  if (response.data.headroom() >= packetSize) {
    response.data.prepend(packetSize);
  } else {
    // This is unexpected, but go ahead and allocate more room just in case this
    // ever does occur.
    XLOG(WARN) << "insufficient headroom for privhelper response packet: "
               << "making more space";
    auto body = std::make_unique<IOBuf>(std::move(response.data));
    response.data = IOBuf(IOBuf::CREATE, packetSize);
    response.data.append(packetSize);
    response.data.prependChain(std::move(body));
  }

  RWPrivateCursor respCursor(&response.data);

  PrivHelperConn::serializeResponsePacket(packet, respCursor);

  conn_->send(std::move(response));
}

UnixSocket::Message PrivHelperServer::makeResponse() {
  // 1024 bytes is enough for most responses.  If the response is longer
  // we will allocate more room later.
  constexpr size_t kDefaultBufferSize = 1024;

  UnixSocket::Message msg;
  msg.data = IOBuf(IOBuf::CREATE, kDefaultBufferSize);

  // Leave enough headroom for the response packet that includes the transaction
  // ID and message type (and any additional metadata).
  msg.data.advance(sizeof(PrivHelperConn::PrivHelperPacket));
  return msg;
}

UnixSocket::Message PrivHelperServer::makeResponse(folly::File&& file) {
  auto response = makeResponse();
  response.files.push_back(std::move(file));
  return response;
}

UnixSocket::Message PrivHelperServer::processMessage(
    PrivHelperConn::PrivHelperPacket& packet,
    Cursor& cursor,
    UnixSocket::Message& request) {
  // In the future, we can use packet.header.version to decide how to handle
  // each request. Each request handler can implement different handler logic
  // for each known version (if needed).
  PrivHelperConn::MsgType msgType{packet.metadata.msg_type};
  XLOGF(
      DBG7,
      "Processing message of type {} for protocol version v{}",
      msgType,
      packet.header.version);
  switch (msgType) {
    case PrivHelperConn::REQ_MOUNT_FUSE:
      return processMountMsg(cursor);
    case PrivHelperConn::REQ_MOUNT_NFS:
      return processMountNfsMsg(cursor);
    case PrivHelperConn::REQ_MOUNT_BIND:
      return processBindMountMsg(cursor);
    case PrivHelperConn::REQ_UNMOUNT_FUSE:
      return processUnmountMsg(cursor);
    case PrivHelperConn::REQ_UNMOUNT_NFS:
      return processNfsUnmountMsg(cursor);
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
    case PrivHelperConn::REQ_SET_USE_EDENFS:
      return processSetUseEdenFs(cursor, request);
    case PrivHelperConn::MSG_TYPE_NONE:
    case PrivHelperConn::RESP_ERROR:
      break;
  }

  throw_<std::runtime_error>(
      "unexpected privhelper message type: ", folly::to_underlying(msgType));
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
      unmount(mountPoint.c_str());
    } catch (const std::exception& ex) {
      XLOG(ERR) << "error unmounting \"" << mountPoint
                << "\": " << folly::exceptionStr(ex);
    }
  }

  mountPoints_.clear();
}

} // namespace facebook::eden

#endif
