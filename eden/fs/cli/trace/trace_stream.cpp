/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <fmt/core.h>
#include <folly/init/Init.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/io/async/ScopedEventBaseThread.h>
#include <thrift/lib/cpp2/async/RocketClientChannel.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;

DEFINE_string(mountRoot, "", "Root of the EdenFS mount");
DEFINE_string(trace, "", "Trace mode");

namespace {
std::string formatTime(uint64_t ns) {
  // Convert to microseconds before converting to double in case we have a
  // duration longer than 3 months.
  double d = double(ns / 1000);
  return fmt::format("{:.3f} ms", d / 1000.0);
}

int trace_hg(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  StreamingEdenServiceAsyncClient client{std::move(channel)};

  apache::thrift::ClientBufferedStream<HgEvent> traceHgStream =
      client.semifuture_traceHgEvents(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .get();

  /**
   * Like `eden strace`, it would be nice to print the active set of requests
   * before streaming the events.
   */
  struct ActiveRequest {
    std::optional<HgEvent> queue;
    std::optional<HgEvent> start;
  };

  std::unordered_map<uint64_t, ActiveRequest> activeRequests;

  static const std::unordered_map<HgEventType, const char*> kEventTypes = {
      {HgEventType::QUEUE, " "},
      {HgEventType::START, u8"\u21E3"},
      {HgEventType::FINISH, u8"\u2193"},
  };

  static const std::unordered_map<HgResourceType, const char*> kResourceTypes =
      {
          {HgResourceType::BLOB, u8"\U0001F954"},
          {HgResourceType::TREE, u8"\U0001F332"},
      };

  std::move(traceHgStream).subscribeInline([&](folly::Try<HgEvent>&& event) {
    if (event.hasException()) {
      fmt::print("Error: {}\n", folly::exceptionStr(event.exception()));
      return;
    }

    HgEvent& evt = event.value();

    std::optional<HgEvent> queueEvent;
    std::optional<HgEvent> startEvent;

    const HgEventType eventType = *evt.eventType_ref();
    const HgResourceType resourceType = *evt.resourceType_ref();
    const uint64_t unique = *evt.unique_ref();

    switch (eventType) {
      case HgEventType::UNKNOWN:
        break;
      case HgEventType::QUEUE:
        activeRequests[unique].queue = evt;
        break;
      case HgEventType::START: {
        auto& record = activeRequests[unique];
        queueEvent = record.queue;
        record.start = evt;
        break;
      }
      case HgEventType::FINISH: {
        auto& record = activeRequests[unique];
        startEvent = record.start;
        activeRequests.erase(unique);
        break;
      }
    }

    std::string timeAnnotation;
    switch (eventType) {
      case HgEventType::UNKNOWN:
        break;
      case HgEventType::QUEUE:
        // TODO: Might be interesting to add an option to see queuing events.
        return;
      case HgEventType::START:
        if (queueEvent) {
          auto queueTime = evt.times_ref()->monotonic_time_ns_ref().value() -
              queueEvent->times_ref()->monotonic_time_ns_ref().value();
          // Don't bother printing queue time under 1 ms.
          if (queueTime >= 1000000) {
            timeAnnotation =
                fmt::format(" queued for {}", formatTime(queueTime));
          }
        } else {
          // This event was queued before we subscribed.
        }
        break;

      case HgEventType::FINISH:
        if (startEvent) {
          auto fetchTime = evt.times_ref()->monotonic_time_ns_ref().value() -
              startEvent->times_ref()->monotonic_time_ns_ref().value();
          timeAnnotation = fmt::format(" fetched in {}", formatTime(fetchTime));
        }
        break;
    }

    const char* eventTypeStr = folly::get_default(kEventTypes, eventType, "?");
    const char* resourceTypeStr =
        folly::get_default(kResourceTypes, resourceType, "?");
    fmt::print(
        "{} {} {}{}\n",
        eventTypeStr,
        resourceTypeStr,
        *evt.path_ref(),
        timeAnnotation);
  });

  fmt::print("{} was unmounted\n", FLAGS_mountRoot);
  return 0;
}
} // namespace

int main(int argc, char** argv) {
  // Don't buffer stdout, even if piped to a file.
  setbuf(stdout, nullptr);

  folly::init(&argc, &argv);

  folly::ScopedEventBaseThread evbThread;

  // TODO: Implement Windows client logic.
  AbsolutePath mountRoot{FLAGS_mountRoot};
  AbsolutePath socketPath = mountRoot + ".eden"_pc + "socket"_pc;

  auto channel = folly::via(
                     evbThread.getEventBase(),
                     [&]() -> apache::thrift::RocketClientChannel::Ptr {
                       auto address = folly::SocketAddress::makeFromPath(
                           socketPath.stringPiece());
                       return apache::thrift::RocketClientChannel::newChannel(
                           folly::AsyncSocket::newSocket(
                               evbThread.getEventBase(), address));
                     })
                     .get();

  if (FLAGS_trace == "hg") {
    return trace_hg(evbThread, mountRoot, std::move(channel));
  } else if (FLAGS_trace.empty()) {
    fmt::print(stderr, "Must specify trace mode\n");
    return 1;
  } else {
    fmt::print(stderr, "Unknown trace mode: {}\n", FLAGS_trace);
    return 1;
  }
}
