/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <folly/CppAttributes.h>
#include <folly/Try.h>
#include <folly/io/IOBuf.h>
#include <folly/json/json.h>
#include <folly/logging/xlog.h>
#include <algorithm>
#include <memory>
#include <string>
#include <unordered_set>
#include <vector>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/scm/lib/backingstore/include/ffi.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h" // @manual

namespace sapling {

namespace {

std::string toStdString(const folly::dynamic& value) {
  const auto& string = value.asString();
  return std::string{string.data(), string.size()};
}

void logMalformedSampleField(
    const char* section,
    const folly::dynamic& key,
    const char* expectedType,
    const folly::dynamic& value) {
  XLOGF(
      WARN,
      "Skipping malformed EdenSample field '{}.{}': expected {}, got {}",
      section,
      key.asString(),
      expectedType,
      value.typeName());
}

bool isStringArray(const folly::dynamic& value) {
  return value.isArray() &&
      std::all_of(value.begin(), value.end(), [](const auto& item) {
           return item.isString();
         });
}

const folly::dynamic* FOLLY_NULLABLE
getObjectField(const folly::dynamic& sample, const char* name) {
  const auto* field = sample.get_ptr(name);
  if (!field) {
    return nullptr;
  }
  if (!field->isObject()) {
    XLOGF(
        WARN,
        "Skipping malformed EdenSample section '{}': expected object, got {}",
        name,
        field->typeName());
    return nullptr;
  }
  return field;
}

} // namespace

void sapling_backingstore_log_edenfs_event(
    const facebook::eden::EdenFsEventsLogger& logger,
    rust::Str sampleJson) {
  const auto sample =
      folly::parseJson(std::string_view{sampleJson.data(), sampleJson.size()});
  if (!sample.isObject()) {
    throw std::invalid_argument{"EdenSample JSON is not an object"};
  }

  facebook::eden::DynamicEvent event;

  if (const auto* ints = getObjectField(sample, "int")) {
    // EdenSample serializes this section from a BTreeMap<String, i64>. Other
    // numeric representations cannot be converted to int64_t without losing
    // precision, so treat them as malformed input.
    for (const auto& [key, value] : ints->items()) {
      if (!value.isInt()) {
        logMalformedSampleField("int", key, "integer", value);
        continue;
      }
      event.addInt(toStdString(key), value.asInt());
    }
  }
  if (const auto* strings = getObjectField(sample, "normal")) {
    for (const auto& [key, value] : strings->items()) {
      if (!value.isString()) {
        logMalformedSampleField("normal", key, "string", value);
        continue;
      }
      event.addString(toStdString(key), toStdString(value));
    }
  }
  if (const auto* vectors = getObjectField(sample, "normvector")) {
    for (const auto& [key, value] : vectors->items()) {
      if (!isStringArray(value)) {
        logMalformedSampleField(
            "normvector", key, "array containing only strings", value);
        continue;
      }
      std::vector<std::string> values;
      values.reserve(value.size());
      for (const auto& item : value) {
        values.emplace_back(toStdString(item));
      }
      event.addStringVec(toStdString(key), std::move(values));
    }
  }
  if (const auto* sets = getObjectField(sample, "tags")) {
    for (const auto& [key, value] : sets->items()) {
      if (!isStringArray(value)) {
        logMalformedSampleField(
            "tags", key, "array containing only strings", value);
        continue;
      }
      std::unordered_set<std::string> values;
      values.reserve(value.size());
      for (const auto& item : value) {
        values.emplace(toStdString(item));
      }
      event.addStringSet(toStdString(key), std::move(values));
    }
  }

  logger.logEvent(event);
}

void sapling_backingstore_get_tree_batch_handler(
    std::shared_ptr<GetTreeBatchResolver> resolver,
    size_t index,
    std::unique_ptr<SaplingBackingStoreError> error,
    std::unique_ptr<TreeBuilder> builder) {
  using ResolveResult = folly::Try<facebook::eden::TreePtr>;

  resolver->resolve(
      index, folly::makeTryWith([&] {
        if (error == nullptr) {
          facebook::eden::TreePtr tree = builder->build();
          if (tree) {
            return ResolveResult{tree};
          } else {
            return ResolveResult{SaplingBackingStoreError{"no tree found"}};
          }
        } else {
          return ResolveResult{std::move(*error)};
        }
      }));
}

void sapling_backingstore_get_tree_aux_batch_handler(
    std::shared_ptr<GetTreeAuxBatchResolver> resolver,
    size_t index,
    std::unique_ptr<SaplingBackingStoreError> error,
    std::shared_ptr<TreeAuxData> aux) {
  using ResolveResult = folly::Try<std::shared_ptr<TreeAuxData>>;

  resolver->resolve(index, folly::makeTryWith([&] {
                      if (error == nullptr) {
                        return ResolveResult{aux};
                      } else {
                        return ResolveResult{std::move(*error)};
                      }
                    }));
}

void sapling_backingstore_get_blob_batch_handler(
    std::shared_ptr<GetBlobBatchResolver> resolver,
    size_t index,
    std::unique_ptr<SaplingBackingStoreError> error,
    std::unique_ptr<folly::IOBuf> blob) {
  using ResolveResult = folly::Try<std::unique_ptr<folly::IOBuf>>;

  resolver->resolve(
      index,
      folly::makeTryWith(
          [blob = std::move(blob), error = std::move(error)]() mutable {
            if (error == nullptr) {
              return ResolveResult{std::move(blob)};
            } else {
              return ResolveResult{std::move(*error)};
            }
          }));
}

void sapling_backingstore_get_file_aux_batch_handler(
    std::shared_ptr<GetFileAuxBatchResolver> resolver,
    size_t index,
    std::unique_ptr<SaplingBackingStoreError> error,
    std::shared_ptr<FileAuxData> aux) {
  using ResolveResult = folly::Try<std::shared_ptr<FileAuxData>>;

  resolver->resolve(index, folly::makeTryWith([&] {
                      if (error == nullptr) {
                        return ResolveResult{aux};
                      } else {
                        return ResolveResult{std::move(*error)};
                      }
                    }));
}

void TreeBuilder::add_entry(
    rust::Str name,
    const std::array<uint8_t, 20>& hg_node,
    facebook::eden::TreeEntryType ttype,
    bool is_restricted,
    bool has_acl) {
  emplace_entry(
      name,
      facebook::eden::TreeEntry{
          make_entry_oid(hg_node, name),
          ttype,
          facebook::eden::makeAclRootState(is_restricted, has_acl),
      });
}

void TreeBuilder::add_entry_with_aux_data(
    rust::Str name,
    const std::array<uint8_t, 20>& hg_node,
    facebook::eden::TreeEntryType ttype,
    const uint64_t size,
    const std::array<uint8_t, 20>& sha1,
    const std::array<uint8_t, 32>& blake3,
    bool is_restricted,
    bool has_acl) {
  emplace_entry(
      name,
      facebook::eden::TreeEntry{
          make_entry_oid(hg_node, name),
          ttype,
          size,
          std::optional<facebook::eden::Hash20>(sha1),
          std::optional<facebook::eden::Hash32>(blake3),
          facebook::eden::makeAclRootState(is_restricted, has_acl),
      });
}

void TreeBuilder::emplace_entry(
    rust::Str name,
    facebook::eden::TreeEntry&& entry) {
  auto nameView = std::string_view{name.data(), name.length()};

  if (entry.isTree()) {
    numDirs_++;
  } else {
    numFiles_++;
  }

  // We skip the path sanity check below, but let's check in debug builds, just
  // in case.
  XDCHECK_EQ(facebook::eden::RelativePathPiece{nameView}.view(), nameView);

  entries_.emplace_back(
      // This name comes from Sapling's PathComponent type, which is already
      // validated.
      facebook::eden::PathComponentPiece{
          nameView, facebook::eden::detail::SkipPathSanityCheck{}},
      std::move(entry));
}

facebook::eden::ObjectId TreeBuilder::make_entry_oid(
    const std::array<uint8_t, 20>& hg_node,
    rust::Str name) {
  auto nameView = std::string_view{name.data(), name.length()};

  // We skip the path sanity check below, but let's check in debug builds, just
  // in case.
  XDCHECK_EQ(facebook::eden::RelativePathPiece{nameView}.view(), nameView);

  return facebook::eden::SlOid{
      reinterpret_cast<const facebook::eden::Hash20&>(hg_node),
      oid_.path(),
      // This name comes from Sapling's PathComponent type, which is already
      // validated.
      facebook::eden::PathComponentPiece{
          nameView, facebook::eden::detail::SkipPathSanityCheck{}}}
      .oid();
}

void TreeBuilder::set_aux_data(
    const std::array<uint8_t, 32>& digest,
    uint64_t size) {
  auxData_ = std::make_shared<facebook::eden::TreeAuxDataPtr::element_type>(
      facebook::eden::Hash32{digest}, size);
}

facebook::eden::TreePtr TreeBuilder::build() {
  if (missing_) {
    return nullptr;
  }
  return std::make_shared<facebook::eden::TreePtr::element_type>(
      std::move(oid_).oid(),
      facebook::eden::Tree::container{std::move(entries_), caseSensitive_},
      std::move(auxData_));
}

std::unique_ptr<TreeBuilder> new_builder(
    bool caseSensitive,
    facebook::eden::HgObjectIdFormat oidFormat,
    const rust::Slice<const uint8_t> oid) {
  return std::make_unique<TreeBuilder>(TreeBuilder{
      facebook::eden::SaplingObjectId{
          folly::StringPiece{
              reinterpret_cast<const char*>(oid.data()), oid.size()},
          // Skip validation - this data has already been validated.
          false},
      caseSensitive ? facebook::eden::CaseSensitivity::Sensitive
                    : facebook::eden::CaseSensitivity::Insensitive,
      oidFormat,
  });
}

} // namespace sapling
