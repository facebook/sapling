/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/async/EventBase.h>

namespace facebook::eden {

/**
 * EventBase state machines need to ensure their state is only accessed from the
 * EventBase. EventBaseState provides that guarantee: the state can only be
 * accessed from the given EventBase.
 */
template <typename State>
class EventBaseState {
 public:
  /**
   * Constructs an EventBaseState tied to the specified EventBase.
   */
  template <typename... T>
  explicit EventBaseState(folly::EventBase* evb, T&&... args)
      : evb_{evb}, state_{std::forward<T>(args)...} {}

  EventBaseState(const EventBaseState&) = delete;
  EventBaseState(EventBaseState&&) = delete;
  EventBaseState& operator=(const EventBaseState&) = delete;
  EventBaseState& operator=(EventBaseState&&) = delete;

  State& get() {
    evb_->checkIsInEventBaseThread();
    return state_;
  }

  const State& get() const {
    evb_->checkIsInEventBaseThread();
    return state_;
  }

 private:
  folly::EventBase* evb_;
  State state_;
};

} // namespace facebook::eden
