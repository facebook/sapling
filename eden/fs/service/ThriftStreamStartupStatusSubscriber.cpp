/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftStreamStartupStatusSubscriber.h"

#include "folly/logging/xlog.h"

namespace facebook::eden {

ThriftStreamStartupStatusSubscriber::ThriftStreamStartupStatusSubscriber(
    apache::thrift::ServerStreamPublisher<std::string> publisher,
    folly::CancellationToken cancellationToken)
    : cancellationToken_{std::move(cancellationToken)},
      publisher_(std::move(publisher)) {}

ThriftStreamStartupStatusSubscriber::
    ~ThriftStreamStartupStatusSubscriber() noexcept {
  // Destroying a publisher without calling complete() aborts the process, so
  // ensure complete() is called if it hasn't yet been called.
  if (!cancellationToken_.isCancellationRequested()) {
    auto ew = folly::try_and_catch([&] {
      // Thrift complete can throw. Let's log the error and move on with things.
      std::move(publisher_).complete();
    });
    if (ew) {
      XLOG(ERR) << "Completing a thrift ServerStreamPublisher failed: " << ew;
    }
  }
}

void ThriftStreamStartupStatusSubscriber::publish(std::string_view data) {
  if (!cancellationToken_.isCancellationRequested()) {
    publisher_.next(std::string{data});
  }
}

apache::thrift::ServerStream<std::string>
ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
    std::shared_ptr<StartupStatusChannel>& startupStatusChannel) {
  // this value will be shared between the complete/cancle callback
  // and the ThriftStreamStartupStatusSubscriber. This value being set renders
  // the ThriftStreamStartupStatusSubscriber operations into no-ops.
  folly::CancellationSource cancellationSource{};
  auto [serverStream, publisher] =
      apache::thrift::ServerStream<std::string>::createPublisher(
          [cancellationSource] {
            // This is called on cancel or complete. This prevents publishing
            // any more events to the stream.
            // - complete is called when startup finishes. We should stop
            // calling any ThriftStreamStartupStatusSubscriber methods
            // anyways, but it's fine to make them no-ops. events.
            // - cancel is when the client closes the stream. In this case, we
            // need to render the ThriftStreamStartupStatusSubscriber methods
            // no-ops because startup will continue and the code will keep
            // trying to publish.
            //
            // It looks so innocent, but think long and hard before changing
            // this. When you are thinking long and hard, make sure you
            // consider these two points:
            //
            // Note the first thing we are not doing here: deleteing the
            // apache::thrift::ServerStreamPublisher. In the complete case,
            // startup has finished, so the caller will cleanup the memory for
            // the publisher momentarily. In the cancel case, the memory is
            // not going to be imminently cleaned up. However, when startup
            // finishes the memory will be cleaned up. Trying to remove
            // ourselves from the StartupStatusSubscriberState here is really
            // tricky (need an extra lock, intrusive list, and very careful
            // lock placement to avoid deadlocks). IMO its not worth it to try
            // to clean up the memory rn because startup is short lived.
            //
            // Note the related but second thing we are not doing right now:
            // holding locks. This lambda is called inline with complete and
            // cancel. We hold the StartupStatusSubscriberState lock during
            // complete, so as it stands, it is not safe to try to reaqure the
            // StartupStatusSubscriberState lock here.
            // Its also dangerous to take a lock here that is held during
            // createPublisher because you would be relying on createPublisher
            // not calling cancel. I'm pretty sure createPublisher doesn't cal
            // cancel right now, but its a bad idea to rely on the internals
            // of an external library.
            cancellationSource.requestCancellation();
          });
  auto startupStatusSubscriber =
      std::make_unique<ThriftStreamStartupStatusSubscriber>(
          std::move(publisher), cancellationSource.getToken());

  startupStatusChannel->subscribe(std::move(startupStatusSubscriber));

  return std::move(serverStream);
}
} // namespace facebook::eden
