/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <assert.h>
#include <stddef.h>
#include <atomic>

namespace facebook::eden {

/**
 * The intrusive part of `RefPtr`. Classes managed by RefPtr must publicly
 * derive from RefCounted. It's best if the RefCounted base class comes before
 * others so that no offset conversions are required on dereference.
 */
class RefCounted {
 protected:
  RefCounted() : refcnt_{1} {}

  RefCounted(const RefCounted&) = delete;
  RefCounted(RefCounted&&) = delete;

  RefCounted& operator=(const RefCounted&) = delete;
  RefCounted& operator=(RefCounted&&) = delete;

  virtual ~RefCounted() = default;

  bool isUnique() const noexcept {
    return 1 == refcnt_.load(std::memory_order_acquire);
  }

 private:
  void incRef() noexcept {
    refcnt_.fetch_add(1, std::memory_order_relaxed);
  }

  template <typename T>
  void decRef() noexcept {
    // Avoid the expensive atomic decrement if we're the last reference.
    if (1 == refcnt_.load(std::memory_order_acquire) ||
        1 == refcnt_.fetch_sub(1, std::memory_order_acq_rel)) {
      // The caller asserts that `this` is `T*`, so we cast before deleting to
      // avoid a virtual destructor call in the case that T is final.
      delete static_cast<T*>(this);
    }
  }

  template <typename T>
  friend class RefPtr;

  std::atomic<size_t> refcnt_;
};

/**
 * RefPtr stores its tagged pointer in a base class so that derived RefPtr<T>
 * and RefPtr<U> implementations can be converted between each other without
 * incrementing the reference count.
 *
 * For this to work, T* and U* have to have the same bit pattern, even if T* and
 * U* would have an offset from each other. Therefore, ptr_ stores a RefCounted*
 * and applies any offsets on dereference.
 */
struct RefPtrBase {
  RefPtrBase() noexcept = default;
  explicit RefPtrBase(uintptr_t ptr) noexcept : ptr_{ptr} {}

  // The pointer is encoded as a uintptr_t where 0 is nullptr. Otherwise, it's a
  // pointer, except the bottom bit is borrowed to indicate whether the object
  // is owned by this pointer.
  //
  // I'm not 100% sure, but this implementation may require a platform where
  // nullptr is represented with zero bits. Certainly kNull and the bit
  // representation of every valid pointer must be distinct.
  static_assert(alignof(RefCounted) >= 2);
  static constexpr uintptr_t kNull = 0;
  static constexpr uintptr_t kOwnedBit = 1;
  static constexpr uintptr_t kPtrMask = ~uintptr_t{} << 1;
  static_assert((kNull & kOwnedBit) == 0);

  uintptr_t ptr_ = kNull;
};

/**
 * Manages an intrusively-reference-counted object, whose reference count is
 * provided by deriving `RefCounted`.
 *
 * Generally, code should reach for `std::shared_ptr`, but `RefPtr` has some
 * advantages in performance-sensitive situations:
 *
 * 1. sizeof(RefPtr) == sizeof(void*)
 * 2. No copy constructor. All reference increments require explicit `copy()`.
 * 3. If the reference is never shared, no atomics are necessary.
 * 4. Supports unowned pointers of static lifetime.
 */
template <typename T>
class RefPtr : private RefPtrBase {
  static_assert(std::is_base_of_v<RefCounted, T>);

 public:
  RefPtr() noexcept = default;

  ~RefPtr() noexcept {
    decRef();
  }

  /**
   * Implicit copy is disabled. Use `copy()`.
   */
  RefPtr(const RefPtr&) = delete;

  RefPtr(RefPtr&& that) noexcept : RefPtrBase(that.ptr_) {
    that.ptr_ = kNull;
  }

  /**
   * Allows conversion of a RefPtr<D> to RefPtr<B> if D* is convertible to B*.
   */
  template <typename U>
  /* implicit */ RefPtr(RefPtr<U> that) noexcept
      : RefPtr{convert_ptr<U>(that.ptr_)} {
    that.ptr_ = kNull;
  }

  /**
   * Implicit copy is disabled. Use `copy()`.
   */
  RefPtr& operator=(const RefPtr&) = delete;

  /**
   * Self-move leaves the pointer in an empty state. This saves a branch on
   * every move.
   */
  RefPtr& operator=(RefPtr&& that) noexcept {
    decRef();
    ptr_ = that.ptr_;
    that.ptr_ = kNull;
    return *this;
  }

  /**
   * Returns a RefPtr that takes a reference to a new reference-counted object.
   * The reference count must be one.
   */
  static RefPtr takeOwnership(T* ptr) {
    RefCounted* base = ptr;
    assert(
        base->isUnique() &&
        "RefPtr::takeOwnership requires a newly-allocated object with a"
        "single reference");
    return RefPtr{base};
  }

  /**
   * Takes a reference of static duration and returns a RefPtr that will not
   * increment or decrement reference counts, and will never delete the object.
   * Intended for singletons that are guaranteed to outlive the pointer.
   */
  static RefPtr singleton(T& singleton) {
    return RefPtr{
        reinterpret_cast<uintptr_t>(static_cast<RefCounted*>(&singleton))};
  }

  /**
   * If you're using RefCounted and RefPtr, you probably care about performance.
   * Otherwise, you'd use shared_ptr. Therefore, prevent implicit copies and
   * require any additional atomic reference counts to require an explicit
   * copy().
   */
  RefPtr copy() const noexcept {
    incRef();
    return RefPtr{ptr_};
  }

  /**
   * If you have a `RefPtr<Derived>` and you want to pass it to a function
   * accepting a `const RefPtr<Base>&`, this function converts the RefPtr
   * without incrementing the reference count. The returned RefPtr is a const
   * reference because it cannot be used to assign into the parent pointer.
   *
   * CAREFUL: You must not assign or clear `this` while the returned `const
   * RefPtr<U>&` is alive. The two pointers are aliases of the same pointer
   * bits, so it's illegal to modify `this` while the return value may be used.
   */
  template <typename U>
  const RefPtr<U>& as() const noexcept {
    static_assert(
        std::is_base_of_v<U, T>, "as() can only convert to base classes");
    // TODO: Does this violate TBAA? Should we use std::launder?
    // The intent is that the encoded `RefCounted*` and tag bit are
    // the same for all pointers, but that we can static_cast to different
    // T* types on the way out.
    return *static_cast<const RefPtr<U>*>(static_cast<const RefPtrBase*>(this));
  }

  /**
   * Releases the reference, if any, and clears this pointer.
   */
  void reset() noexcept {
    decRef();
    ptr_ = kNull;
  }

  explicit operator bool() const noexcept {
    return ptr_ != kNull;
  }

  T* operator->() const noexcept {
    assert(ptr_ != kNull);
    return get();
  }

  T& operator*() const noexcept {
    assert(ptr_ != kNull);
    return *get();
  }

  T* get() const noexcept {
    return static_cast<T*>(reinterpret_cast<RefCounted*>(ptr_ & kPtrMask));
  }

 private:
  template <typename U>
  friend class RefPtr;

  // Takes an existing reference.
  explicit RefPtr(RefCounted* ptr) noexcept
      : RefPtrBase{reinterpret_cast<uintptr_t>(ptr) | kOwnedBit} {}

  explicit RefPtr(uintptr_t ptr) noexcept : RefPtrBase{ptr} {}

  template <typename U>
  static uintptr_t convert_ptr(uintptr_t that) {
    uint64_t owned = that & kOwnedBit;
    T* t = reinterpret_cast<U*>(that & kPtrMask);
    return reinterpret_cast<uintptr_t>(t) | owned;
  }

  void incRef() const noexcept {
    if (ptr_ & kOwnedBit) {
      get()->incRef();
    }
  }

  void decRef() const noexcept {
    if (ptr_ & kOwnedBit) {
      get()->template decRef<T>();
    }
  }
};

/**
 * Convenience function with a similar signature to std::make_unique and
 * std::make_shared.
 */
template <typename T, typename... Args>
RefPtr<T> makeRefPtr(Args&&... args) {
  return RefPtr<T>::takeOwnership(new T{std::forward<Args>(args)...});
}

} // namespace facebook::eden
