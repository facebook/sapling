/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <thrift/lib/cpp/TProcessorEventHandler.h>
#include <stdexcept>

namespace facebook::eden {

class ServerState;

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
  explicit ThriftPermissionChecker(std::shared_ptr<ServerState> serverState);

  void* getContext(
      std::string_view fn_name,
      apache::thrift::TConnectionContext* connectionContext) override;
  void freeContext(void* ctx, std::string_view fn_name) override;

  void preRead(void* ctx, std::string_view fn_name) override;

 private:
  std::shared_ptr<ServerState> serverState_;
};

} // namespace facebook::eden
