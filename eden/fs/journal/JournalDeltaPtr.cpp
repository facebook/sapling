/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "JournalDeltaPtr.h"
#include "JournalDelta.h"

#include <glog/logging.h>

namespace facebook {
namespace eden {

JournalDeltaPtr::JournalDeltaPtr(std::unique_ptr<JournalDelta> ptr) {
  if (ptr) {
    ptr_ = ptr.release();
    DCHECK_EQ(0, ptr_->refCount_.load(std::memory_order_acquire));
    ptr_->refCount_.store(1, std::memory_order_release);
  }
}

JournalDeltaPtr::JournalDeltaPtr(const JournalDeltaPtr& other) noexcept
    : ptr_{other.ptr_} {
  if (ptr_) {
    ptr_->incRef();
  }
}

JournalDeltaPtr::JournalDeltaPtr(JournalDeltaPtr&& other) noexcept
    : ptr_{other.ptr_} {
  other.ptr_ = nullptr;
}

JournalDeltaPtr::~JournalDeltaPtr() {
  if (ptr_) {
    ptr_->decRef();
  }
}

JournalDeltaPtr& JournalDeltaPtr::operator=(
    const JournalDeltaPtr& other) noexcept {
  if (ptr_ == other.ptr_) {
    return *this;
  }

  if (ptr_) {
    auto* p = ptr_;
    ptr_ = nullptr;
    p->decRef();
  }
  ptr_ = other.ptr_;
  if (ptr_) {
    ptr_->incRef();
  }
  return *this;
}

JournalDeltaPtr& JournalDeltaPtr::operator=(JournalDeltaPtr&& other) noexcept {
  JournalDeltaPtr(std::move(other)).swap(*this);
  return *this;
}

void JournalDeltaPtr::swap(JournalDeltaPtr& p) noexcept {
  std::swap(ptr_, p.ptr_);
}

bool JournalDeltaPtr::unique() const {
  DCHECK(ptr_);
  return ptr_->isUnique();
}

} // namespace eden
} // namespace facebook
