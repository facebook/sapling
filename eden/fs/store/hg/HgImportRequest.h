/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Promise.h>
#include <utility>
#include <variant>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ImportPriority.h"

namespace facebook {
namespace eden {

/**
 * Represents an Hg import request. This class contains all the necessary
 * information needed to fulfill the request as well as a promise that will be
 * resolved after the requested data is imported.
 */
class HgImportRequest {
 public:
  enum RequestType : uint8_t {
    BlobImport,
    TreeImport,
  };

  static std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Blob>>>
  makeBlobImportRequest(Hash hash, ImportPriority priority);

  static std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
  makeTreeImportRequest(Hash hash, ImportPriority priority);

  RequestType getType() const {
    return type_;
  }

  const Hash& getHash() const {
    return hash_;
  }

  /**
   * Set Try with the Future for the inner promise.
   *
   * We need this method instead of letting the caller directly call
   * `promise.setTry()` because of the use of `std::variant`, `promise.setTry`
   * won't be able to convert the incoming response to the correct
   * `std::variant` automatically.
   */
  template <typename T>
  void setSemiFuture(folly::SemiFuture<T>&& future) {
    auto&& result = std::move(future).getTry();

    if (result.hasValue()) {
      promise_.setValue(ResponseType{std::forward<T>(result.value())});
    } else {
      promise_.setException(std::move(result.exception()));
    }
  }

 private:
  using ResponseType =
      std::variant<std::unique_ptr<Blob>, std::unique_ptr<Tree>>;

  HgImportRequest(
      RequestType type,
      Hash hash,
      ImportPriority priority,
      folly::Promise<ResponseType>&& promise)
      : type_(type),
        hash_(std::move(hash)),
        priority_(priority),
        promise_(std::move(promise)) {}

  RequestType type_;
  Hash hash_;
  ImportPriority priority_;
  folly::Promise<ResponseType> promise_;

  friend bool operator<(
      const HgImportRequest& lhs,
      const HgImportRequest& rhs) {
    return lhs.priority_ < rhs.priority_;
  }
};

} // namespace eden
} // namespace facebook
