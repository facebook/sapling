/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/synchronization/Rcu.h>
#include <memory>

namespace facebook::eden {

/**
 * Smart pointer to automatically manage RCU resources.
 *
 * For details about RCU: https://en.wikipedia.org/wiki/Read-copy-update
 */
template <
    typename T,
    typename RcuTag = folly::RcuTag,
    typename Deleter = std::default_delete<T>>
class RcuPtr {
 public:
  using RcuDomain = folly::rcu_domain<RcuTag>;

  /**
   * Smart pointer that ensures the proper use of the rcu_reader guard.
   *
   * The managed resource is guaranteed to be valid as long as this object is
   * alive. It is expected that an RcuLockedPtr is short lived, as live
   * RcuLockedPtr would prevent the RCU domain to synchronize, potentially
   * leading to memory from other RcuPtr from being reclaimed.
   */
  class RcuLockedPtr {
   public:
    ~RcuLockedPtr() = default;

    RcuLockedPtr(const RcuLockedPtr&) = delete;
    RcuLockedPtr& operator=(const RcuLockedPtr&) = delete;

    RcuLockedPtr(RcuLockedPtr&& other) = default;
    RcuLockedPtr& operator=(RcuLockedPtr&&) = default;

    /**
     * Return a pointer to the inner resource.
     *
     * The lifetime of the returned value is the same as the RcuLockedPtr.
     */
    T* get() const noexcept {
      return inner_;
    }

    T& operator*() const noexcept {
      return *inner_;
    }

    T* operator->() const noexcept {
      return inner_;
    }

    explicit operator bool() const noexcept {
      return inner_;
    }

   private:
    friend RcuPtr<T, RcuTag, Deleter>;

    /**
     * Construct the smart pointer.
     *
     * The RCU section needs to be created first to ensure that the pointer
     * isn't dangling.
     */
    explicit RcuLockedPtr(RcuPtr& self)
        : guard_(&self.domain_),
          inner_(self.inner_.load(std::memory_order_acquire)) {}

    folly::rcu_reader_domain<RcuTag> guard_;
    T* inner_;
  };

  template <class... Args>
  explicit RcuPtr(RcuDomain& rcuDomain, Args&&... args)
      : domain_(rcuDomain), inner_(new T(std::forward<Args>(args)...)) {}

  explicit RcuPtr(RcuDomain& rcuDomain) : domain_(rcuDomain) {}

  explicit RcuPtr(RcuDomain& rcuDomain, std::unique_ptr<T, Deleter> ptr)
      : domain_(rcuDomain), inner_(ptr.release()) {}

  RcuPtr(const RcuPtr&) = delete;
  RcuPtr& operator=(const RcuPtr&) = delete;
  RcuPtr(RcuPtr&&) = delete;
  RcuPtr& operator=(RcuPtr&&) = delete;

  /**
   * Destroy this RcuPtr.
   *
   * The underlying resource will be asynchronously freed.
   */
  ~RcuPtr() {
    reset();
  }

  void reset() {
    update_inner(nullptr);
  }

  /**
   * Obtain a reference to the inner resource.
   */
  RcuLockedPtr rlock() noexcept {
    return RcuLockedPtr{*this};
  }

  /**
   * Build a new resource in place.
   *
   * Returns the old resource. As concurrent threads may be holding a
   * RcuLockedPtr with the returned pointer, care must be taken to not free it
   * until they all RcuLockedPtr are destroyed. The use of RcuPtr::synchronize
   * after this can be used to that effect.
   */
  template <class... Args>
  T* exchange(Args&&... args) noexcept {
    return exchange_inner(new T(std::forward<Args>(args)...));
  }

  /**
   * Swap the inner resource and release it.
   *
   * The resource is freed asynchronously.
   */
  template <class... Args>
  void update(Args&&... args) noexcept {
    update_inner(new T(std::forward<Args>(args)...));
  }

  void update(std::unique_ptr<T, Deleter> ptr) noexcept {
    update_inner(ptr.release());
  }

  /**
   * Blocks until no RcuLockedPtr are live.
   *
   * This only waits until all the live RcuLockedPtr at the time the function
   * is called are destroyed.
   */
  void synchronize() noexcept {
    domain_.synchronize();
  }

 private:
  friend class RcuPtr<T, RcuTag>::RcuLockedPtr;

  T* exchange_inner(T* inner) {
    return inner_.exchange(inner, std::memory_order_acq_rel);
  }

  void update_inner(T* inner) {
    auto old = exchange_inner(inner);
    if (old) {
      domain_.call([old] { Deleter()(old); });
    }
  }

  RcuDomain& domain_;
  std::atomic<T*> inner_{};
};

} // namespace facebook::eden
