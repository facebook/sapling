/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftPermissionChecker.h"

#include <thrift/lib/cpp2/server/Cpp2ConnContext.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/ServerState.h"

namespace {

/**
 * A linear scan is faster than a non-lexical binary search until about 10
 * entries, and faster than a hash (libstdc++ uses murmurhash32) lookup until
 * about 30 entries, and a linear scan does not require a global constructor.
 *
 * All of the implementations I benchmarked finish in under 100 ns, so perhaps
 * it doesn't matter.
 */
bool isAllowlisted(
    std::string_view methodName,
    const std::vector<std::string>& methodAllowlist) {
  for (auto& name : methodAllowlist) {
    if (methodName == name) {
      return true;
    }
  }
  return false;
}

} // namespace

namespace facebook::eden {

ThriftPermissionChecker::ThriftPermissionChecker(
    std::shared_ptr<ServerState> serverState)
    : serverState_{std::move(serverState)} {}

void* ThriftPermissionChecker::getContext(
    std::string_view /*fn_name*/,
    apache::thrift::TConnectionContext* connectionContext) {
  return connectionContext;
}

void ThriftPermissionChecker::freeContext(
    void* /*ctx*/,
    std::string_view /*fn_name*/) {
  // We don't own the connectionContext.
}

void ThriftPermissionChecker::preRead(void* ctx, std::string_view fn_name) {
  if (isAllowlisted(
          fn_name,
          serverState_->getReloadableConfig()
              ->getEdenConfig()
              ->thriftFunctionsAllowlist.getValue())) {
    return;
  }

  auto* requestContext = dynamic_cast<apache::thrift::Cpp2RequestContext*>(
      reinterpret_cast<apache::thrift::TConnectionContext*>(ctx));
  if (!requestContext) {
    throw NotAuthorized{"unknown request context"};
  }
  auto* connectionContext = requestContext->getConnectionContext();

  auto* peerAddress = connectionContext->getPeerAddress();
  if (!peerAddress) {
    throw NotAuthorized{"unknown peer address"};
  }

  if (AF_UNIX != peerAddress->getFamily()) {
    throw NotAuthorized{
        "Permission checking on non-unix sockets is not implemented"};
  }

#ifdef _WIN32
  // There is no way to retrieve peer credentials on Windows, so assume all
  // AF_UNIX connections are okay.
  return;
#else
  auto maybePeerCreds = connectionContext->getPeerEffectiveCreds();
  if (!maybePeerCreds) {
    if (auto error = connectionContext->getPeerCredError()) {
      throw NotAuthorized{folly::to<std::string>(
          "error retrieving unix domain socket peer: ", *error)};
    } else {
      // Either not a unix domain socket, or platform does not support
      // retrieving peer credentials.
      throw NotAuthorized{"unknown peer user for unix domain socket"};
    }
  }
  const auto& peerCreds = *maybePeerCreds;

  uid_t processOwner = serverState_->getUserInfo().getUid();

  if (peerCreds.uid == 0 || peerCreds.uid == processOwner) {
    return;
  }

  throw NotAuthorized{folly::to<std::string>(
      "user ", peerCreds.uid, " not authorized to invoke method ", fn_name)};
#endif
}

} // namespace facebook::eden
