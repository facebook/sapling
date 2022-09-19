/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cpptoml.h>
#include <fmt/core.h>
#include <folly/Portability.h>
#include <folly/init/Init.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/io/async/ScopedEventBaseThread.h>
#include <folly/lang/ToAscii.h>
#include <thrift/lib/cpp/util/EnumUtils.h>
#include <thrift/lib/cpp2/async/RocketClientChannel.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/service/gen-cpp2/StreamingEdenServiceAsyncClient.h"
#include "eden/fs/service/gen-cpp2/streamingeden_constants.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/TimeUtil.h"

using namespace facebook::eden;
using namespace std::string_view_literals;

DEFINE_string(mountRoot, "", "Root of the EdenFS mount");
DEFINE_string(trace, "", "Trace mode");
DEFINE_bool(writes, false, "Limit trace to write operations");
DEFINE_bool(reads, false, "Limit trace to write operations");
DEFINE_bool(verbose, false, "Show import priority and cause");
DEFINE_bool(
    retroactive,
    false,
    "Provide stored inode events (from a buffer) across past changes");

namespace {
constexpr auto kTimeout = std::chrono::seconds{1};
constexpr size_t kStartingInodeWidth = 5;
static const auto kTreeEmoji = reinterpret_cast<const char*>(u8"\U0001F332");
static const auto kBlobEmoji = reinterpret_cast<const char*>(u8"\U0001F954");
static const auto kDashedArrowEmoji = reinterpret_cast<const char*>(u8"\u21E3");
static const auto kSolidArrowEmoji = reinterpret_cast<const char*>(u8"\u2193");
static const auto kWarningSignEmoji = reinterpret_cast<const char*>(u8"\u26A0");
static const auto kRedSquareEmoji =
    reinterpret_cast<const char*>(u8"\U0001F7E5");
static const auto kOrangeDiamondEmoji =
    reinterpret_cast<const char*>(u8"\U0001F536");
static const auto kGreenCircleEmoji =
    reinterpret_cast<const char*>(u8"\U0001F7E2");
static const auto kQuestionEmoji = reinterpret_cast<const char*>(u8"\u2753");
static const auto kFolderEmoji = reinterpret_cast<const char*>(u8"\U0001F4C1");
static const auto kFaxMachineEmoji =
    reinterpret_cast<const char*>(u8"\U0001F4E0");
static const auto kCalendarEmoji =
    reinterpret_cast<const char*>(u8"\U0001F4C5");

static const std::unordered_map<HgEventType, const char*> kHgEventTypes = {
    {HgEventType::QUEUE, " "},
    {HgEventType::START, kDashedArrowEmoji},
    {HgEventType::FINISH, kSolidArrowEmoji},
};

static const std::unordered_map<InodeEventType, const char*> kInodeEventTypes =
    {
        {InodeEventType::UNKNOWN, "?"},
        {InodeEventType::MATERIALIZE, "M"},
        {InodeEventType::LOAD, "L"},
};

static const std::unordered_map<InodeEventProgress, const char*>
    kInodeProgresses = {
        {InodeEventProgress::START, kDashedArrowEmoji},
        {InodeEventProgress::END, kSolidArrowEmoji},
        {InodeEventProgress::FAIL, kWarningSignEmoji},
};

static const std::unordered_map<HgResourceType, const char*> kResourceTypes = {
    {HgResourceType::BLOB, kBlobEmoji},
    {HgResourceType::TREE, kTreeEmoji},
};

static const std::unordered_map<HgImportPriority, const char*>
    kImportPriorities = {
        {HgImportPriority::LOW, kRedSquareEmoji},
        {HgImportPriority::NORMAL, kOrangeDiamondEmoji},
        {HgImportPriority::HIGH, kGreenCircleEmoji},
};

static const std::unordered_map<HgImportCause, const char*> kImportCauses = {
    {HgImportCause::UNKNOWN, kQuestionEmoji},
    {HgImportCause::FS, kFolderEmoji},
    {HgImportCause::THRIFT, kFaxMachineEmoji},
    {HgImportCause::PREFETCH, kCalendarEmoji},
};

/**
 * Represents an active HgRequest that has been queued and possibly started but
 * not finished. Note queue events must be optional since tracing could begin
 * between queue and start events and also becaue queue events are not present
 * in the ActivityBuffer for retroactive tracing. Note: Like `eden strace`, it
 * would be nice to print the active set of requests (that are currently in
 * progress when streaming starts) before streaming the events in trace_hg.
 */
struct ActiveHgRequest {
  std::optional<HgEvent> queue;
  std::optional<HgEvent> start;
};

std::string formatFuseOpcode(const FuseCall& call) {
  std::string name = *call.opcodeName();
  auto mutableName = folly::MutableStringPiece(name.data(), name.size());
  (void)mutableName.removePrefix("FUSE_");
  folly::toLowerAscii(mutableName);
  return mutableName.str();
}

std::string formatFuseCall(
    const FuseCall& call,
    const std::string& arguments = "",
    const std::string& result = "") {
  auto* processNamePtr = apache::thrift::get_pointer(call.processName());
  std::string processNameString = processNamePtr
      ? fmt::format("{}({})", processNamePtr->c_str(), *call.pid())
      : std::to_string(*call.pid());

  std::string argString = arguments.empty()
      ? fmt::format("{}", *call.nodeid())
      : fmt::format("{}, {}", *call.nodeid(), arguments);
  std::string resultString =
      result.empty() ? result : fmt::format(" = {}", result);

  return fmt::format(
      "{} from {}: {}({}){}",
      *call.unique(),
      processNameString,
      formatFuseOpcode(call),
      argString,
      resultString);
}

std::string formatNfsCall(
    const NfsCall& call,
    const std::string& arguments = std::string{}) {
  return fmt::format(
      "{}: {}({}) {}",
      static_cast<uint32_t>(*call.xid()),
      *call.procName(),
      *call.procNumber(),
      arguments);
}

std::string formatPrjfsCall(
    const PrjfsCall& call,
    std::string arguments = std::string{}) {
  if (arguments.empty()) {
    return fmt::format(
        "{} from {}: {}",
        *call.commandId(),
        *call.pid(),
        apache::thrift::util::enumName(*call.callType(), "(unknown)"));
  } else {
    return arguments;
  }
}

void print_hg_event(
    const HgEvent& evt,
    std::unordered_map<uint64_t, ActiveHgRequest>& activeRequests) {
  std::optional<HgEvent> queueEvent;
  std::optional<HgEvent> startEvent;

  const HgEventType eventType = *evt.eventType();
  const HgResourceType resourceType = *evt.resourceType();
  const HgImportPriority importPriority = *evt.importPriority();
  const HgImportCause importCause = *evt.importCause();
  const uint64_t unique = *evt.unique();

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
        auto queueTime = evt.times()->monotonic_time_ns().value() -
            queueEvent->times()->monotonic_time_ns().value();
        // Don't bother printing queue time under 1 ms.
        if (queueTime >= 1000000) {
          timeAnnotation =
              fmt::format(" queued for {}", formatNsTimeToMs(queueTime));
        }
      } else {
        // This event was queued before we subscribed.
      }
      break;

    case HgEventType::FINISH:
      if (startEvent) {
        auto fetchTime = evt.times()->monotonic_time_ns().value() -
            startEvent->times()->monotonic_time_ns().value();
        timeAnnotation =
            fmt::format(" fetched in {}", formatNsTimeToMs(fetchTime));
      }
      break;
  }

  const char* eventTypeStr = folly::get_default(kHgEventTypes, eventType, "?");
  const char* resourceTypeStr =
      folly::get_default(kResourceTypes, resourceType, "?");
  const char* importPriorityStr =
      folly::get_default(kImportPriorities, importPriority, "?");
  const char* importCauseStr =
      folly::get_default(kImportCauses, importCause, "?");

  if (FLAGS_verbose) {
    fmt::print(
        "{} {} {} {} {}{}\n",
        eventTypeStr,
        resourceTypeStr,
        importPriorityStr,
        importCauseStr,
        *evt.path(),
        timeAnnotation);
  } else {
    fmt::print(
        "{} {} {}{}\n",
        eventTypeStr,
        resourceTypeStr,
        *evt.path(),
        timeAnnotation);
  }
}

int trace_hg(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  apache::thrift::Client<StreamingEdenService> client{std::move(channel)};

  apache::thrift::ClientBufferedStream<HgEvent> traceHgStream =
      client.semifuture_traceHgEvents(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .get();

  /**
   * Like `eden strace`, would be nice to print the active set of requests (that
   * are currently in progress when streaming starts) before streaming the
   * events in trace_hg.
   */
  std::unordered_map<uint64_t, ActiveHgRequest> activeRequests;

  std::move(traceHgStream).subscribeInline([&](folly::Try<HgEvent>&& event) {
    if (event.hasException()) {
      fmt::print("Error: {}\n", folly::exceptionStr(event.exception()));
      return;
    }
    print_hg_event(event.value(), activeRequests);
  });

  fmt::print("{} was unmounted\n", FLAGS_mountRoot);
  return 0;
}

int trace_hg_retroactive(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  auto client =
      std::make_unique<apache::thrift::Client<EdenService>>(std::move(channel));

  GetRetroactiveHgEventsParams params{};
  params.mountPoint() = mountRoot.stringPiece();
  auto future = client->semifuture_getRetroactiveHgEvents(params).via(
      evbThread.getEventBase());

  std::move(future)
      .thenValue([](GetRetroactiveHgEventsResult allEvents) {
        auto events = *allEvents.events();
        std::sort(
            events.begin(), events.end(), [](const auto& a, const auto& b) {
              return a.times()->timestamp() < b.times()->timestamp();
            });

        std::unordered_map<uint64_t, ActiveHgRequest> activeRequests;
        fmt::print("Last {} hg events\n", events.size());
        for (auto& event : events) {
          print_hg_event(event, activeRequests);
        }
      })
      .thenError([](const folly::exception_wrapper& ex) {
        fmt::print("{}\n", ex.what());
        if (ex.get_exception<EdenError>()->errorCode() == ENOTSUP) {
          fmt::print(
              "Can't run retroactive command in eden mount without an initialized ActivityBuffer. Make sure the enable-activitybuffer config is true to save events retroactively.\n");
        }
      })
      .ensure(
          // Move the client into the callback so that it will be destroyed
          // on an EventBase thread.
          [c = std::move(client)] {})
      .get();
  return 0;
}

int trace_fs(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel,
    bool reads,
    bool writes) {
  int64_t mask = 0;
  if (reads) {
    mask |= streamingeden_constants::FS_EVENT_READ_;
  }
  if (writes) {
    mask |= streamingeden_constants::FS_EVENT_WRITE_;
  }

  apache::thrift::Client<StreamingEdenService> client{std::move(channel)};
  apache::thrift::ClientBufferedStream<FsEvent> traceFsStream =
      client.semifuture_traceFsEvents(mountRoot.stringPiece().str(), mask)
          .via(evbThread.getEventBase())
          .get();

  // TODO (liuz): Rather than issuing one call per filesystem interface, it
  // would be better to introduce a new thrift method that returns a list of
  // live filesystem calls, with an optional FuseCall, optional NfsCall,
  // optional PrjfsCall, just like streamingeden's FsEvent.
  std::vector<folly::SemiFuture<folly::Unit>> outstandingCallFutures;
#ifndef _WIN32
  outstandingCallFutures.emplace_back(
      client.semifuture_debugOutstandingFuseCalls(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .thenValue([](std::vector<FuseCall> outstandingCalls) {
            if (outstandingCalls.empty()) {
              return;
            }
            std::string_view header = "Outstanding FUSE calls"sv;
            fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
            for (const auto& call : outstandingCalls) {
              fmt::print("+ {}\n", formatFuseCall(call));
            }
            fmt::print("{}\n", std::string(header.size(), '-'));
          }));

  outstandingCallFutures.emplace_back(
      client.semifuture_debugOutstandingNfsCalls(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .thenValue([](std::vector<NfsCall> outstandingCalls) {
            if (outstandingCalls.empty()) {
              return;
            }
            std::string_view header = "Outstanding NFS calls"sv;
            fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
            for (const auto& call : outstandingCalls) {
              fmt::print("+ {}\n", formatNfsCall(call));
            }
            fmt::print("{}\n", std::string(header.size(), '-'));
          }));
#else
  outstandingCallFutures.emplace_back(
      client
          .semifuture_debugOutstandingPrjfsCalls(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .thenValue([](std::vector<PrjfsCall> outstandingCalls) {
            if (outstandingCalls.empty()) {
              return;
            }
            std::string_view header = "Outstanding PrjFS calls"sv;
            fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
            for (const auto& call : outstandingCalls) {
              fmt::print("+ {}\n", formatPrjfsCall(call));
            }
            fmt::print("{}\n", std::string(header.size(), '-'));
          }));
#endif // !_WIN32
  folly::collectAll(outstandingCallFutures).wait(kTimeout);

  std::unordered_map<uint64_t, FsEvent> activeRequests;

  std::move(traceFsStream).subscribeInline([&](folly::Try<FsEvent>&& event) {
    if (event.hasException()) {
      fmt::print("Error: {}\n", folly::exceptionStr(event.exception()));
      return;
    }

    FsEvent& evt = event.value();

    const FsEventType eventType = *evt.type();
    const FuseCall* fuseRequest =
        apache::thrift::get_pointer(evt.fuseRequest());
    const NfsCall* nfsRequest = apache::thrift::get_pointer(evt.nfsRequest());
    const PrjfsCall* prjfsRequest =
        apache::thrift::get_pointer(evt.prjfsRequest());
    if (!fuseRequest && !nfsRequest && !prjfsRequest) {
      fprintf(stderr, "Error: trace event must have a non-null *Request\n");
      return;
    }

    uint64_t unique = 0;
    if (fuseRequest) {
      unique = *fuseRequest->unique();
    } else if (nfsRequest) {
      unique = static_cast<uint32_t>(*nfsRequest->xid());
    } else {
      unique = *prjfsRequest->commandId();
    }

    switch (eventType) {
      case FsEventType::UNKNOWN:
        break;
      case FsEventType::START: {
        activeRequests[unique] = evt;
        std::string callString;
        if (fuseRequest) {
          callString = formatFuseCall(
              *apache::thrift::get_pointer(evt.fuseRequest()),
              (*evt.arguments()));
        } else if (nfsRequest) {
          callString = formatNfsCall(
              *apache::thrift::get_pointer(evt.nfsRequest()),
              (*evt.arguments()));
        } else {
          callString = formatPrjfsCall(
              *apache::thrift::get_pointer(evt.prjfsRequest()),
              (*evt.arguments()));
        }
        fmt::print("+ {}\n", callString);
        break;
      }
      case FsEventType::FINISH: {
        std::string formattedCall;
        if (fuseRequest) {
          formattedCall = formatFuseCall(
              *apache::thrift::get_pointer(evt.fuseRequest()),
              "" /* arguments */,
              apache::thrift::get_pointer(evt.result())
                  ? std::to_string(*apache::thrift::get_pointer(evt.result()))
                  : "");
        } else if (nfsRequest) {
          formattedCall = formatNfsCall(
              *apache::thrift::get_pointer(evt.nfsRequest()),
              (*evt.arguments()));
        } else {
          formattedCall = formatPrjfsCall(
              *apache::thrift::get_pointer(evt.prjfsRequest()),
              (*evt.arguments()));
        }
        const auto it = activeRequests.find(unique);
        if (it != activeRequests.end()) {
          auto& record = it->second;
          uint64_t elapsedTime =
              *evt.monotonic_time_ns() - *record.monotonic_time_ns();
          fmt::print(
              "- {} in {}\n",
              formattedCall,
              fmt::format("{:.3f} \u03BCs", double(elapsedTime) / 1000.0));
          activeRequests.erase(unique);
        } else {
          fmt::print("- {}\n", formattedCall);
        }
        break;
      }
    }
  });
  fmt::print("{} was unmounted\n", FLAGS_mountRoot);
  return 0;
}

char thriftRequestEventTypeSymbol(const ThriftRequestEvent& event) {
  switch (*event.eventType()) {
    case ThriftRequestEventType::START:
      return '+';
    case ThriftRequestEventType::FINISH:
      return '-';
    case ThriftRequestEventType::UNKNOWN:
      break;
  }
  return ' ';
}

// Thrift async C++ method name prefixes to omit from output.
//
// For p1, p2 in this vector: If p1 is a prefix of p2, it must be located
// *after* p2 in the vector.
const std::string_view kAsyncThriftMethodPrefixes[] = {
    "semifuture_",
    "future_",
    "async_tm_",
    "async_",
    "co_",
};

std::string stripAsyncThriftMethodPrefix(const std::string& method) {
  for (const auto& prefix : kAsyncThriftMethodPrefixes) {
    if (method.find(prefix) == 0) {
      return method.substr(prefix.length());
    }
  }
  return method;
}

std::string formatThriftRequestMetadata(const ThriftRequestMetadata& request) {
  std::string clientPidString;
  if (*request.clientPid()) {
    clientPidString = fmt::format(" from {}", (*request.clientPid()));
  }
  return fmt::format(
      "{}{}: {}",
      *request.requestId(),
      clientPidString,
      stripAsyncThriftMethodPrefix(*request.method()));
}

void print_thrift_event(
    const ThriftRequestEvent& event,
    std::unordered_map<int64_t, int64_t>& startTimesNs) {
  const auto requestId = (*event.requestMetadata()).get_requestId();
  const int64_t eventNs = (*event.times()).get_monotonic_time_ns();

  std::string latencyString;
  switch (*event.eventType()) {
    case ThriftRequestEventType::START:
      startTimesNs[requestId] = eventNs;
      break;
    case ThriftRequestEventType::FINISH: {
      auto kv = startTimesNs.find(requestId);
      if (kv != startTimesNs.end()) {
        int64_t startNs = kv->second;
        int64_t latencyNs = eventNs - startNs;
        latencyString = fmt::format(" in {} μs", latencyNs / 1000);
        startTimesNs.erase(kv);
      }
    } break;
    case ThriftRequestEventType::UNKNOWN:
      break;
  }

  fmt::print(
      "{} {}{}\n",
      thriftRequestEventTypeSymbol(event),
      formatThriftRequestMetadata(*event.requestMetadata()),
      latencyString);
}

int trace_thrift(
    folly::ScopedEventBaseThread& evbThread,
    apache::thrift::RocketClientChannel::Ptr channel) {
  apache::thrift::Client<StreamingEdenService> client{std::move(channel)};

  auto future = client.semifuture_debugOutstandingThriftRequests().via(
      evbThread.getEventBase());
  apache::thrift::ClientBufferedStream<ThriftRequestEvent> traceThriftStream =
      client.semifuture_traceThriftRequestEvents()
          .via(evbThread.getEventBase())
          .get();

  std::move(future)
      .thenValue([](std::vector<ThriftRequestMetadata> outstandingRequests) {
        if (outstandingRequests.empty()) {
          return;
        }
        std::string_view header = "Outstanding Thrift requests"sv;
        fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
        for (const auto& request : outstandingRequests) {
          fmt::print("  {}\n", formatThriftRequestMetadata(request));
        }
        fmt::print("\n");
      })
      .get();

  std::string_view header = "Ongoing Thrift requests"sv;
  fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));

  std::unordered_map<int64_t, int64_t> requestStartMonoTimesNs;

  std::move(traceThriftStream)
      .subscribeInline(
          // Move the client into the callback so that it will be destroyed on
          // an EventBase thread.
          [c = std::move(client),
           startTimesNs = std::move(requestStartMonoTimesNs)](
              folly::Try<ThriftRequestEvent>&& maybeEvent) mutable {
            if (maybeEvent.hasException()) {
              fmt::print(
                  "Error: {}\n", folly::exceptionStr(maybeEvent.exception()));
              return;
            }
            print_thrift_event(maybeEvent.value(), startTimesNs);
          });

  return 0;
}

int trace_thrift_retroactive(
    folly::ScopedEventBaseThread& evbThread,
    apache::thrift::RocketClientChannel::Ptr channel) {
  auto client =
      std::make_unique<apache::thrift::Client<EdenService>>(std::move(channel));

  auto future = client->semifuture_getRetroactiveThriftRequestEvents().via(
      evbThread.getEventBase());

  std::move(future)
      .thenValue([](GetRetroactiveThriftRequestEventsResult allEvents) {
        auto events = *allEvents.events();
        std::sort(
            events.begin(), events.end(), [](const auto& a, const auto& b) {
              return a.times()->timestamp() < b.times()->timestamp();
            });

        fmt::print("Last {} thrift events\n", events.size());
        std::unordered_map<int64_t, int64_t> requestStartMonoTimesNs;
        for (auto& event : events) {
          print_thrift_event(event, requestStartMonoTimesNs);
        }
      })
      .thenError([](const folly::exception_wrapper& ex) {
        fmt::print("{}\n", ex.what());
        if (ex.get_exception<EdenError>()->errorCode() == ENOTSUP) {
          fmt::print(
              "Can't run retroactive command in eden mount without an initialized ActivityBuffer. Make sure the enable-activitybuffer config is true to save events retroactively.\n");
        }
      })
      .ensure(
          // Move the client into the callback so that it will be destroyed
          // on an EventBase thread.
          [c = std::move(client)] {})
      .get();
  return 0;
}

void print_inode_event(InodeEvent& event, size_t inode_width) {
  // Convert from ns to seconds
  time_t seconds = (*event.times()->timestamp()) / 1000000000;
  struct tm time_buffer;
  if (!localtime_r(&seconds, &time_buffer)) {
    folly::throwSystemError("localtime_r failed");
  }
  char formattedTime[30];
  if (!strftime(formattedTime, 30, "%Y-%m-%d %H:%M:%S", &time_buffer)) {
    // strftime doesn't set errno! Thus we throw a runtime_error intead of
    // calling folly:throwSystemError
    throw std::runtime_error(
        "strftime failed. Formatted string exceeds size of buffer");
  }
  auto milliseconds = *event.times()->timestamp() / 1000 % 1000000;
  fmt::print(
      "{} {}.{:0>6}  {:<{}} {}    {}      {:<10}  {}\n",
      kInodeProgresses.at(*event.progress()),
      formattedTime,
      milliseconds,
      *event.ino(),
      inode_width,
      *event.inodeType() == InodeType::TREE ? kTreeEmoji : kBlobEmoji,
      kInodeEventTypes.at(*event.eventType()),
      *event.progress() == InodeEventProgress::END
          ? formatMicrosecondTime(*event.duration())
          : "",
      *event.path());
}

int trace_inode(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  apache::thrift::Client<StreamingEdenService> client{std::move(channel)};

  apache::thrift::ClientBufferedStream<InodeEvent> traceInodeStream =
      client.semifuture_traceInodeEvents(mountRoot.stringPiece().str())
          .via(evbThread.getEventBase())
          .get();

  size_t inode_width = kStartingInodeWidth;

  std::move(traceInodeStream)
      .subscribeInline([&](folly::Try<InodeEvent>&& event) {
        if (event.hasException()) {
          fmt::print("Error: {}\n", folly::exceptionStr(event.exception()));
          return;
        }
        inode_width =
            std::max(inode_width, folly::to_ascii_size_decimal(*event->ino()));
        print_inode_event(event.value(), inode_width);
      });
  return 0;
}

int trace_inode_retroactive(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  auto client =
      std::make_unique<apache::thrift::Client<EdenService>>(std::move(channel));

  GetRetroactiveInodeEventsParams params{};
  params.mountPoint() = mountRoot.stringPiece();
  auto future = client->semifuture_getRetroactiveInodeEvents(params).via(
      evbThread.getEventBase());

  std::move(future)
      .thenValue([](GetRetroactiveInodeEventsResult allEvents) {
        auto events = *allEvents.events();
        std::sort(
            events.begin(), events.end(), [](const auto& a, const auto& b) {
              return a.times()->timestamp() < b.times()->timestamp();
            });

        fmt::print("Last {} inode events\n", events.size());

        int max_inode =
            *std::max_element(
                 events.begin(),
                 events.end(),
                 [](const auto& a, const auto& b) { return a.ino() < b.ino(); })
                 ->ino();
        size_t inode_width = std::max(
            kStartingInodeWidth, folly::to_ascii_size_decimal(max_inode));

        std::string header = fmt::format(
            "  Timestamp                   {:<{}} Type  Event  Duration    Path",
            "Ino",
            inode_width);
        fmt::print("{}\n{}\n", header, std::string(header.size() + 2, '-'));
        for (auto& event : events) {
          print_inode_event(event, inode_width);
        }
        fmt::print("{}\n", std::string(header.size() + 2, '-'));
      })
      .thenError([](const folly::exception_wrapper& ex) {
        fmt::print("{}\n", ex.what());
        if (ex.get_exception<EdenError>()->errorCode() == ENOTSUP) {
          fmt::print(
              "Can't run retroactive command in eden mount without an initialized ActivityBuffer. Make sure the enable-activitybuffer config is true to save events retroactively.\n");
        }
      })
      .ensure(
          // Move the client into the callback so that it will be destroyed
          // on an EventBase thread.
          [c = std::move(client)] {})
      .get();
  return 0;
}

AbsolutePath getSocketPath(AbsolutePathPiece mountRoot) {
  if constexpr (folly::kIsWindows) {
    auto configPath = mountRoot + ".eden"_pc + "config"_pc;
    auto config = cpptoml::parse_file(configPath.stringPiece().toString());
    auto socketPath = *config->get_qualified_as<std::string>("Config.socket");
    return AbsolutePath{socketPath};
  } else {
    return mountRoot + ".eden"_pc + "socket"_pc;
  }
}
} // namespace

int main(int argc, char** argv) {
  // Don't buffer stdout, even if piped to a file.
  setbuf(stdout, nullptr);

  folly::init(&argc, &argv);

  folly::ScopedEventBaseThread evbThread;

  AbsolutePath mountRoot{FLAGS_mountRoot};
  AbsolutePath socketPath = getSocketPath(mountRoot);

  if (FLAGS_retroactive && FLAGS_trace == "fs") {
    fmt::print("Retroactive mode not supported for fs events\n");
    return 0;
  }

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
    return FLAGS_retroactive
        ? trace_hg_retroactive(evbThread, mountRoot, std::move(channel))
        : trace_hg(evbThread, mountRoot, std::move(channel));
  } else if (FLAGS_trace == "fs") {
    return trace_fs(
        evbThread, mountRoot, std::move(channel), FLAGS_reads, FLAGS_writes);
  } else if (FLAGS_trace == "thrift") {
    return FLAGS_retroactive
        ? trace_thrift_retroactive(evbThread, std::move(channel))
        : trace_thrift(evbThread, std::move(channel));
  } else if (FLAGS_trace == "inode") {
    return FLAGS_retroactive
        ? trace_inode_retroactive(evbThread, mountRoot, std::move(channel))
        : trace_inode(evbThread, mountRoot, std::move(channel));
  } else if (FLAGS_trace.empty()) {
    fmt::print(stderr, "Must specify trace mode\n");
    return 1;
  } else {
    fmt::print(stderr, "Unknown trace mode: {}\n", FLAGS_trace);
    return 1;
  }
}
