/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "eden/fs/telemetry/EdenComponent.h"

namespace facebook::eden {

class EdenErrorInfoBuilder;
class ErrorArg;

struct SourceInfo {
  const char* file;
  int line;
  const char* func;

  static SourceInfo current(
      const char* f = __builtin_FILE(),
      int l = __builtin_LINE(),
      const char* fn = __builtin_FUNCTION()) {
    return {f, l, fn};
  }
};

class EdenErrorInfo {
 public:
  EdenComponent component;
  std::string errorMessage;
  std::optional<int64_t> errorCode;
  std::optional<std::string> errorName;
  std::optional<std::string> exceptionType;
  std::optional<std::string> stackTrace;
  std::optional<std::string> clientCommandName;
  std::optional<uint64_t> inode;
  std::optional<std::string> mountPoint;

  // Per-component factory methods.
  // Return an EdenErrorInfoBuilder for optional chaining (withMountPoint, etc.)
  // before calling create() to produce the final EdenErrorInfo.

  static EdenErrorInfoBuilder fuse(
      const ErrorArg& error,
      uint64_t inode,
      std::string mountPoint,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder nfs(
      const ErrorArg& error,
      uint64_t inode,
      std::string mountPoint,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder overlay(
      const ErrorArg& error,
      uint64_t inode,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder thrift(
      const ErrorArg& error,
      std::string clientCommandName,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder prjfs(
      const ErrorArg& error,
      std::string mountPoint,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder backingStore(
      const ErrorArg& error,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder objectStore(
      const ErrorArg& error,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder takeover(
      const ErrorArg& error,
      SourceInfo loc = SourceInfo::current());

  static EdenErrorInfoBuilder privhelper(
      const ErrorArg& error,
      SourceInfo loc = SourceInfo::current());
};

} // namespace facebook::eden
