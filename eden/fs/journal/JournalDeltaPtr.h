/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <memory>

namespace facebook {
namespace eden {

class JournalDelta;

/**
 * Analogous to a std::shared_ptr<const JournalDelta> but has a synchronizing
 * unique() method. Used in the destructor for O(1) destruction.
 */
class JournalDeltaPtr {
 public:
  JournalDeltaPtr(std::nullptr_t = nullptr) {}
  explicit JournalDeltaPtr(std::unique_ptr<JournalDelta> ptr);
  JournalDeltaPtr(const JournalDeltaPtr&) noexcept;
  JournalDeltaPtr(JournalDeltaPtr&&) noexcept;
  ~JournalDeltaPtr();

  JournalDeltaPtr& operator=(const JournalDeltaPtr&) noexcept;
  JournalDeltaPtr& operator=(JournalDeltaPtr&&) noexcept;

  explicit operator bool() const {
    return ptr_ != nullptr;
  }

  const JournalDelta* get() const {
    return ptr_;
  }

  bool operator!() const {
    return !ptr_;
  }

  const JournalDelta& operator*() const {
    return *ptr_;
  }

  const JournalDelta* operator->() const {
    return ptr_;
  }

  void swap(JournalDeltaPtr& p) noexcept;

  /**
   * Returns true if the reference count (loaded with memory_order_acquire) is
   * 1, meaning this pointer is the only owner of the underlying object.
   */
  bool unique() const;

 private:
  const JournalDelta* ptr_ = nullptr;
};

inline bool operator==(const JournalDeltaPtr& p, const JournalDeltaPtr& q) {
  return p.get() == q.get();
}

inline bool operator!=(const JournalDeltaPtr& p, const JournalDeltaPtr& q) {
  return p.get() != q.get();
}

} // namespace eden
} // namespace facebook
