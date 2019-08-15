/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "RevisionStore.h"
using namespace facebook::eden;

// The following functions are exported from this rust library:
// @dep=//scm/hg/lib/revisionstore:revisionstore

namespace {
struct ByteData {
  const uint8_t* ptr;
  size_t len;
};

struct GetData {
  RevisionStoreByteVecStruct* value;
  RevisionStoreStringStruct* error;
  bool is_key_error;
};
} // namespace

extern "C" DataPackUnionStruct* revisionstore_datapackunion_new(
    const char* const paths[],
    size_t num_paths) noexcept;
extern "C" void revisionstore_datapackunion_free(
    DataPackUnionStruct* store) noexcept;
extern "C" GetData revisionstore_datapackunion_get(
    DataPackUnionStruct* store,
    const uint8_t* name,
    size_t name_len,
    const uint8_t* node,
    size_t node_len) noexcept;

extern "C" void revisionstore_string_free(
    RevisionStoreStringStruct* str) noexcept;
extern "C" ByteData revisionstore_string_data(
    RevisionStoreStringStruct* str) noexcept;

extern "C" void revisionstore_bytevec_free(
    RevisionStoreByteVecStruct* bytes) noexcept;
extern "C" ByteData revisionstore_bytevec_data(
    RevisionStoreByteVecStruct* bytes) noexcept;

namespace facebook {
namespace eden {

void DataPackUnion::Deleter::operator()(DataPackUnionStruct* ptr) const
    noexcept {
  revisionstore_datapackunion_free(ptr);
}

DataPackUnion::DataPackUnion(const char* const paths[], size_t num_paths)
    : store_(revisionstore_datapackunion_new(paths, num_paths)) {}

folly::Optional<RevisionStoreByteVec> DataPackUnion::get(
    folly::ByteRange name,
    folly::ByteRange node) {
  // This implementation is strongly coupled to that of
  // revisionstore_datapackunion_get in scm/hg/lib/revisionstore/src/c_api.rs
  auto got = revisionstore_datapackunion_get(
      store_.get(), name.data(), name.size(), node.data(), node.size());
  if (got.value) {
    return RevisionStoreByteVec(got.value);
  }
  if (got.is_key_error) {
    return folly::none;
  }
  RevisionStoreString error(got.error);
  throw DataPackUnionGetError(error.stringPiece().str());
}

RevisionStoreString::RevisionStoreString(RevisionStoreStringStruct* ptr)
    : ptr_(ptr) {}

void RevisionStoreString::Deleter::operator()(
    RevisionStoreStringStruct* ptr) const noexcept {
  revisionstore_string_free(ptr);
}

folly::StringPiece RevisionStoreString::stringPiece() const noexcept {
  auto data = revisionstore_string_data(ptr_.get());
  return folly::StringPiece(reinterpret_cast<const char*>(data.ptr), data.len);
}

RevisionStoreByteVec::RevisionStoreByteVec(RevisionStoreByteVecStruct* ptr)
    : ptr_(ptr) {}

void RevisionStoreByteVec::Deleter::operator()(
    RevisionStoreByteVecStruct* ptr) const noexcept {
  revisionstore_bytevec_free(ptr);
}

folly::ByteRange RevisionStoreByteVec::bytes() const noexcept {
  auto data = revisionstore_bytevec_data(ptr_.get());
  return folly::ByteRange(data.ptr, data.len);
}

} // namespace eden
} // namespace facebook
