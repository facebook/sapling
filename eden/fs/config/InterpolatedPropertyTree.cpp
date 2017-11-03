/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "InterpolatedPropertyTree.h"
namespace facebook {
namespace eden {

using defaultPtree =
    boost::property_tree::basic_ptree<std::string, std::string>;

InterpolatedPropertyTree::InterpolatedPropertyTree(
    std::initializer_list<std::pair<folly::StringPiece, folly::StringPiece>>
        replacements) {
  for (auto& it : replacements) {
    replacements_.emplace(std::make_pair(
        folly::to<std::string>("${", it.first, "}"), it.second.toString()));
  }
}

namespace {

// Replace all occurrences of "search" in "subject" with "replace".
// Has forward progress guarantees in case "replace" contains "search".
void replaceAll(
    std::string& subject,
    const std::string& search,
    const std::string& replace) {
  size_t pos = 0;
  while ((pos = subject.find(search, pos)) != std::string::npos) {
    subject.replace(pos, search.size(), replace);
    pos += replace.size();
  }
}
} // namespace

std::string InterpolatedPropertyTree::interpolate(
    const std::string& input) const {
  std::string result(input);
  for (auto& it : replacements_) {
    replaceAll(result, it.first, it.second);
  }
  return result;
}

std::string InterpolatedPropertyTree::get(
    folly::StringPiece section,
    folly::StringPiece key,
    folly::StringPiece defaultValue) const {
  auto child = tree_.get_child(section.toString(), defaultPtree());
  return interpolate(child.get(key.toString(), defaultValue.toString()));
}

void InterpolatedPropertyTree::set(
    folly::StringPiece section,
    folly::StringPiece key,
    folly::StringPiece value) {
  tree_.put(folly::to<std::string>(section, ".", key), value.toString());
}

void InterpolatedPropertyTree::loadIniFile(AbsolutePathPiece path) {
  boost::property_tree::ini_parser::read_ini(
      path.stringPiece().toString(), tree_);
}

bool InterpolatedPropertyTree::hasSection(folly::StringPiece section) const {
  return !tree_.get_child(section.toString(), defaultPtree()).empty();
}

folly::StringKeyedUnorderedMap<std::string>
InterpolatedPropertyTree::getSection(folly::StringPiece section) const {
  folly::StringKeyedUnorderedMap<std::string> result;

  auto child = tree_.get_child(section.toString(), defaultPtree());

  for (auto& it : child) {
    result.emplace(it.first, interpolate(it.second.data()));
  }

  return result;
}

void InterpolatedPropertyTree::updateFromIniFile(
    AbsolutePathPiece path,
    std::function<MergeDisposition(
        const InterpolatedPropertyTree& tree,
        folly::StringPiece sectionName)> acceptSection) {
  boost::property_tree::ptree loaded;
  boost::property_tree::ini_parser::read_ini(
      path.stringPiece().toString(), loaded);

  for (auto& entry : loaded) {
    auto disp = acceptSection(*this, entry.first);
    switch (disp) {
      case MergeDisposition::SkipAll:
        break;
      case MergeDisposition::UpdateAll:
        for (auto& item : entry.second) {
          tree_.put_child(
              folly::to<std::string>(entry.first, ".", item.first),
              item.second);
        }
        break;
    }
  }
}
} // namespace eden
} // namespace facebook
