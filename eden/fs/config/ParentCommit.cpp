/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ParentCommit.h"

namespace facebook::eden {

bool ParentCommit::isCheckoutInProgress() const {
  return std::holds_alternative<ParentCommit::CheckoutInProgress>(state_);
}

std::optional<RootId> ParentCommit::getCurrentRootId(
    RootIdPreference preference) const {
  return std::visit(
      [preference](auto&& state) -> std::optional<RootId> {
        using StateType = std::decay_t<decltype(state)>;
        if constexpr (std::is_same_v<StateType, RootId>) {
          return state;
        } else {
          switch (preference) {
            case ParentCommit::RootIdPreference::To:
              return state.to;
            case ParentCommit::RootIdPreference::From:
              return state.from;
            case ParentCommit::RootIdPreference::OnlyStable:
              return std::nullopt;
          }
        }
      },
      state_);
}

bool operator==(
    const ParentCommit::CheckoutInProgress& left,
    const ParentCommit::CheckoutInProgress& right) {
  return left.from == right.from && left.to == right.to;
}

bool ParentCommit::operator==(const ParentCommit& other) const {
  return state_ == other.state_;
}

} // namespace facebook::eden
