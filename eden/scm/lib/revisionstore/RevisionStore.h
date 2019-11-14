// Copyright 2004-present Facebook. All Rights Reserved
#pragma once
/** This module makes available to C++ some of the Rust revisionstore
 * functionality.
 */
#include <folly/Optional.h>
#include <folly/Range.h>
#include <cstddef>
#include <cstdint>

namespace facebook {
namespace eden {

struct DataPackUnionStruct;
struct RevisionStoreStringStruct;
struct RevisionStoreByteVecStruct;

/** Represents a Rust String value returned from the revisionstore crate.
 * The string bytes are guaranteed to be value UTF-8.
 * The string value is used to represent a human readable error string.
 */
class RevisionStoreString {
 public:
  explicit RevisionStoreString(RevisionStoreStringStruct* ptr);

  // Explicitly reference the string bytes as a StringPiece
  folly::StringPiece stringPiece() const noexcept;

  operator folly::StringPiece() const noexcept {
    return stringPiece();
  }

 private:
  struct Deleter {
    void operator()(RevisionStoreStringStruct*) const noexcept;
  };
  std::unique_ptr<RevisionStoreStringStruct, Deleter> ptr_;
};

/** Represents a Rust Vec<u8> value returned from the revisionstore crate.
 */
class RevisionStoreByteVec {
 public:
  explicit RevisionStoreByteVec(RevisionStoreByteVecStruct* ptr);

  // Explicitly reference the bytes as a ByteRange
  folly::ByteRange bytes() const noexcept;

  // Implicit conversion to ByteRange
  operator folly::ByteRange() const noexcept {
    return bytes();
  }

 private:
  struct Deleter {
    void operator()(RevisionStoreByteVecStruct*) const noexcept;
  };

  std::unique_ptr<RevisionStoreByteVecStruct, Deleter> ptr_;
};

class DataPackUnionGetError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

/** DataPackUnion is configured with a list of directory paths that
 * contain some number of datapack files.
 * DataPackUnion can be queried to see if it contains a given key,
 * and fetch the corresponding de-delta'd value
 */
class DataPackUnion {
 public:
  DataPackUnion(const char* const paths[], size_t num_paths);

  // Look up the name/hgid pair.  If found, de-delta and return the data as a
  // RevisionStoreByteVec.  If not found, return folly::none.  If an error
  // occurs, throw a DataPackUnionGetError exception. This method is not thread
  // safe.
  folly::Optional<RevisionStoreByteVec> get(
      folly::ByteRange name,
      folly::ByteRange hgid);

 private:
  struct Deleter {
    void operator()(DataPackUnionStruct*) const noexcept;
  };
  std::unique_ptr<DataPackUnionStruct, Deleter> store_;
};

} // namespace eden
} // namespace facebook
