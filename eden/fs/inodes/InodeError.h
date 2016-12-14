/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Format.h>
#include <folly/Optional.h>
#include <folly/Synchronized.h>
#include <memory>
#include <system_error>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class InodeBase;
class TreeInode;
using InodePtr = std::shared_ptr<InodeBase>;
using TreeInodePtr = std::shared_ptr<TreeInode>;

/**
 * A subclass of std::system_error referring to a specific inode.
 *
 * The main advantage of this class is that it can include the Inode path in
 * the error message.  However, it avoids computing the path until the error
 * message is actually needed.  If the error is caught and handled without
 * looking at the error message, then the path never needs to be computed.
 */
class InodeError : public std::system_error {
 public:
  InodeError(int errnum, InodePtr inode)
      : std::system_error(errnum, std::system_category()),
        inode_(std::move(inode)) {}
  InodeError(int errnum, TreeInodePtr inode, PathComponentPiece child);
  InodeError(int errnum, InodePtr inode, std::string message)
      : std::system_error(errnum, std::system_category()),
        inode_(std::move(inode)),
        message_(std::move(message)) {}
  InodeError(
      int errnum,
      TreeInodePtr inode,
      PathComponentPiece child,
      std::string&& message);
  template <typename... Args>
  InodeError(
      int errnum,
      InodePtr inode,
      folly::StringPiece format,
      Args&&... args)
      : InodeError(
            errnum,
            inode,
            folly::sformat(format, std::forward<Args>(args)...)) {}
  template <typename... Args>
  InodeError(
      int errnum,
      TreeInodePtr inode,
      PathComponentPiece child,
      folly::StringPiece format,
      Args&&... args)
      : InodeError(
            errnum,
            inode,
            child,
            message_(folly::sformat(format, std::forward<Args>(args)...))) {}

  const char* what() const noexcept override;

  int errnum() const {
    return code().value();
  }

  InodeError(InodeError const&) = default;
  InodeError& operator=(InodeError const&) = default;
  InodeError(InodeError&&) = default;
  InodeError& operator=(InodeError&&) = default;

 private:
  std::string computeMessage() const;

  InodePtr inode_;
  folly::Optional<PathComponent> child_;
  std::string message_;
  mutable folly::Synchronized<std::string> fullMessage_;
};
}
}
