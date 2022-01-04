/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <string>
#include <utility>

namespace folly {
class IOBuf;
}

namespace facebook::eden {

class KeySpace;

/*
 * StoreResult contains the result of a LocalStore lookup.
 *
 * RocksDB returns the data in a std::string (which is somewhat unfortunate,
 * since it gives us relatively poor memory management control).
 *
 * This class is a wrapper around the returned string, with a few benefits:
 * - It can also represent a "not found" result, so we can efficiently handle
 *   key lookups that are not present, without throwing an exception.
 * - It is move-only, so prevents us from ever unintentionally copying the
 *   string data.
 * - It provides APIs for creating IOBuf objects around the string result.
 */
class StoreResult {
 public:
  /**
   * Construct an invalid StoreResult containing the reason why it's invalid.
   */
  static StoreResult missing(KeySpace keySpace, folly::ByteRange key);

  /**
   * Construct a StoreResult from payload data.
   */
  explicit StoreResult(std::string data) : StoreResult{true, std::move(data)} {}

  StoreResult(StoreResult&& that) noexcept
      : valid_{false}, data_{"moved-from"} {
    std::swap(valid_, that.valid_);
    std::swap(data_, that.data_);
  }

  StoreResult& operator=(StoreResult&& that) noexcept {
    std::string data{"moved-from"};
    // Allocate the new std::string before performing the no-except swaps.
    valid_ = std::exchange(that.valid_, false);
    data_ = std::exchange(that.data_, std::move(data));
    return *this;
  }

  /**
   * Returns true if the value was found in the store,
   * or false if the key was not present.
   */
  bool isValid() const {
    return valid_;
  }

  /**
   * Get a reference to the std::string result.
   *
   * Throws std::domain_error if the key was not present in the store.
   */
  const std::string& asString() const {
    ensureValid();
    return data_;
  }

  /**
   * Get a ByteRange pointing to the result.
   *
   * Throws std::domain_error if the key was not present in the store.
   */
  folly::ByteRange bytes() const {
    ensureValid();
    return folly::StringPiece{data_};
  }

  /**
   * Get a StringPiece pointing to the result.
   *
   * Throws std::domain_error if the key was not present in the store.
   */
  folly::StringPiece piece() const {
    ensureValid();
    return folly::StringPiece{data_};
  }

  /**
   * Return an IOBuf that temporarily wraps this StoreResult.
   *
   * The IOBuf is unmanaged, and points to the string data contained in this
   * StoreResult.  It will be invalidated by any operation that invalidates the
   * StoreResult.
   */
  folly::IOBuf iobufWrapper() const;

  /**
   * Extract the std::string contained in this StoreResult.
   */
  std::string extractValue() {
    ensureValid();
    valid_ = false;
    return std::move(data_);
  }

  /**
   * Extract the data as an IOBuf.
   *
   * This will return a managed IOBuf, which will free the result data when
   * the last IOBuf clone is destroyed.
   *
   * This does require a memory allocation to move the stored std::string onto
   * the heap (but it just does a small allocation for the string object
   * itself, and not the string data).
   */
  folly::IOBuf extractIOBuf();

  /**
   * Throw an exception if this result is not valid
   * (i.e., if the key was not present in the store).
   */
  void ensureValid() const {
    if (!valid_) {
      throwInvalidError();
    }
  }

 private:
  StoreResult(bool valid, std::string data)
      : valid_{valid}, data_{std::move(data)} {}

  // Forbidden copy constructor and assignment operator
  StoreResult(StoreResult const&) = delete;
  StoreResult& operator=(StoreResult const&) = delete;

  [[noreturn]] void throwInvalidError() const;

  /**
   * If true, data_ contains the payload from the store.
   * If false, it contains an error message that includes context about what was
   * looked up.
   */
  bool valid_{false};
  std::string data_;
};

} // namespace facebook::eden
