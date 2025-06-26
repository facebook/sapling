/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/fibers/Semaphore.h>
#include <cstdlib>
#include <memory>

namespace facebook::eden {

/**
 * RAII guard for acquiring and releasing a request permit.
 *
 * This class automatically acquires a permit when constructed and releases it
 * when destroyed, ensuring that every wait() operation is matched with a
 * signal() operation.
 */
class RequestPermit {
 public:
  explicit RequestPermit(std::weak_ptr<folly::fibers::Semaphore> sem)
      : sem_(std::move(sem)) {
    if (auto semPtr = sem_.lock()) {
      semPtr->wait();
    }
  }

  ~RequestPermit() {
    if (auto semPtr = sem_.lock()) {
      semPtr->signal();
    }
    sem_.reset();
  }

  // Disallow default construction, coping, and moving to avoid accidental
  // permit creation/deletion. This class should be managed by a smart pointer.
  // If RequestPermitVendor is extended to support a try_acquirePermit() or
  // a co_acquirePermit() method, this class will likely need to be extended to
  // offer alternative construction methods.
  RequestPermit() = delete;
  RequestPermit(const RequestPermit&) = delete;
  RequestPermit& operator=(const RequestPermit&) = delete;
  RequestPermit(RequestPermit&&) = delete;
  RequestPermit& operator=(RequestPermit&&) = delete;

 private:
  std::weak_ptr<folly::fibers::Semaphore> sem_;

  friend class RequestPermitVendor;
};

/**
 * RequestPermitVendor generates RequestPermits which represent a resource
 * acquired from a semaphore. RequestPermits release the resource when
 * destructed. RequestPermitVendor has sole ownership over the underlying
 * semaphore. This can be added to any class that wishes to implement rate
 * limiting
 *
 * This class currently only offers a blocking acquire method, but it can be
 * extended in the future to add a try_acquirePermit() method which can return
 * immediately if the semaphore is out of capacity. It can also be extended to
 * support a co_acquirePermit() method, see folly::fibers::Semaphore::co_wait()
 * for more information.
 */
class RequestPermitVendor {
 public:
  explicit RequestPermitVendor(std::size_t limit)
      : sem_(std::make_shared<folly::fibers::Semaphore>(limit)) {}

  /**
   * This will block until a permit is available.
   */
  inline std::unique_ptr<RequestPermit> acquirePermit() {
    return std::make_unique<RequestPermit>(sem_);
  }

  /**
   * Get the configured max capacity of the underlying semaphore
   */
  inline std::size_t capacity() const {
    return sem_->getCapacity();
  }

  /**
   * Get the current available headroom of the underlying semaphore
   */
  inline std::size_t available() const {
    return sem_->getAvailableTokens();
  }

  /**
   * Get the current number of inflight requests
   */
  inline std::size_t inflight() const {
    return sem_->getCapacity() - sem_->getAvailableTokens();
  }

 private:
  std::shared_ptr<folly::fibers::Semaphore> sem_;
};

} // namespace facebook::eden
