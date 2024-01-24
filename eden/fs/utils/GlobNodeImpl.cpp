/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/GlobNodeImpl.h"
#include <iomanip>
#include <iostream>

using folly::StringPiece;
using std::string;
using std::unique_ptr;
using std::vector;

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

namespace {
struct Indentation {
  int width;

  friend std::ostream& operator<<(
      std::ostream& s,
      const Indentation& indentation) {
    return s << std::setw(indentation.width) << "";
  }
};
} // namespace

void GlobNodeImpl::debugDump(int currentDepth) const {
  auto& out = std::cerr;
  auto indentation = Indentation{currentDepth * 2};
  auto boolString = [](bool b) -> const char* { return b ? "true" : "false"; };

  out << indentation << "- GlobNodeImpl " << this << "\n"
      << indentation << "  alwaysMatch=" << boolString(alwaysMatch_) << "\n"
      << indentation << "  hasSpecials=" << boolString(hasSpecials_) << "\n"
      << indentation << "  includeDotfiles=" << boolString(includeDotfiles_)
      << "\n"
      << indentation << "  isLeaf=" << boolString(isLeaf_) << "\n";

  if (pattern_.empty()) {
    out << indentation << "  pattern is empty\n";
  } else {
    out << indentation << "  pattern: " << pattern_ << "\n";
  }

  if (!children_.empty()) {
    out << indentation << "  children (" << children_.size() << "):\n";
    for (const auto& child : children_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }

  if (!recursiveChildren_.empty()) {
    out << indentation << "  recursiveChildren (" << recursiveChildren_.size()
        << "):\n";
    for (const auto& child : recursiveChildren_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }
}

} // namespace facebook::eden
