/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <boost/property_tree/ini_parser.hpp>
#include <folly/Optional.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class InterpolatedPropertyTree {
 public:
  // Create a property tree with no interpolation replacements
  InterpolatedPropertyTree() = default;

  // Create a property tree using the supplied interpolation replacements.
  // The initializer list is something like: {{"USER", "foo"}}.
  // This will cause "${USER}" to be replaced by "foo" when the get method
  // is called.
  explicit InterpolatedPropertyTree(
      std::initializer_list<std::pair<folly::StringPiece, folly::StringPiece>>
          replacements);

  // Get a key from the tree.  If the key is not present, use defaultValue.
  // This string is then subject to interpolation using the configured
  // replacements on this InterpolatedPropertyTree.  That includes the
  // value supplied in defval.
  std::string get(
      folly::StringPiece section,
      folly::StringPiece key,
      folly::StringPiece defaultValue) const;

  // Set a value in the specified section
  void set(
      folly::StringPiece section,
      folly::StringPiece key,
      folly::StringPiece value);

  // Load a config file, replacing the contents of the internal property tree
  void loadIniFile(AbsolutePathPiece path);

  enum class MergeDisposition {
    // Don't load any data from this section; skip all keys
    SkipAll,
    // Create or replace each of the keys with the values from the
    // newly loaded section
    UpdateAll
  };

  // Load a config file and merge it into the current property tree.
  // The acceptSection function will be passed the name of a section
  // and should return a value indicating how we'd like to apply
  // the configuration from the newly loaded configuration file.
  void updateFromIniFile(
      AbsolutePathPiece path,
      std::function<MergeDisposition(
          const InterpolatedPropertyTree& tree,
          folly::StringPiece sectionName)> acceptSection =
          [](const InterpolatedPropertyTree&, folly::StringPiece) {
            return MergeDisposition::UpdateAll;
          });

  bool hasSection(folly::StringPiece section) const;

  // Returns a dictionary holding the keys and interpolated values
  // from the specified section
  folly::StringKeyedUnorderedMap<std::string> getSection(
      folly::StringPiece section) const;

 private:
  boost::property_tree::ptree tree_;
  std::map<std::string, std::string> replacements_;

  // Apply all replacements to the input string and return the
  // resultant string
  std::string interpolate(const std::string& input) const;
};
}
}
