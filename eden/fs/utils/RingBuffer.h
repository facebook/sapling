/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <stddef.h>
#include <atomic>
#include <stdexcept>
#include <vector>

namespace facebook::eden {

/**
 * Non-synchronized ring buffer with a fixed capacity.
 */
template <typename T>
class RingBuffer {
 public:
  /**
   * Constructs a RingBuffer with a given capacity. Zero is legal.
   */
  explicit RingBuffer(size_t capacity);

  /**
   * Returns the capacity.
   */
  size_t capacity() const {
    return capacity_;
  }

  /**
   * Returns the number of entries pushed minus the number of entries evicted.
   *
   * size() <= capacity().
   */
  size_t size() const;

  /**
   * Pushes an entry into the RingBuffer. This replaces the oldest existing
   * entry if capacity has been reached.
   */
  template <typename U>
  void push(U&& entry);

  /**
   * Returns the contents of this RingBuffer in order from oldest to newest.
   */
  std::vector<T> toVector() const;

  /**
   * Extracts the contents of the ring buffer. The order of the elements is
   * not guaranteed, and the ring buffer should not be used after this call.
   */
  std::vector<T> extractVector() &&;

 private:
  size_t capacity_;
  std::vector<T> entries_;
  // Indicates whether the entries_ vector has been extracted.
  std::atomic<bool> extracted_ = false;
  size_t write_ = 0;
};

template <typename T>
RingBuffer<T>::RingBuffer(size_t capacity) : capacity_{capacity} {
  // vector does not require that reserve sets capacity precisely.
  // Overshooting the desired capacity may be undesirable if T has its own
  // externally-allocated memory.
  entries_.reserve(capacity);
}

template <typename T>
size_t RingBuffer<T>::size() const {
  auto extracted = extracted_.load(std::memory_order_acquire);
  if (extracted) {
    throw std::runtime_error("RingBuffer has been extracted");
  }
  return entries_.size();
}

template <typename T>
template <typename U>
void RingBuffer<T>::push(U&& entry) {
  static_assert(std::is_constructible_v<T, U&&>);
  auto extracted = extracted_.load(std::memory_order_acquire);
  if (extracted) {
    throw std::runtime_error("RingBuffer has been extracted");
  }

  if (capacity_ == 0) {
    return;
  }
  if (entries_.size() < capacity_) {
    entries_.push_back(std::forward<U>(entry));
  } else {
    entries_[write_] = std::forward<U>(entry);
    ++write_;
    if (write_ == capacity_) {
      write_ = 0;
    }
  }
}

template <typename T>
std::vector<T> RingBuffer<T>::toVector() const {
  auto extracted = extracted_.load(std::memory_order_acquire);
  if (extracted) {
    throw std::runtime_error("RingBuffer has been extracted");
  }
  if (write_ == 0) {
    return entries_;
  } else {
    std::vector<T> entries;
    entries.reserve(capacity_);
    for (size_t i = write_; i < capacity_; ++i) {
      entries.push_back(entries_[i]);
    }
    for (size_t i = 0; i < write_; ++i) {
      entries.push_back(entries_[i]);
    }

    return entries;
  }
}
template <typename T>
std::vector<T> RingBuffer<T>::extractVector() && {
  extracted_.store(true, std::memory_order_release);
  return std::move(entries_);
}

} // namespace facebook::eden
