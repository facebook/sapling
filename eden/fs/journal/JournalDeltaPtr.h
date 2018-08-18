/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
