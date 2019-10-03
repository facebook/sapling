/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/service/ThriftPermissionChecker.h"

#include <thrift/lib/cpp2/server/Cpp2ConnContext.h>
#include "eden/fs/fuse/privhelper/UserInfo.h"

namespace {
/**
 * Any user can call these methods.
 */
constexpr folly::StringPiece METHOD_WHITELIST[] = {
    "BaseService.getCounter",
    "BaseService.getCounters",
    "BaseService.getRegexCounters",
    "BaseService.getSelectedCounters",
};

/**
 * A linear scan is faster than a non-lexical binary search until about 10
 * entries, and faster than a hash (libstdc++ uses murmurhash32) lookup until
 * about 30 entries, and a linear scan does not require a global constructor.
 *
 * All of the implementations I benchmarked finish in under 100 ns, so perhaps
 * it doesn't matter.
 */
bool isWhitelisted(folly::StringPiece methodName) {
  for (auto& name : METHOD_WHITELIST) {
    if (methodName == name) {
      return true;
    }
  }
  return false;
}

} // namespace

namespace facebook {
namespace eden {

ThriftPermissionChecker::ThriftPermissionChecker(const UserInfo& userInfo)
#ifndef _WIN32
    : processOwner_ {
  userInfo.getUid()
}
#endif
{}

void* ThriftPermissionChecker::getContext(
    const char* /*fn_name*/,
    apache::thrift::TConnectionContext* connectionContext) {
  return connectionContext;
}

void ThriftPermissionChecker::freeContext(
    void* /*ctx*/,
    const char* /*fn_name*/) {
  // We don't own the connectionContext.
}

void ThriftPermissionChecker::preRead(void* ctx, const char* fn_name) {
  if (isWhitelisted(fn_name)) {
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
  folly::Optional<uid_t> maybeUid = connectionContext->getPeerEffectiveUid();
  if (!maybeUid) {
    if (auto error = connectionContext->getPeerCredError()) {
      throw NotAuthorized{folly::to<std::string>(
          "error retrieving unix domain socket peer: ", *error)};
    } else {
      // Either not a unix domain socket, or platform does not support
      // retrieving peer credentials.
      throw NotAuthorized{"unknown peer user for unix domain socket"};
    }
  }
  uid_t uid = *maybeUid;

  if (uid == 0 || uid == processOwner_) {
    return;
  }

  throw NotAuthorized{folly::to<std::string>(
      "user ", uid, " not authorized to invoke method ", fn_name)};
#endif
}

} // namespace eden
} // namespace facebook
