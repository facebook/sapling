/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/mononoke/MononokeAPIUtils.h"

#include <folly/io/IOBuf.h>
#include <folly/json.h>

namespace facebook {
namespace eden {
std::unique_ptr<Tree> parseMononokeTree(
    std::unique_ptr<folly::IOBuf>&& buf,
    const Hash& id) {
  auto s = buf->moveToFbString();
  auto parsed = folly::parseJson(s);
  if (!parsed.isArray()) {
    throw std::runtime_error(
        "malformed json response from mononoke: should be array");
  }

  std::vector<TreeEntry> entries;
  entries.reserve(parsed.size());
  for (auto i = parsed.begin(); i != parsed.end(); ++i) {
    auto name = i->at("name").asString();
    auto hash = Hash(i->at("hash").asString());
    auto str_type = i->at("type").asString();
    TreeEntryType file_type;
    if (str_type == "file") {
      file_type = TreeEntryType::REGULAR_FILE;
    } else if (str_type == "tree") {
      file_type = TreeEntryType::TREE;
    } else if (str_type == "executable") {
      file_type = TreeEntryType::EXECUTABLE_FILE;
    } else if (str_type == "symlink") {
      file_type = TreeEntryType::SYMLINK;
    } else {
      throw std::runtime_error(folly::to<std::string>(
          "unknown file type from mononoke: ", str_type));
    }

    auto contentSha1 = i->get_ptr("content_sha1");
    auto size = i->get_ptr("size");
    if (contentSha1 && !contentSha1->isNull() && size && !size->isNull()) {
      entries.emplace_back(
          hash,
          name,
          file_type,
          static_cast<uint64_t>(size->asInt()),
          Hash(contentSha1->asString()));
    } else {
      entries.emplace_back(hash, name, file_type);
    }
  }
  return std::make_unique<Tree>(std::move(entries), id);
}

} // namespace eden
} // namespace facebook
