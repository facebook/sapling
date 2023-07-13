/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/XAttr.h"
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/String.h>

#ifndef _WIN32

namespace facebook::eden {

std::string getxattr(std::string_view path, std::string_view name) {
  folly::File file(path, O_RDONLY);
  return fgetxattr(file.fd(), name);
}

std::string fgetxattr(int fd, std::string_view name) {
  std::string result;

  // Reasonable ballpark for most attributes we might want; this saves
  // us from an extra syscall to query the size in the common case.
  result.resize(64, 0);

  std::string namestr{name};

  // We loop until we either hit a hard error or succeed in extracting
  // the requested information.
  while (true) {
    // First, try to read into the buffer at its existing size.
    auto size = ::fgetxattr(
        fd,
        namestr.c_str(),
        &result[0],
        result.size()
#ifdef __APPLE__
            ,
        0, // position
        0 // options
#endif
    );
    if (size != -1) {
      result.resize(size);
      return result;
    }

    // ERANGE means that the buffer wasn't large enough.  Any other
    // error terminates our attempt to get the attribute.
    if (errno != ERANGE) {
      folly::throwSystemError("fgetxattr");
    }

    // Got the wrong size, query to find out what we should have used
    size = ::fgetxattr(
        fd,
        namestr.c_str(),
        nullptr,
        0
#ifdef __APPLE__
        ,
        0, // position
        0 // options
#endif
    );
    if (size < 0) {
      folly::throwSystemError("fgetxattr to query the size failed");
    }

    // Make sure we have room for a trailing NUL byte.
    result.resize(size + 1, 0);
  }
}

void fsetxattr(int fd, std::string_view name, std::string_view value) {
  std::string namestr{name};

  folly::checkUnixError(::fsetxattr(
      fd,
      namestr.c_str(),
      value.data(),
      value.size()
#ifdef __APPLE__
          ,
      0 // position
#endif
      ,
      0 // allow create and replace
      ));
}

std::vector<std::string> listxattr(std::string_view path) {
  std::string buf;
  std::string pathStr{path};

  buf.resize(128, 0);

  while (true) {
    auto size = ::listxattr(
        pathStr.c_str(),
        &buf[0],
        buf.size()
#ifdef __APPLE__
            ,
        XATTR_NOFOLLOW
#endif
    );

    if (size != -1) {
      // Success; parse the result in a list of names separated by NUL
      // bytes, terminated by a NUL byte.
      std::vector<std::string> result;
      // Don't include the final terminator in the size, as that just causes
      // the split array to contain a final empty name.
      folly::split('\0', std::string_view(buf.data(), size - 1), result);
      return result;
    }

    if (errno != ERANGE) {
      folly::throwSystemError("listxattr");
    }

    // Query for the size
    size = ::listxattr(
        pathStr.c_str(),
        nullptr,
        0
#ifdef __APPLE__
        ,
        XATTR_NOFOLLOW
#endif
    );

    if (size == -1) {
      folly::throwSystemError("listxattr");
    }

    buf.resize(size, 0);
  }
}

} // namespace facebook::eden

#endif
