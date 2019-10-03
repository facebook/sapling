/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <thrift/lib/cpp/TProcessorEventHandler.h>
#include <stdexcept>

namespace facebook {
namespace eden {

class UserInfo;

class NotAuthorized : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

/**
 * Throws NotAuthorized in preRead if process connected to Eden's unix domain
 * socket has an effective uid not allowed to access a given Thrift method.
 */
class ThriftPermissionChecker : public apache::thrift::TProcessorEventHandler {
 public:
  explicit ThriftPermissionChecker(const UserInfo& userInfo);

  void* getContext(
      const char* fn_name,
      apache::thrift::TConnectionContext* connectionContext) override;
  void freeContext(void* ctx, const char* fn_name) override;

  void preRead(void* ctx, const char* fn_name) override;

 private:
#ifndef _WIN32
  const uid_t processOwner_;
#endif
};

} // namespace eden
} // namespace facebook
