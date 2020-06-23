/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/IObjectStore.h"

namespace facebook {
namespace eden {

class LoggingFetchContext : public ObjectFetchContext {
 public:
  struct Request {
    Request(ObjectType t, Hash h, Origin o) : type{t}, hash{h}, origin{o} {}

    ObjectType type;
    Hash hash;
    Origin origin;
  };

  void didFetch(ObjectType type, const Hash& hash, Origin origin) override {
    requests.emplace_back(type, hash, origin);
  }

  std::optional<pid_t> getClientPid() const override {
    return std::nullopt;
  }

  Cause getCause() const override {
    return Cause::Unknown;
  }

  std::vector<Request> requests;
};

} // namespace eden
} // namespace facebook
