/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#if defined(__linux__) || defined(__APPLE__)

#include "eden/fs/privhelper/PinScan.h"

#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cstdio>
#include <cstring>
#include <set>

#include <folly/Conv.h>
#include <folly/String.h>

#include "eden/common/utils/FSDetect.h"

#ifdef __linux__
#include <dirent.h>
#include <sys/sysmacros.h>

#include <cctype>

#include "eden/fs/utils/MountInfoTable.h"
#endif

#ifdef __APPLE__
#include <libproc.h> // @manual
#include <sys/mount.h> // @manual
#include <sys/param.h> // @manual
#include <sys/proc_info.h> // @manual
#endif

namespace facebook::eden {

#ifdef __linux__

std::optional<uid_t> parseFuseUserId(std::string_view mountOptions) {
  std::vector<std::string_view> options;
  folly::split(',', mountOptions, options);
  for (auto option : options) {
    constexpr std::string_view kUserIdPrefix = "user_id=";
    if (option.substr(0, kUserIdPrefix.size()) == kUserIdPrefix) {
      auto value = folly::tryTo<uid_t>(option.substr(kUserIdPrefix.size()));
      if (value.hasValue()) {
        return value.value();
      }
      return std::nullopt;
    }
  }
  return std::nullopt;
}

namespace {

bool isAllDigits(const char* name) {
  if (*name == '\0') {
    return false;
  }
  for (const char* p = name; *p != '\0'; ++p) {
    if (!isdigit(static_cast<unsigned char>(*p))) {
      return false;
    }
  }
  return true;
}

} // namespace

folly::Expected<std::vector<PinnedInode>, int> scanProcessPins(
    const std::vector<uint64_t>& devices,
    const char* procRoot) {
  std::set<PinnedInode> pins;
  if (devices.empty()) {
    return std::vector<PinnedInode>{};
  }

  DIR* proc = opendir(procRoot);
  if (proc == nullptr) {
    return folly::makeUnexpected(errno);
  }

  while (true) {
    // statx below clobbers errno, so reset it before each readdir to tell
    // end-of-directory apart from a read failure.
    errno = 0;
    struct dirent* entry = readdir(proc);
    if (entry == nullptr) {
      break;
    }
    if (!isAllDigits(entry->d_name)) {
      continue;
    }
    for (const char* link : {"cwd", "root"}) {
      const auto path =
          folly::to<std::string>(procRoot, "/", entry->d_name, "/", link);

      // Following a /proc magic link resolves directly to the process's
      // actual (mount, dentry) pair, so the device and inode numbers are
      // truthful regardless of the process's mount namespace or chroot.
      // AT_STATX_DONT_SYNC avoids a round trip into the target filesystem:
      // st_dev/st_ino are served from the kernel inode, so the scan does not
      // block on slow or wedged (FUSE/NFS) filesystems.
      struct statx stx{};
      if (statx(
              AT_FDCWD,
              path.c_str(),
              AT_STATX_DONT_SYNC | AT_NO_AUTOMOUNT,
              STATX_INO,
              &stx) != 0) {
        // Most commonly EACCES (another user's process when not running as
        // root) or ENOENT (the process exited mid-scan). Nothing to do but
        // skip it.
        continue;
      }

      const uint64_t dev = makedev(stx.stx_dev_major, stx.stx_dev_minor);
      for (auto wanted : devices) {
        if (dev == wanted) {
          pins.insert(PinnedInode{dev, stx.stx_ino});
          break;
        }
      }
    }
  }
  const int readdirError = errno;
  closedir(proc);
  if (readdirError != 0) {
    return folly::makeUnexpected(readdirError);
  }

  return std::vector<PinnedInode>{pins.begin(), pins.end()};
}

#endif // __linux__

#ifdef __APPLE__

namespace {

/**
 * libproc's public header only offers PROC_PIDREGIONPATHINFO, which returns
 * one VM region per call, anonymous or not, so walking a process costs one
 * syscall per mapping (a few thousand each). Flavor 22 is
 * PROC_PIDREGIONPATHINFO2 from XNU's proc_info_private.h: it returns the
 * next region at or above the given address that is backed by a vnode,
 * skipping the rest inside the kernel. lsof uses it for the same purpose.
 * Measured with ~1200 processes: 2.3M calls and 2.5s with the public flavor
 * against 36K calls and 250ms with this one, finding the same pins.
 */
constexpr int kProcPidRegionPathInfo2 = 22;

void recordIfOnDevice(
    const vnode_info_path& vip,
    const std::vector<uint64_t>& devices,
    std::set<PinnedInode>& pins) {
  // Compared with the 32-bit device numbers callerMountDevices() collects.
  const uint64_t dev =
      static_cast<uint64_t>(static_cast<uint32_t>(vip.vip_vi.vi_stat.vst_dev));
  if (std::find(devices.begin(), devices.end(), dev) != devices.end()) {
    pins.insert(PinnedInode{dev, vip.vip_vi.vi_stat.vst_ino});
  }
}

/**
 * Returns 0, or an errno when the process's directories could not be seen
 * for a reason other than it being off limits or gone.
 */
int scanWorkingAndRootDirectories(
    pid_t pid,
    const std::vector<uint64_t>& devices,
    std::set<PinnedInode>& pins) {
  proc_vnodepathinfo info{};
  errno = 0;
  if (proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &info, sizeof(info)) !=
      static_cast<int>(sizeof(info))) {
    return errno == 0 || errno == EPERM || errno == ESRCH ? 0 : errno;
  }
  recordIfOnDevice(info.pvi_cdir, devices, pins);
  // A process that has not chroot'ed reports an empty root.
  if (info.pvi_rdir.vip_vi.vi_stat.vst_dev != 0) {
    recordIfOnDevice(info.pvi_rdir, devices, pins);
  }
  return 0;
}

/**
 * Returns 0, or an errno when the process's descriptors could not all be
 * seen: a missed pin is worse than a failed scan, which leaves GC to files.
 */
int scanOpenFiles(
    pid_t pid,
    const std::vector<uint64_t>& devices,
    std::vector<proc_fdinfo>& fds,
    std::set<PinnedInode>& pins) {
  errno = 0;
  int bytes = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, nullptr, 0);
  if (bytes <= 0) {
    // No descriptors, or a process the caller may not inspect or that exited.
    return errno == 0 || errno == EPERM || errno == ESRCH ? 0 : errno;
  }
  // LISTFDS silently truncates a full buffer, so a buffer that came back
  // full is grown and the list fetched again, up to a bound.
  constexpr size_t kMaxFds = 1 << 20;
  fds.resize(bytes / sizeof(proc_fdinfo) + 16);
  while (true) {
    errno = 0;
    bytes = proc_pidinfo(
        pid, PROC_PIDLISTFDS, 0, fds.data(), fds.size() * sizeof(proc_fdinfo));
    if (bytes <= 0) {
      return errno == 0 || errno == EPERM || errno == ESRCH ? 0 : errno;
    }
    if (static_cast<size_t>(bytes) < fds.size() * sizeof(proc_fdinfo)) {
      break;
    }
    if (fds.size() >= kMaxFds) {
      return EOVERFLOW;
    }
    fds.resize(fds.size() * 2);
  }
  const size_t count = bytes / sizeof(proc_fdinfo);
  for (size_t i = 0; i < count; ++i) {
    if (fds[i].proc_fdtype != PROX_FDTYPE_VNODE) {
      continue;
    }
    vnode_fdinfowithpath info{};
    errno = 0;
    if (proc_pidfdinfo(
            pid,
            fds[i].proc_fd,
            PROC_PIDFDVNODEPATHINFO,
            &info,
            sizeof(info)) != static_cast<int>(sizeof(info))) {
      // EBADF: the descriptor was closed after the list was taken.
      if (errno == 0 || errno == EPERM || errno == ESRCH || errno == EBADF) {
        continue;
      }
      return errno;
    }
    recordIfOnDevice(info.pvip, devices, pins);
  }
  return 0;
}

int scanThreadWorkingDirectories(
    pid_t pid,
    const std::vector<uint64_t>& devices,
    std::vector<uint64_t>& threads,
    std::set<PinnedInode>& pins) {
  proc_bsdshortinfo process{};
  if (proc_pidinfo(pid, PROC_PIDT_SHORTBSDINFO, 0, &process, sizeof(process)) ==
          static_cast<int>(sizeof(process)) &&
      !(process.pbsi_flags & PROC_FLAG_THCWD)) {
    return 0;
  }

  // LISTTHREADS cannot size a buffer with a null query, and silently truncates
  // a full buffer. Bound retries and reject an incomplete snapshot.
  constexpr size_t kMaxThreads = 65536;
  int bytes;
  while (true) {
    errno = 0;
    bytes = proc_pidinfo(
        pid,
        PROC_PIDLISTTHREADS,
        0,
        threads.data(),
        threads.size() * sizeof(uint64_t));
    if (bytes <= 0) {
      // No threads to report (a zombie, or one that exited mid-scan) is
      // not a reason to distrust the scan; a real error is.
      return errno == 0 || errno == EPERM || errno == ESRCH ? 0 : errno;
    }
    if (bytes % sizeof(uint64_t) != 0) {
      return EIO;
    }
    if (static_cast<size_t>(bytes) < threads.size() * sizeof(uint64_t)) {
      break;
    }
    if (threads.size() >= kMaxThreads) {
      return EOVERFLOW;
    }
    threads.resize(threads.size() * 2);
  }
  for (size_t i = 0; i < bytes / sizeof(uint64_t); ++i) {
    proc_threadwithpathinfo info{};
    errno = 0;
    if (proc_pidinfo(
            pid, PROC_PIDTHREADPATHINFO, threads[i], &info, sizeof(info)) !=
        static_cast<int>(sizeof(info))) {
      if (errno == 0 || errno == EPERM || errno == ESRCH) {
        continue;
      }
      return errno;
    }
    recordIfOnDevice(info.pvip, devices, pins);
  }
  return 0;
}

/**
 * Returns whether the walk saw any region at all. The private flavor ends
 * the walk with an error whose errno is not documented, so a kernel that
 * rejects the flavor outright is told apart by every process reporting no
 * region, which scanProcessPins checks.
 */
bool scanMappedFiles(
    pid_t pid,
    const std::vector<uint64_t>& devices,
    std::set<PinnedInode>& pins) {
  bool sawRegion = false;
  uint64_t address = 0;
  while (true) {
    // A kernel whose layout of the private flavor differs would fill less
    // than the struct; the walk then ends rather than read a partial fill.
    proc_regionwithpathinfo info{};
    if (proc_pidinfo(
            pid, kProcPidRegionPathInfo2, address, &info, sizeof(info)) !=
        static_cast<int>(sizeof(info))) {
      return sawRegion;
    }
    sawRegion = true;
    if (info.prp_vip.vip_vi.vi_stat.vst_dev != 0) {
      recordIfOnDevice(info.prp_vip, devices, pins);
    }
    // Always move forward, even past a region reported with no size.
    const uint64_t next =
        info.prp_prinfo.pri_address + info.prp_prinfo.pri_size;
    address = next > address ? next : address + 1;
  }
}

} // namespace

folly::Expected<std::vector<PinnedInode>, int> scanProcessPins(
    const std::vector<uint64_t>& devices) {
  std::set<PinnedInode> pins;
  if (devices.empty()) {
    return std::vector<PinnedInode>{};
  }

  // With no buffer, libproc reports how many pids there are right now. Leave
  // room for processes started before the next call, and since a full buffer
  // is silently truncated, grow it and list again when it comes back full.
  errno = 0;
  int count = proc_listallpids(nullptr, 0);
  if (count <= 0) {
    return folly::makeUnexpected(errno == 0 ? EIO : errno);
  }
  std::vector<pid_t> pids(static_cast<size_t>(count) * 2 + 64);
  while (true) {
    errno = 0;
    count = proc_listallpids(pids.data(), pids.size() * sizeof(pid_t));
    if (count <= 0) {
      return folly::makeUnexpected(errno == 0 ? EIO : errno);
    }
    if (static_cast<size_t>(count) < pids.size()) {
      break;
    }
    pids.resize(pids.size() * 2);
  }
  pids.resize(count);

  std::vector<proc_fdinfo> fds;
  std::vector<uint64_t> threads(64);
  bool sawRegion = false;
  for (pid_t pid : pids) {
    if (pid <= 0) {
      continue;
    }
    // Each call fails with EPERM for processes the caller may not inspect
    // and ESRCH for ones that exited mid-scan; both are skipped. Any other
    // failure fails the scan: a missed pin is worse than no pin set, which
    // leaves GC to files.
    if (auto error = scanWorkingAndRootDirectories(pid, devices, pins)) {
      return folly::makeUnexpected(error);
    }
    if (auto error =
            scanThreadWorkingDirectories(pid, devices, threads, pins)) {
      return folly::makeUnexpected(error);
    }
    if (auto error = scanOpenFiles(pid, devices, fds, pins)) {
      return folly::makeUnexpected(error);
    }
    sawRegion |= scanMappedFiles(pid, devices, pins);
  }
  if (!sawRegion) {
    // Every process maps at least its executable: none reporting a region
    // means the kernel rejected the private flavor.
    return folly::makeUnexpected(ENOTSUP);
  }
  return std::vector<PinnedInode>{pins.begin(), pins.end()};
}

folly::Expected<std::vector<PinScanMount>, int> listMountsForPinScan() {
  // MNT_NOWAIT returns the cached statfs data rather than asking every
  // filesystem, so listing mounts cannot block on a slow or wedged one.
  int count = getfsstat(nullptr, 0, MNT_NOWAIT);
  if (count < 0) {
    return folly::makeUnexpected(errno);
  }
  // A full buffer is silently truncated, so grow it and list again then.
  std::vector<struct statfs> stats(static_cast<size_t>(count) + 16);
  while (true) {
    count = getfsstat(
        stats.data(), stats.size() * sizeof(struct statfs), MNT_NOWAIT);
    if (count < 0) {
      return folly::makeUnexpected(errno);
    }
    if (static_cast<size_t>(count) < stats.size()) {
      break;
    }
    stats.resize(stats.size() * 2);
  }
  std::vector<PinScanMount> mounts;
  mounts.reserve(count);
  for (int i = 0; i < count; ++i) {
    const auto& st = stats[i];
    // For NFS mounts, the only kind that consults the pin scan, the kernel
    // reports f_fsid.val[0] as st_dev, so the daemon can map its mounts to
    // the devices the scanner reports without a stat of its own mounts.
    mounts.push_back(
        PinScanMount{
            st.f_mntonname,
            st.f_mntfromname,
            st.f_fstypename,
            static_cast<uint64_t>(static_cast<uint32_t>(st.f_fsid.val[0]))});
  }
  return mounts;
}

#endif // __APPLE__

namespace {
constexpr folly::StringPiece kDoneMarker{"done"};
} // namespace

std::string formatPinScanReport(const PinScanReport& report) {
  std::vector<uint64_t> devices{
      report.scannedDevices.begin(), report.scannedDevices.end()};
  std::sort(devices.begin(), devices.end());
  std::vector<uint64_t> pinDevices;
  for (const auto& [dev, _] : report.pinsByDevice) {
    pinDevices.push_back(dev);
  }
  std::sort(pinDevices.begin(), pinDevices.end());

  std::string out;
  for (auto dev : devices) {
    out += folly::to<std::string>("dev ", dev, "\n");
  }
  for (auto dev : pinDevices) {
    for (auto ino : report.pinsByDevice.at(dev)) {
      out += folly::to<std::string>(dev, " ", ino, "\n");
    }
  }
  out += kDoneMarker.str();
  out += "\n";
  return out;
}

std::optional<PinScanReport> parsePinScanReport(std::string_view output) {
  PinScanReport report;
  std::vector<folly::StringPiece> lines;
  folly::split('\n', output, lines);
  for (folly::StringPiece line : lines) {
    if (line.empty()) {
      continue;
    }
    if (line == kDoneMarker) {
      return report;
    }
    folly::StringPiece first;
    folly::StringPiece second;
    if (!folly::split(' ', line, first, second)) {
      return std::nullopt;
    }
    if (first == "dev") {
      auto dev = folly::tryTo<uint64_t>(second);
      if (!dev) {
        return std::nullopt;
      }
      report.scannedDevices.insert(*dev);
      continue;
    }
    auto dev = folly::tryTo<uint64_t>(first);
    auto ino = folly::tryTo<uint64_t>(second);
    if (!dev || !ino) {
      return std::nullopt;
    }
    report.pinsByDevice[*dev].push_back(*ino);
  }
  return std::nullopt;
}

namespace {

/**
 * The devices of the caller's own EdenFS mounts, or an errno if the mount
 * table could not be read.
 */
folly::Expected<std::vector<uint64_t>, int> callerMountDevices() {
  const uid_t uid = getuid();
  std::vector<uint64_t> devices;
#ifdef __linux__
  auto mounts = getAllMounts(
      MountInfoOptions{
          .includeMountSource = true, .includeMountOptions = true});
  if (mounts.hasError()) {
    return folly::makeUnexpected(mounts.error());
  }
  for (const auto& mount : mounts.value()) {
    if (!is_edenfs_mount(mount.mountSource, mount.fsType)) {
      continue;
    }
    // Only the caller's own FUSE mounts: NFS mounts carry no user_id option.
    if (parseFuseUserId(mount.mountOptions) != uid) {
      continue;
    }
    devices.push_back(makedev(mount.devMajor, mount.devMinor));
  }
#else
  auto mounts = listMountsForPinScan();
  if (mounts.hasError()) {
    return folly::makeUnexpected(mounts.error());
  }
  for (const auto& mount : mounts.value()) {
    if (!is_edenfs_mount(mount.mountSource, mount.fsType)) {
      continue;
    }
    // The privhelper mounts as root, so the mount table does not record the
    // owner. EdenFS reports its owner as the uid of everything it serves,
    // the mount's root directory included. This stat is answered by whichever
    // daemon serves the mount, the caller's or another user's, and hangs
    // while that daemon is wedged. EdenFS mounts are interruptible, so the
    // SIGTERM/SIGKILL the requesting daemon sends when its deadline on the
    // scan passes ends the wait.
    struct stat st{};
    if (stat(mount.mountPoint.c_str(), &st) != 0 || st.st_uid != uid) {
      continue;
    }
    // libproc reports st_dev for the mount's vnodes, whatever the mount type,
    // as an unsigned 32-bit value; dev_t is signed, so widen without sign.
    devices.push_back(static_cast<uint64_t>(static_cast<uint32_t>(st.st_dev)));
  }
#endif
  return devices;
}

} // namespace

int runScanPinsMode() {
  auto devices = callerMountDevices();
  if (devices.hasError()) {
    fprintf(
        stderr,
        "scan-pins: unable to list mounts: %s\n",
        folly::errnoStr(devices.error()).c_str());
    return 1;
  }

  auto pins = scanProcessPins(devices.value());
  if (pins.hasError()) {
    fprintf(
        stderr,
        "scan-pins: unable to scan processes: %s\n",
        folly::errnoStr(pins.error()).c_str());
    return 1;
  }
  PinScanReport report;
  report.scannedDevices.insert(devices.value().begin(), devices.value().end());
  for (const auto& pin : pins.value()) {
    report.pinsByDevice[pin.dev].push_back(pin.ino);
  }
  const auto output = formatPinScanReport(report);
  if (fwrite(output.data(), 1, output.size(), stdout) != output.size() ||
      fflush(stdout) != 0) {
    return 1;
  }
  return 0;
}

} // namespace facebook::eden

#endif // __linux__ || __APPLE__
