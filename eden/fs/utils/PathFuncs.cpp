/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/PathFuncs.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>

#include <folly/Exception.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Stdlib.h>
#include <folly/portability/Unistd.h>

#include <optional>

#ifdef __APPLE__
#include <mach-o/dyld.h> // @manual
#endif

using folly::Expected;

namespace facebook::eden {

std::string_view dirname(std::string_view path) {
  auto dirSeparator = detail::rfindPathSeparator(path);

  if (dirSeparator != std::string::npos) {
    return path.substr(0, dirSeparator);
  }
  return "";
}

std::string_view basename(std::string_view path) {
  auto dirSeparator = detail::rfindPathSeparator(path);

  if (dirSeparator != std::string::npos) {
    return path.substr(dirSeparator + 1);
  }
  return path;
}

AbsolutePath getcwd() {
  char cwd[PATH_MAX];
  if (!::getcwd(cwd, sizeof(cwd))) {
    folly::throwSystemError("getcwd() failed");
  }
  return canonicalPath(cwd);
}

namespace {
struct CanonicalData {
  std::vector<std::string_view> components;
  bool isAbsolute{false};
};

bool startsWithUNC(string_view path) {
  return folly::kIsWindows && path.starts_with(detail::kUNCPrefix);
}

/**
 * Parse path into a collection of path components such that:
 * - "." (single dot) and "" (empty) components are discarded.
 * - ".." component either destructively combines with the last
 *   parsed path component, or becomes the first component when
 *   the vector of previously extracted components is empty.
 */
CanonicalData canonicalPathData(std::string_view path) {
  CanonicalData data;

  if (startsWithUNC(path)) {
    path = path.substr(detail::kUNCPrefix.size());
    data.isAbsolute = true;
  }

  const char* componentStart = path.data();
  auto processSlash = [&](const char* end) {
    auto component = detail::string_view_range(componentStart, end);
    componentStart = end + 1;
    if (component.empty()) {
      // Ignore empty components (doubled slash characters)
      // An empty component at the start of the string indicates an
      // absolute path.
      //
      // (POSIX specifies that "//" at the start of a path is special, and has
      // platform-specific behavior.  We intentionally ignore that, and treat a
      // leading "//" the same as a single leading "/".)
      if (component.begin() == path.begin()) {
        data.isAbsolute = true;
      }
    } else if (component == ".") {
      // ignore this component
    } else if (component == "..") {
      if (data.components.empty()) {
        if (!data.isAbsolute) {
          // We have no choice but to add ".." to the start
          data.components.push_back(component);
        }
      } else if (data.components.back() != "..") {
        data.components.pop_back();
      }
    } else {
      if (folly::kIsWindows && component.begin() == path.begin()) {
        // Drive letter paths are absolute.
        if (component.size() == 2 && std::isalpha(component[0]) &&
            component[1] == ':') {
          data.isAbsolute = true;
        }
      }
      data.components.push_back(component);
    }
  };

  for (const char* p = path.data(); p != path.data() + path.size(); ++p) {
    if (detail::isDirSeparator(*p)) {
      processSlash(p);
    }
  }
  if (componentStart != path.data() + path.size()) {
    processSlash(path.data() + path.size());
  }

  return data;
}

AbsolutePath canonicalPathImpl(
    std::string_view path,
    std::optional<AbsolutePathPiece> base) {
  auto makeAbsolutePath = [](const std::vector<std::string_view>& parts) {
    if (parts.empty()) {
      return AbsolutePath{};
    }

    size_t length = 1; // reserve 1 byte for terminating '\0'
    for (const auto& part : parts) {
      length += part.size() + 1; // +1 for the path separator
    }

    length += detail::kRootStr.size();

    std::string value;
    value.reserve(length);

    value.append(detail::kRootStr.begin(), detail::kRootStr.end());
    fmt::format_to(
        std::back_inserter(value),
        "{}",
        fmt::join(parts, std::string_view{&kAbsDirSeparator, 1}));

    return AbsolutePath{std::move(value)};
  };

  auto canon = canonicalPathData(path);
  if (canon.isAbsolute) {
    return makeAbsolutePath(canon.components);
  }

  // Get the components from the base path
  // For simplicity we are just re-using canonicalPathData() even though the
  // base path is guaranteed to already be in canonical form.
  CanonicalData baseCanon;
  AbsolutePath cwd;
  if (!base.has_value()) {
    // canonicalPathData() returns std::string_views pointing to the input,
    // so we have to store the cwd in a variable that will persist until the
    // end of this function.
    cwd = getcwd();
    baseCanon = canonicalPathData(cwd.view());
  } else {
    baseCanon = canonicalPathData(base.value().view());
  }

  for (auto it = canon.components.begin(); it != canon.components.end(); ++it) {
    // There may be leading ".." parts, so we have to deal with them here
    if (*it == "..") {
      if (!baseCanon.components.empty()) {
        baseCanon.components.pop_back();
      }
    } else {
      // Once we found a non-".." component, none of the rest can be "..",
      // so add everything else and break out of the loop
      baseCanon.components.insert(
          baseCanon.components.end(), it, canon.components.end());
      break;
    }
  }

  return makeAbsolutePath(baseCanon.components);
}
} // namespace

AbsolutePath canonicalPath(std::string_view path) {
  // Pass in std::nullopt.
  // canonicalPathImpl() will only call getcwd() if it is actually necessary.
  return canonicalPathImpl(path, std::nullopt);
}

AbsolutePath canonicalPath(std::string_view path, AbsolutePathPiece base) {
  return canonicalPathImpl(path, std::optional<AbsolutePathPiece>{base});
}

folly::Expected<RelativePath, int> joinAndNormalize(
    RelativePathPiece base,
    string_view path) {
  if (path.starts_with(kDirSeparator)) {
    return folly::makeUnexpected(EPERM);
  }
  const std::string joined = base.value().empty() ? std::string{path}
      : path.empty()                              ? std::string{base.value()}
                     : fmt::format("{}{}{}", base, kDirSeparator, path);
  const CanonicalData cdata{canonicalPathData(joined)};
  const auto& parts{cdata.components};
  XDCHECK(!cdata.isAbsolute);
  if (!parts.empty() && parts[0] == "..") {
    return folly::makeUnexpected(EXDEV);
  } else {
    return folly::makeExpected<int>(RelativePath{parts.begin(), parts.end()});
  }
}

Expected<AbsolutePath, int> realpathExpected(const char* path) {
  auto pathBuffer = ::realpath(path, nullptr);
  if (!pathBuffer) {
    return folly::makeUnexpected(errno);
  }
  SCOPE_EXIT {
    free(pathBuffer);
  };

  return folly::makeExpected<int>(canonicalPath(pathBuffer));
}

Expected<AbsolutePath, int> realpathExpected(string_view path) {
  // The input may not be nul-terminated, so we have to construct a std::string
  return realpath(std::string{path}.c_str());
}

AbsolutePath realpath(const char* path) {
  auto result = realpathExpected(path);
  if (!result) {
    folly::throwSystemErrorExplicit(
        result.error(), "realpath(", path, ") failed");
  }
  return result.value();
}

AbsolutePath realpath(std::string_view path) {
  // The input may not be nul-terminated, so we have to construct a std::string
  return realpath(std::string{path}.c_str());
}

AbsolutePath normalizeBestEffort(const char* path) {
  auto result = realpathExpected(path);
  if (result) {
    return result.value();
  }

  return canonicalPathImpl(path, std::nullopt);
}

AbsolutePath normalizeBestEffort(std::string_view path) {
  return normalizeBestEffort(std::string{path}.c_str());
}

std::pair<PathComponentPiece, RelativePathPiece> splitFirst(
    RelativePathPiece path) {
  auto piece = path.view();
  auto dirSeparator = detail::findPathSeparator(piece);

  if (dirSeparator != std::string::npos) {
    return {
        PathComponentPiece{std::string_view{piece.data(), dirSeparator}},
        RelativePathPiece{detail::string_view_range(
            piece.data() + dirSeparator + 1, piece.data() + piece.size())}};
  } else {
    return {PathComponentPiece{piece}, RelativePathPiece{}};
  }
}

void validatePathComponentLength(PathComponentPiece name) {
  if (name.value().size() > kMaxPathComponentLength) {
    folly::throwSystemErrorExplicit(
        ENAMETOOLONG, fmt::format("path component too long: {}", name));
  }
}

namespace {
boost::filesystem::path asBoostPath(AbsolutePathPiece path) {
  return boost::filesystem::path{path.asString()};
}
} // namespace

bool ensureDirectoryExists(AbsolutePathPiece path) {
  return boost::filesystem::create_directories(asBoostPath(path));
}

bool ensureDirectoryExists(
    AbsolutePathPiece path,
    boost::system::error_code& error) noexcept {
  return boost::filesystem::create_directories(asBoostPath(path), error);
}

bool removeRecursively(AbsolutePathPiece path) {
  return boost::filesystem::remove_all(asBoostPath(path));
}

bool removeFileWithAbsolutePath(AbsolutePathPiece path) {
  return boost::filesystem::remove(asBoostPath(path));
}

void renameWithAbsolutePath(
    AbsolutePathPiece srcPath,
    AbsolutePathPiece destPath) {
  boost::filesystem::rename(asBoostPath(srcPath), asBoostPath(destPath));
}

AbsolutePath expandUser(
    string_view path,
    std::optional<std::string_view> homeDir) {
  if (!path.starts_with("~")) {
    return canonicalPath(path);
  }

  if (path.size() > 1 && !path.starts_with("~/")) {
    // path is not "~" and doesn't start with "~/".
    // Most likely the input is something like "~user" which
    // we don't support.
    throw std::runtime_error(folly::to<std::string>(
        "expandUser: can only ~-expand the current user. Input path was: `",
        path,
        "`"));
  }

  if (!homeDir) {
    throw std::runtime_error(
        "Unable to expand ~ in path because homeDir is not set");
  }

  if (homeDir->size() == 0) {
    throw std::runtime_error(
        "Unable to expand ~ in path because homeDir is the empty string");
  }

  if (path == "~") {
    return canonicalPath(*homeDir);
  }

  // Otherwise: we know the path starts_with("~/") due to the
  // checks made above, so we can skip the first 2 characters
  // to build the expansion here.

  auto expanded =
      folly::to<std::string>(*homeDir, kDirSeparator, path.substr(2));
  return canonicalPath(expanded);
}

AbsolutePath executablePath() {
#ifdef __linux__
  // The maximum symlink limit is filesystem dependent, but many common Linux
  // filesystems have a limit of 4096.
  constexpr size_t pathMax = 4096;
  std::array<char, pathMax> buf;
  auto result = readlink("/proc/self/exe", buf.data(), buf.size());
  folly::checkUnixError(result, "failed to read /proc/self/exe");
  return AbsolutePath(
      std::string_view(buf.data(), static_cast<size_t>(result)));
#elif defined(__APPLE__)
  std::vector<char> buf;
  buf.resize(4096, 0);
  uint32_t size = buf.size();
  if (_NSGetExecutablePath(buf.data(), &size) != 0) {
    buf.resize(size, 0);
    if (_NSGetExecutablePath(buf.data(), &size) != 0) {
      throw std::runtime_error("_NSGetExecutablePath failed");
    }
  }
  // Note that on success, the size is not updated and we need to look
  // for NUL termination
  return AbsolutePath(std::string_view(buf.data()));
#elif defined(_WIN32)
  std::vector<WCHAR> buf;
  buf.resize(4096);
  auto res =
      GetModuleFileNameW(NULL, buf.data(), static_cast<DWORD>(buf.size()));
  while (res == buf.size()) {
    buf.resize(buf.size() * 2);
    res = GetModuleFileNameW(NULL, buf.data(), static_cast<DWORD>(buf.size()));
  }
  if (res == 0) {
    auto err = GetLastError();
    throw std::system_error(err, std::system_category(), "GetModuleFileNameW");
  }
  auto execPath = wideToMultibyteString<std::string>(
      std::wstring_view(buf.data(), static_cast<size_t>(res)));
  return normalizeBestEffort(execPath);
#else
#error executablePath not implemented
#endif
}

} // namespace facebook::eden
