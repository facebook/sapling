/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/PathFuncs.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <glog/logging.h>

#include <folly/Exception.h>
#include <folly/portability/Stdlib.h>
#include <optional>
#ifdef _WIN32
#include <folly/portability/Unistd.h>
#else
#include <unistd.h>
#endif

using folly::Expected;
using folly::StringPiece;

namespace facebook {
namespace eden {

StringPiece dirname(StringPiece path) {
  auto slash = path.rfind('/');
  if (slash != std::string::npos) {
    return path.subpiece(0, slash);
  }
  return "";
}

StringPiece basename(StringPiece path) {
  auto slash = path.rfind('/');
  if (slash != std::string::npos) {
    path.advance(slash + 1);
    return path;
  }
  return path;
}

AbsolutePath getcwd() {
  char cwd[PATH_MAX];
  if (!::getcwd(cwd, sizeof(cwd))) {
    folly::throwSystemError("getcwd() failed");
  }
  return AbsolutePath{cwd};
}

namespace {
struct CanonicalData {
  std::vector<StringPiece> components;
  bool isAbsolute{false};
};

/**
 * Parse path into a collection of path components such that:
 * - "." (single dot) and "" (empty) components are discarded.
 * - ".." component either destructively combines with the last
 *   parsed path component, or becomes the first component when
 *   the vector of previously extracted components is empty.
 */
CanonicalData canonicalPathData(StringPiece path) {
  CanonicalData data;
  const char* componentStart = path.begin();
  auto processSlash = [&](const char* end) {
    auto component = StringPiece{componentStart, end};
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
      data.components.push_back(component);
    }
  };

  for (const char* p = path.begin(); p != path.end(); ++p) {
    if (*p == kDirSeparator) {
      processSlash(p);
    }
  }
  if (componentStart != path.end()) {
    processSlash(path.end());
  }

  return data;
}

AbsolutePath canonicalPathImpl(
    StringPiece path,
    std::optional<AbsolutePathPiece> base) {
  auto makeAbsolutePath = [](const std::vector<StringPiece>& parts) {
    if (parts.empty()) {
      return AbsolutePath{};
    }

    size_t length = 1; // reserve 1 byte for terminating '\0'
    for (const auto& part : parts) {
      length += part.size();
    }

    std::string value;
    value.reserve(length);
    for (const auto& part : parts) {
      value.push_back('/');
      value.append(part.begin(), part.end());
    }

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
    // canonicalPathData() returns StringPieces pointing to the input,
    // so we have to store the cwd in a variable that will persist until the
    // end of this function.
    cwd = getcwd();
    baseCanon = canonicalPathData(cwd.stringPiece());
  } else {
    baseCanon = canonicalPathData(base.value().stringPiece());
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

AbsolutePath canonicalPath(folly::StringPiece path) {
  // Pass in std::nullopt.
  // canonicalPathImpl() will only call getcwd() if it is actually necessary.
  return canonicalPathImpl(path, std::nullopt);
}

AbsolutePath canonicalPath(folly::StringPiece path, AbsolutePathPiece base) {
  return canonicalPathImpl(path, std::optional<AbsolutePathPiece>{base});
}

folly::Expected<RelativePath, int> joinAndNormalize(
    RelativePathPiece base,
    folly::StringPiece path) {
  if (path.startsWith(kDirSeparator)) {
    return folly::makeUnexpected(EPERM);
  }
  const std::string joined = base.value().empty()
      ? path.str()
      : path.empty() ? base.value().str()
                     : folly::to<std::string>(base, kDirSeparator, path);
  const CanonicalData cdata{canonicalPathData(joined)};
  const auto& parts{cdata.components};
  DCHECK(!cdata.isAbsolute);
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

  return folly::makeExpected<int>(AbsolutePath{pathBuffer});
}

Expected<AbsolutePath, int> realpathExpected(StringPiece path) {
  // The input may not be nul-terminated, so we have to construct a std::string
  return realpath(path.str().c_str());
}

AbsolutePath realpath(const char* path) {
  auto result = realpathExpected(path);
  if (!result) {
    folly::throwSystemErrorExplicit(
        result.error(), "realpath(", path, ") failed");
  }
  return result.value();
}

AbsolutePath realpath(StringPiece path) {
  // The input may not be nul-terminated, so we have to construct a std::string
  return realpath(path.str().c_str());
}

AbsolutePath normalizeBestEffort(const char* path) {
  auto result = realpathExpected(path);
  if (result) {
    return result.value();
  }

  return canonicalPathImpl(path, std::nullopt);
}

AbsolutePath normalizeBestEffort(folly::StringPiece path) {
  return normalizeBestEffort(path.str().c_str());
}

std::pair<PathComponentPiece, RelativePathPiece> splitFirst(
    RelativePathPiece path) {
  auto piece = path.stringPiece();
  auto p = piece.find(kDirSeparator);
  if (p != std::string::npos) {
    return {PathComponentPiece{
                folly::StringPiece{piece.begin(), piece.begin() + p}},
            RelativePathPiece{
                folly::StringPiece{piece.begin() + p + 1, piece.end()}}};
  } else {
    return {PathComponentPiece{piece}, RelativePathPiece{}};
  }
}

void validatePathComponentLength(PathComponentPiece name) {
  if (name.value().size() > kMaxPathComponentLength) {
    folly::throwSystemErrorExplicit(
        ENAMETOOLONG, "path component too long: ", name);
  }
}

namespace {
boost::filesystem::path toBoostPath(AbsolutePathPiece path) {
  auto piece = path.stringPiece();
  return boost::filesystem::path(piece.begin(), piece.end());
}
} // namespace

bool ensureDirectoryExists(AbsolutePathPiece path) {
  return boost::filesystem::create_directories(toBoostPath(path));
}

bool removeRecursively(AbsolutePathPiece path) {
  return boost::filesystem::remove_all(toBoostPath(path));
}

AbsolutePath expandUser(
    folly::StringPiece path,
    std::optional<folly::StringPiece> homeDir) {
  if (!path.startsWith("~")) {
    return canonicalPath(path);
  }

  if (path.size() > 1 && !path.startsWith("~/")) {
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

  // Otherwise: we know the path startsWith("~/") due to the
  // checks made above, so we can skip the first 2 characters
  // to build the expansion here.

  auto expanded =
      folly::to<std::string>(*homeDir, kDirSeparator, path.subpiece(2));
  return canonicalPath(expanded);
}
} // namespace eden
} // namespace facebook
