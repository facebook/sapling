/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "StreamingSubscriber.h"

#include <folly/logging/xlog.h>

using folly::StringPiece;

namespace facebook {
namespace eden {

StreamingSubscriber::State::State(StreamingSubscriber::Callback callback)
    : callback(std::move(callback)) {}

void StreamingSubscriber::onEventBaseDestruction() noexcept {
  auto state = state_.wlock();
  if (state->callback) {
    // We're called on the eventBase thread so we can call these
    // methods directly and tear down the peer.  Note that we
    // should only get here in the case that the server is being
    // shutdown.  The individual unmount case is handled by the
    // destructor.
    state->callback->done();
    state->callback.reset();
  }
  state->eventBaseAlive = false;
}

void StreamingSubscriber::subscribe(
    Callback callback,
    std::shared_ptr<EdenMount> edenMount) {
  auto self =
      std::make_shared<StreamingSubscriber>(std::move(callback), edenMount);

  // Separately scope the lock as the schedule() below will attempt to acquire
  // it for itself.
  {
    auto state = self->state_.wlock();

    // Arrange to be told when the eventBase is about to be destroyed
    state->callback->getEventBase()->runOnDestruction(*self);
    state->subscriberId =
        edenMount->getJournal().registerSubscriber([self] { schedule(self); });
  }

  // Suggest to the subscription that the journal has been updated so that
  // it will compute initial delta information.
  schedule(self);
}

StreamingSubscriber::StreamingSubscriber(
    Callback callback,
    std::shared_ptr<EdenMount> edenMount)
    : edenMount_(std::move(edenMount)),
      state_(folly::in_place, std::move(callback)) {}

StreamingSubscriber::~StreamingSubscriber() {
  // Cancel the EventBase::OnDestructionCallback
  cancel();

  auto state = state_.wlock();
  // If the eventBase is still live then we should tear down the peer
  if (state->callback) {
    CHECK(state->eventBaseAlive);
    auto evb = state->callback->getEventBase();

    // Move the callback away; we won't be able to use it
    // via state-> again.
    evb->runInEventBaseThread(
        [callback = std::move(state->callback)]() mutable {
          callback->done();
          callback.reset();
        });
  }
}

void StreamingSubscriber::schedule(std::shared_ptr<StreamingSubscriber> self) {
  auto state = self->state_.rlock();
  if (state->callback) {
    state->callback->getEventBase()->runInEventBaseThread(
        [self] { self->journalUpdated(); });
  }
}

void StreamingSubscriber::journalUpdated() {
  auto edenMount = edenMount_.lock();
  if (!edenMount) {
    XLOG(DBG1) << "Mount is released: subscription is no longer active";
    auto state = state_.wlock();
    state->callback->done();
    state->callback.reset();
    return;
  }

  auto state = state_.wlock();
  if (!state->callback) {
    // We were cancelled while this callback was queued up.
    // There's nothing for us to do now.
    return;
  }

  auto& journal = edenMount->getJournal();
  if (!state->callback->isRequestActive() ||
      !journal.isSubscriberValid(state->subscriberId)) {
    XLOG(DBG1) << "Subscription is no longer active";
    journal.cancelSubscriber(state->subscriberId);
    state->callback->done();
    state->callback.reset();
    return;
  }

  JournalPosition pos;

  auto delta = journal.getLatest();
  pos.sequenceNumber = delta->sequenceID;
  pos.snapshotHash = StringPiece(delta->toHash.getBytes()).str();
  pos.mountGeneration = edenMount->getMountGeneration();

  try {
    // And send it
    state->callback->write(pos);
  } catch (const std::exception& exc) {
    XLOG(ERR) << "Error while sending subscription update: " << exc.what();
  }
}
} // namespace eden
} // namespace facebook
