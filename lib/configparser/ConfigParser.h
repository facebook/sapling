// Copyright 2004-present Facebook. All Rights Reserved
#pragma once
#include <folly/Optional.h>
#include <folly/Range.h>

/** This module makes available to C++ some of the Rust ConfigSet API */
namespace facebook {
namespace eden {

struct HgRcBytesStruct;
struct HgRcConfigSetStruct;

/** Encapsulates a rust Bytes object returned from the configparser library.
 * HgRcBytes can be converted to a folly::ByteRange */
class HgRcBytes {
 public:
  explicit HgRcBytes(HgRcBytesStruct* ptr);

  // Explicitly reference the data as a ByteRange
  folly::ByteRange bytes() const;

  // Explicitly reference the data as a StringPiece
  folly::StringPiece stringPiece() const {
    return folly::StringPiece{bytes()};
  }

  operator folly::ByteRange() const {
    return bytes();
  }

  operator folly::StringPiece() const {
    return stringPiece();
  }

 private:
  struct Deleter {
    void operator()(HgRcBytesStruct*) const;
  };
  std::unique_ptr<HgRcBytesStruct, Deleter> ptr_;
};

class HgRcConfigError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

/** Encapsulates a ConfigSet instance from the configparser library.
 * It is initially empty but can have multiple configuration files
 * loaed into it via loadPath().
 */
class HgRcConfigSet {
 public:
  HgRcConfigSet();

  // Attempt to load configuration from path.
  // Throws HgRcConfigError if there were error(s)
  void loadPath(const char* path);

  // Attempt to load the system configuration files
  // Throws HgRcConfigError if there were error(s)
  void loadSystem();

  // Attempt to load the user's configuration files
  // Throws HgRcConfigError if there were error(s)
  void loadUser();

  // Return the configuration value for the specified section/name
  folly::Optional<HgRcBytes> get(
      folly::ByteRange section,
      folly::ByteRange name) const noexcept;

  // Return the configuration value for the specified section/name
  folly::Optional<HgRcBytes> get(
      folly::StringPiece section,
      folly::StringPiece name) const noexcept {
    return get(folly::ByteRange{section}, folly::ByteRange{name});
  }

 private:
  struct Deleter {
    void operator()(HgRcConfigSetStruct*) const;
  };
  std::unique_ptr<HgRcConfigSetStruct, Deleter> ptr_;
};

} // namespace eden
} // namespace facebook
