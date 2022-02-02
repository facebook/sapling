/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <variant>
#include "eden/fs/model/RootId.h"

namespace facebook::eden {

/**
 * In memory representation of the state of the SNAPSHOT file.
 */
class ParentCommit {
 public:
  struct CheckoutInProgress;

  /* implicit */ ParentCommit(RootId root) : state_{std::move(root)} {}

  /* implicit */ ParentCommit(ParentCommit::CheckoutInProgress inProgress)
      : state_{std::move(inProgress)} {}

  /**
   * Returns true if a checkout is currently ongoing.
   */
  bool isCheckoutInProgress() const;

  /**
   * Since the parent commit might contain multiple RootId, allows chosing
   * which one should be preferred.
   *
   * In all the cases, when no checkout are ongoing, the current stable RootId
   * will always be used.
   */
  enum class RootIdPreference {
    /** During an update, prefer the destination RootId */
    To,
    /** During an update, prefer the originating RootId */
    From,
    /** During an update, no RootId are used */
    OnlyStable,
  };

  /**
   * Return the current RootId.
   *
   * See the documentation of RootIdPreference for the behavior during
   * checkout.
   */
  std::optional<RootId> getCurrentRootId(RootIdPreference preference) const;

  struct CheckoutInProgress {
    RootId from;
    RootId to;
  };

  bool operator==(const ParentCommit& other) const;

 private:
  std::variant<RootId, CheckoutInProgress> state_;
};

} // namespace facebook::eden
