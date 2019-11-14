/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "ConfigParser.h"
using namespace facebook::eden;

// The following functions are exported from this rust library:
// @dep=//eden/scm/lib/configparser:configparser

namespace {
struct BytesData {
  const uint8_t* ptr;
  size_t len;
};
} // namespace

extern "C" HgRcConfigSetStruct* hgrc_configset_new(void) noexcept;
extern "C" void hgrc_configset_free(HgRcConfigSetStruct* configset) noexcept;
extern "C" HgRcBytesStruct* hgrc_configset_load_path(
    HgRcConfigSetStruct* ptr,
    const char* path) noexcept;
extern "C" HgRcBytesStruct* hgrc_configset_load_system(
    HgRcConfigSetStruct* ptr) noexcept;
extern "C" HgRcBytesStruct* hgrc_configset_load_user(
    HgRcConfigSetStruct* ptr) noexcept;
extern "C" HgRcBytesStruct* hgrc_configset_get(
    HgRcConfigSetStruct* ptr,
    const uint8_t* section,
    size_t section_len,
    const uint8_t* name,
    size_t name_len) noexcept;

extern "C" void hgrc_bytes_free(HgRcBytesStruct* bytes) noexcept;
extern "C" BytesData hgrc_bytes_data(HgRcBytesStruct* bytes) noexcept;

namespace facebook {
namespace eden {

HgRcBytes::HgRcBytes(HgRcBytesStruct* ptr) : ptr_(ptr) {}

void HgRcBytes::Deleter::operator()(HgRcBytesStruct* ptr) const {
  hgrc_bytes_free(ptr);
}

folly::ByteRange HgRcBytes::bytes() const {
  auto data = hgrc_bytes_data(ptr_.get());
  return folly::ByteRange(data.ptr, data.len);
}

HgRcConfigSet::HgRcConfigSet() : ptr_(hgrc_configset_new()) {}

void HgRcConfigSet::Deleter::operator()(HgRcConfigSetStruct* ptr) const {
  hgrc_configset_free(ptr);
}

void HgRcConfigSet::loadPath(const char* path) {
  auto result = hgrc_configset_load_path(ptr_.get(), path);
  if (!result) {
    return;
  }
  HgRcBytes errorText(result);
  throw HgRcConfigError(errorText.stringPiece().str());
}

void HgRcConfigSet::loadSystem() {
  auto result = hgrc_configset_load_system(ptr_.get());
  if (!result) {
    return;
  }
  HgRcBytes errorText(result);
  throw HgRcConfigError(errorText.stringPiece().str());
}

void HgRcConfigSet::loadUser() {
  auto result = hgrc_configset_load_user(ptr_.get());
  if (!result) {
    return;
  }
  HgRcBytes errorText(result);
  throw HgRcConfigError(errorText.stringPiece().str());
}

folly::Optional<HgRcBytes> HgRcConfigSet::get(
    folly::ByteRange section,
    folly::ByteRange name) const noexcept {
  auto result = hgrc_configset_get(
      ptr_.get(), section.data(), section.size(), name.data(), name.size());
  if (result) {
    return HgRcBytes(result);
  }
  return folly::none;
}

} // namespace eden
} // namespace facebook
