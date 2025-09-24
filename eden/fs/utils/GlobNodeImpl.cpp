/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/GlobNodeImpl.h"
#include <iostream>

using folly::StringPiece;
using std::string;
using std::unique_ptr;
using std::vector;

namespace facebook::eden::detail {
struct Indentation {
  int width;
};
} // namespace facebook::eden::detail

template <>
struct fmt::formatter<facebook::eden::detail::Indentation> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(
      const facebook::eden::detail::Indentation& indentation,
      FormatContext& ctx) const {
    return std::fill_n(ctx.out(), indentation.width, ' ');
  }
};

namespace facebook::eden {

GlobNodeImpl::GlobNodeImpl(
    StringPiece pattern,
    bool includeDotfiles,
    bool hasSpecials,
    CaseSensitivity caseSensitive)
    : pattern_(pattern.str()),
      includeDotfiles_(includeDotfiles),
      hasSpecials_(hasSpecials) {
  if (includeDotfiles && (pattern == "**" || pattern == "*")) {
    alwaysMatch_ = true;
  } else {
    auto options =
        includeDotfiles ? GlobOptions::DEFAULT : GlobOptions::IGNORE_DOTFILES;
    if (caseSensitive == CaseSensitivity::Insensitive) {
      options |= GlobOptions::CASE_INSENSITIVE;
    }
    auto compiled = GlobMatcher::create(pattern, options);
    if (compiled.hasError()) {
      throw std::system_error(
          EINVAL,
          std::generic_category(),
          fmt::format(
              "failed to compile pattern `{}` to GlobMatcher: {}",
              pattern,
              compiled.error()));
    }
    matcher_ = std::move(compiled.value());
  }
}

void GlobNodeImpl::parse(StringPiece pattern) {
  GlobNodeImpl* parent = this;
  string normalizedPattern;

  while (!pattern.empty()) {
    StringPiece token;
    auto* container = &parent->children_;
    bool hasSpecials;

    if (pattern.startsWith("**")) {
      // Recursive match defeats most optimizations; we have to stop
      // tokenizing here.

      // HACK: We special-case "**" if includeDotfiles=false. In this case, we
      // need to create a GlobMatcher for this pattern, but GlobMatcher is
      // designed to reject "**". As a workaround, we use "**/*", which is
      // functionally equivalent in this case because there are no other
      // "tokens" in the pattern following the "**" at this point.
      if (pattern == "**" && !includeDotfiles_) {
        normalizedPattern = "**/*";
        token = normalizedPattern;
      } else {
        token = pattern;
      }

      pattern = StringPiece();
      container = &parent->recursiveChildren_;
      hasSpecials = true;
    } else {
      token = tokenize(pattern, &hasSpecials);
      // Exit early for illegal glob node syntax.
      (void)PathComponentPiece{token};
    }

    auto node = lookupToken(container, token);
    if (!node) {
      container->emplace_back(std::make_unique<GlobNodeImpl>(
          token, includeDotfiles_, hasSpecials, caseSensitive_));
      node = container->back().get();
    }

    // If there are no more tokens remaining then we have a leaf node
    // that will emit results.  Update the node to reflect this.
    // Note that this may convert a pre-existing node from an earlier
    // glob specification to a leaf node.
    if (pattern.empty()) {
      node->isLeaf_ = true;
    }

    // Continue parsing the remainder of the pattern using this
    // (possibly new) node as the parent.
    parent = node;
  }
}

StringPiece GlobNodeImpl::tokenize(StringPiece& pattern, bool* hasSpecials) {
  *hasSpecials = false;

  for (auto it = pattern.begin(); it != pattern.end(); ++it) {
    switch (*it) {
      case '*':
      case '?':
      case '[':
      case '\\':
        *hasSpecials = true;
        break;
      case '/':
        // token is the input up-to-but-not-including the current position,
        // which is a '/' character
        StringPiece token(pattern.begin(), it);
        // update the pattern to be the text after the slash
        pattern = StringPiece(it + 1, pattern.end());
        return token;
    }
  }

  // No slash found, so the the rest of the pattern is the token
  StringPiece token = pattern;
  pattern = StringPiece();
  return token;
}

GlobNodeImpl* FOLLY_NULLABLE GlobNodeImpl::lookupToken(
    vector<unique_ptr<GlobNodeImpl>>* container,
    StringPiece token) {
  for (auto& child : *container) {
    if (child->pattern_ == token) {
      return child.get();
    }
  }
  return nullptr;
}

void GlobNodeImpl::debugDump() const {
  debugDump(/*currentDepth=*/0);
}

void GlobNodeImpl::debugDump(int currentDepth) const {
  auto& out = std::cerr;
  auto indentation = detail::Indentation{currentDepth * 2};
  auto boolString = [](bool b) -> const char* { return b ? "true" : "false"; };

  out << fmt::format(
             "{}- GlobNodeImpl {:p}\n",
             indentation,
             static_cast<const void*>(this))
      << fmt::format(
             "{}  alwaysMatch={}\n", indentation, boolString(alwaysMatch_))
      << fmt::format(
             "{}  hasSpecials={}\n", indentation, boolString(hasSpecials_))
      << fmt::format(
             "{}  includeDotfiles={}\n",
             indentation,
             boolString(includeDotfiles_))
      << fmt::format("{}  isLeaf={}\n", indentation, boolString(isLeaf_));

  if (pattern_.empty()) {
    out << fmt::format("{}  pattern is empty\n", indentation);
  } else {
    out << fmt::format("{}  pattern: {}\n", indentation, pattern_);
  }

  if (!children_.empty()) {
    out << fmt::format("{}  children ({}):\n", indentation, children_.size());
    for (const auto& child : children_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }

  if (!recursiveChildren_.empty()) {
    out << fmt::format(
        "{}  recursiveChildren ({}):\n",
        indentation,
        recursiveChildren_.size());
    for (const auto& child : recursiveChildren_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }
}
} // namespace facebook::eden
