/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "XAttr.h"
#include <folly/Exception.h>

namespace facebook {
namespace eden {

std::string fgetxattr(int fd, folly::StringPiece name) {
  std::string result;

  // Reasonable ballpark for most attributes we might want; this saves
  // us from an extra syscall to query the size in the common case.
  result.resize(64, 0);

  auto namestr = name.str();

  // We loop until we either hit a hard error or succeed in extracting
  // the requested information.
  while (true) {
    // First, try to read into the buffer at its existing size.
    auto size = ::fgetxattr(
        fd,
        namestr.c_str(),
        &result[0],
        result.capacity()
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

void fsetxattr(int fd, folly::StringPiece name, folly::StringPiece value) {
  auto namestr = name.str();

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
}
}
