/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/service/CgroupFileCacheReclaimer.h"

#include <fcntl.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <charconv>
#include <limits>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include <fmt/core.h>
#include <folly/File.h>
#include <folly/FileUtil.h>

namespace facebook::eden {
namespace {

constexpr size_t kMaxInputFileSize = 4 * 1024 * 1024;

std::string joinPath(std::string_view directory, std::string_view name) {
  if (directory.empty() || directory.back() == '/') {
    return fmt::format("{}{}", directory, name);
  }
  return fmt::format("{}/{}", directory, name);
}

std::string readTextFile(const std::string& path) {
  folly::File file{path, O_RDONLY | O_CLOEXEC};
  std::string contents;
  if (!folly::readFile(file.fd(), contents, kMaxInputFileSize + 1)) {
    const auto error = errno;
    throw std::system_error{
        error, std::generic_category(), fmt::format("reading {}", path)};
  }
  if (contents.size() > kMaxInputFileSize) {
    throw std::runtime_error{
        fmt::format("{} is larger than {} bytes", path, kMaxInputFileSize)};
  }
  return contents;
}

std::string_view trim(std::string_view value) {
  constexpr std::string_view whitespace{" \t\r\n"};
  const auto first = value.find_first_not_of(whitespace);
  if (first == std::string_view::npos) {
    return {};
  }
  const auto last = value.find_last_not_of(whitespace);
  return value.substr(first, last - first + 1);
}

uint64_t parseUint64(std::string_view value, std::string_view description) {
  value = trim(value);
  uint64_t result;
  const auto [end, error] =
      std::from_chars(value.data(), value.data() + value.size(), result);
  if (error != std::errc{} || end != value.data() + value.size()) {
    throw std::runtime_error{
        fmt::format("invalid {} value: '{}'", description, value)};
  }
  return result;
}

std::vector<std::string_view> splitLines(std::string_view contents) {
  std::vector<std::string_view> lines;
  while (!contents.empty()) {
    const auto newline = contents.find('\n');
    lines.push_back(contents.substr(0, newline));
    if (newline == std::string_view::npos) {
      break;
    }
    contents.remove_prefix(newline + 1);
  }
  return lines;
}

std::vector<std::string_view> splitFields(std::string_view line) {
  std::vector<std::string_view> fields;
  while (true) {
    line = trim(line);
    if (line.empty()) {
      return fields;
    }
    const auto separator = line.find_first_of(" \t");
    fields.push_back(line.substr(0, separator));
    if (separator == std::string_view::npos) {
      return fields;
    }
    line.remove_prefix(separator + 1);
  }
}

uint64_t readNamedValue(
    std::string_view contents,
    std::string_view name,
    std::string_view path) {
  for (const auto line : splitLines(contents)) {
    const auto fields = splitFields(line);
    if (fields.size() == 2 && fields[0] == name) {
      return parseUint64(fields[1], fmt::format("{} in {}", name, path));
    }
  }
  throw std::runtime_error{fmt::format("{} does not contain {}", path, name)};
}

std::string decodeMountInfoPath(std::string_view encoded) {
  std::string decoded;
  decoded.reserve(encoded.size());
  for (size_t i = 0; i < encoded.size(); ++i) {
    if (encoded[i] != '\\' || i + 3 >= encoded.size() || encoded[i + 1] < '0' ||
        encoded[i + 1] > '7' || encoded[i + 2] < '0' || encoded[i + 2] > '7' ||
        encoded[i + 3] < '0' || encoded[i + 3] > '7') {
      decoded.push_back(encoded[i]);
      continue;
    }
    const auto value = static_cast<char>(
        (encoded[i + 1] - '0') * 64 + (encoded[i + 2] - '0') * 8 +
        (encoded[i + 3] - '0'));
    decoded.push_back(value);
    i += 3;
  }
  return decoded;
}

struct Cgroup2Mount {
  std::string root;
  std::string mountPoint;
};

std::vector<Cgroup2Mount> findCgroup2Mounts(std::string_view mountInfo) {
  std::vector<Cgroup2Mount> mounts;
  for (const auto line : splitLines(mountInfo)) {
    const auto fields = splitFields(line);
    const auto separator =
        std::find(fields.begin(), fields.end(), std::string_view{"-"});
    if (fields.size() > 4 && separator != fields.end() &&
        separator + 1 != fields.end() && separator[1] == "cgroup2") {
      mounts.push_back({
          .root = decodeMountInfoPath(fields[3]),
          .mountPoint = decodeMountInfoPath(fields[4]),
      });
    }
  }
  return mounts;
}

std::string findCgroupPath(std::string_view cgroupFile) {
  for (const auto line : splitLines(cgroupFile)) {
    if (line.starts_with("0::")) {
      const auto path = line.substr(3);
      if (path.empty() || path.front() != '/') {
        break;
      }
      return std::string{path};
    }
  }
  throw std::runtime_error{"/proc/self/cgroup has no valid cgroup v2 entry"};
}

std::string joinCgroupPath(
    std::string_view mountPoint,
    std::string_view cgroupPath) {
  std::string result{mountPoint};
  while (result.size() > 1 && result.back() == '/') {
    result.pop_back();
  }
  if (result.empty() || result.front() != '/') {
    throw std::runtime_error{
        fmt::format("invalid cgroup2 mount point: {}", mountPoint)};
  }

  while (!cgroupPath.empty()) {
    if (cgroupPath.front() == '/') {
      cgroupPath.remove_prefix(1);
      continue;
    }
    const auto slash = cgroupPath.find('/');
    const auto component = cgroupPath.substr(0, slash);
    if (component == "." || component == "..") {
      throw std::runtime_error{
          fmt::format("invalid cgroup v2 path component: {}", component)};
    }
    if (!component.empty()) {
      result = joinPath(result, component);
    }
    if (slash == std::string_view::npos) {
      break;
    }
    cgroupPath.remove_prefix(slash + 1);
  }
  return result;
}

std::string_view stripTrailingSlashes(std::string_view path) {
  while (path.size() > 1 && path.back() == '/') {
    path.remove_suffix(1);
  }
  return path;
}

std::optional<std::string_view> pathBelowMountRoot(
    std::string_view cgroupPath,
    std::string_view mountRoot) {
  mountRoot = stripTrailingSlashes(mountRoot);
  if (mountRoot.empty() || mountRoot.front() != '/' || cgroupPath.empty() ||
      cgroupPath.front() != '/') {
    return std::nullopt;
  }
  if (mountRoot == "/") {
    return cgroupPath;
  }
  if (cgroupPath == mountRoot) {
    return std::string_view{"/"};
  }
  if (cgroupPath.size() > mountRoot.size() &&
      cgroupPath.starts_with(mountRoot) &&
      cgroupPath[mountRoot.size()] == '/') {
    return cgroupPath.substr(mountRoot.size());
  }
  return std::nullopt;
}

std::string findCgroup2Directory(
    std::string_view mountInfo,
    std::string_view cgroupPath) {
  std::optional<std::string> directory;
  size_t bestRootLength = 0;
  for (const auto& mount : findCgroup2Mounts(mountInfo)) {
    const auto root = stripTrailingSlashes(mount.root);
    const auto relativePath = pathBelowMountRoot(cgroupPath, root);
    if (relativePath && (!directory || root.size() > bestRootLength)) {
      directory = joinCgroupPath(mount.mountPoint, *relativePath);
      bestRootLength = root.size();
    }
  }
  if (!directory) {
    throw std::runtime_error{fmt::format(
        "/proc/self/mountinfo has no cgroup2 mount covering {}", cgroupPath)};
  }
  return std::move(*directory);
}

std::string_view lastPathComponent(std::string_view path) {
  path = stripTrailingSlashes(path);
  const auto slash = path.rfind('/');
  return slash == std::string_view::npos ? path : path.substr(slash + 1);
}

std::string discoverCgroupDirectory(
    const std::string& procSelfCgroupPath,
    const std::string& procSelfMountInfoPath) {
  const auto cgroupPath = findCgroupPath(readTextFile(procSelfCgroupPath));
  if (!lastPathComponent(cgroupPath).starts_with("edenfs")) {
    throw NotEdenFsCgroupError{
        fmt::format("cgroup {} is not an EdenFS cgroup", cgroupPath)};
  }
  return findCgroup2Directory(readTextFile(procSelfMountInfoPath), cgroupPath);
}

void validateCgroup(const std::string& directory) {
  const auto typePath = joinPath(directory, "cgroup.type");
  if (trim(readTextFile(typePath)) != "domain") {
    throw std::runtime_error{
        fmt::format("{} is not a domain cgroup", directory)};
  }

  const auto statPath = joinPath(directory, "cgroup.stat");
  const auto descendants =
      readNamedValue(readTextFile(statPath), "nr_descendants", statPath);
  if (descendants != 0) {
    throw std::runtime_error{
        fmt::format("{} has {} descendant cgroups", directory, descendants)};
  }

  const auto procsPath = joinPath(directory, "cgroup.procs");
  const auto procs = readTextFile(procsPath);
  const auto pid = static_cast<uint64_t>(getpid());
  bool containsCurrentProcess = false;
  for (const auto line : splitLines(procs)) {
    if (!trim(line).empty() && parseUint64(line, procsPath) == pid) {
      containsCurrentProcess = true;
      break;
    }
  }
  if (!containsCurrentProcess) {
    throw std::runtime_error{
        fmt::format("{} does not contain EdenFS pid {}", directory, pid)};
  }
}

uint64_t readFileCacheBytes(const std::string& directory) {
  const auto path = joinPath(directory, "memory.stat");
  const auto contents = readTextFile(path);
  const auto active = readNamedValue(contents, "active_file", path);
  const auto inactive = readNamedValue(contents, "inactive_file", path);
  if (active > std::numeric_limits<uint64_t>::max() - inactive) {
    throw std::overflow_error{
        fmt::format("file cache size overflows in {}", path)};
  }
  return active + inactive;
}

uint64_t calculateReclaimBytes(
    uint64_t fileCacheBytes,
    const CgroupFileCacheReclaimOptions& options) {
  if (fileCacheBytes <= options.targetBytes) {
    return 0;
  }
  const auto excess = fileCacheBytes - options.targetBytes;
  if (options.maxReclaimBytes == 0) {
    return excess;
  }
  return std::min(excess, options.maxReclaimBytes);
}

} // namespace

CgroupFileCacheReclaimer::CgroupFileCacheReclaimer(
    std::string procSelfCgroupPath,
    std::string procSelfMountInfoPath)
    : procSelfCgroupPath_{std::move(procSelfCgroupPath)},
      procSelfMountInfoPath_{std::move(procSelfMountInfoPath)} {}

CgroupFileCacheReclaimResult CgroupFileCacheReclaimer::reclaim(
    const CgroupFileCacheReclaimOptions& options) const {
  const auto directory =
      discoverCgroupDirectory(procSelfCgroupPath_, procSelfMountInfoPath_);
  validateCgroup(directory);

  const auto before = readFileCacheBytes(directory);
  const auto requested = calculateReclaimBytes(before, options);
  if (requested == 0) {
    return {before, before, 0};
  }

  const auto reclaimPath = joinPath(directory, "memory.reclaim");
  const int fd = ::open(reclaimPath.c_str(), O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    const auto openError = errno;
    if (openError == ENOENT) {
      throw UnsupportedKernelError{fmt::format(
          "{} does not exist; memory.reclaim needs Linux 5.19", reclaimPath)};
    }
    throw std::system_error{
        openError,
        std::generic_category(),
        fmt::format("opening {}", reclaimPath)};
  }
  folly::File reclaimFile{fd, /*ownsFd=*/true};

  const auto request = fmt::format("{} swappiness=0", requested);
  // A single write, deliberately not retried. The kernel reclaims as it goes
  // and answers EAGAIN when it fell short of the full amount or EINTR when a
  // signal cut the pass off; either way the before and after sizes show what
  // happened, and writing again would ask for the full amount on top of what
  // was already reclaimed.
  const auto written =
      ::write(reclaimFile.fd(), request.data(), request.size());
  const auto writeError = written < 0 ? errno : 0;
  if (writeError == EINVAL) {
    throw UnsupportedKernelError{fmt::format(
        "{} rejected '{}'; the swappiness argument needs Linux 6.6",
        reclaimPath,
        request)};
  }
  if (written < 0 && writeError != EAGAIN && writeError != EINTR) {
    throw std::system_error{
        writeError,
        std::generic_category(),
        fmt::format("writing {}", reclaimPath)};
  }
  if (written >= 0 && static_cast<size_t>(written) != request.size()) {
    throw std::runtime_error{fmt::format("short write to {}", reclaimPath)};
  }

  const auto after = readFileCacheBytes(directory);
  return {before, after, requested};
}

} // namespace facebook::eden

#endif // __linux__
