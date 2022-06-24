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
#include <thrift/lib/cpp/util/EnumUtils.h>
#include <thrift/lib/cpp2/async/RocketClientChannel.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "common/time/TimeFormat.h"
#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"
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
static const auto kTreeEmoji = reinterpret_cast<const char*>(u8"\U0001F332");
static const auto kBlobEmoji = reinterpret_cast<const char*>(u8"\U0001F954");
static const auto kDashedArrowEmoji = reinterpret_cast<const char*>(u8"\u21E3");
static const auto kSolidArrowEmoji = reinterpret_cast<const char*>(u8"\u2193");
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
        {InodeEventType::Unknown, "?"},
        {InodeEventType::Materialize, "M"},
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

std::string formatFuseOpcode(const FuseCall& call) {
  std::string name = call.get_opcodeName();
  auto mutableName = folly::MutableStringPiece(name.data(), name.size());
  (void)mutableName.removePrefix("FUSE_");
  folly::toLowerAscii(mutableName);
  return mutableName.str();
}

std::string formatFuseCall(
    const FuseCall& call,
    const std::string& arguments = "",
    const std::string& result = "") {
  auto* processNamePtr = call.get_processName();
  std::string processNameString = processNamePtr
      ? fmt::format("{}({})", processNamePtr->c_str(), call.get_pid())
      : std::to_string(call.get_pid());

  std::string argString = arguments.empty()
      ? fmt::format("{}", call.get_nodeid())
      : fmt::format("{}, {}", call.get_nodeid(), arguments);
  std::string resultString =
      result.empty() ? result : fmt::format(" = {}", result);

  return fmt::format(
      "{} from {}: {}({}){}",
      call.get_unique(),
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
      static_cast<uint32_t>(call.get_xid()),
      call.get_procName(),
      call.get_procNumber(),
      arguments);
}

std::string formatPrjfsCall(
    const PrjfsCall& call,
    std::string arguments = std::string{}) {
  if (arguments.empty()) {
    return fmt::format(
        "{} from {}: {}",
        call.get_commandId(),
        call.get_pid(),
        apache::thrift::util::enumName(call.get_callType(), "(unknown)"));
  } else {
    return arguments;
  }
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
    const HgImportPriority importPriority = *evt.importPriority_ref();
    const HgImportCause importCause = *evt.importCause_ref();
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
                fmt::format(" queued for {}", formatNsTimeToMs(queueTime));
          }
        } else {
          // This event was queued before we subscribed.
        }
        break;

      case HgEventType::FINISH:
        if (startEvent) {
          auto fetchTime = evt.times_ref()->monotonic_time_ns_ref().value() -
              startEvent->times_ref()->monotonic_time_ns_ref().value();
          timeAnnotation =
              fmt::format(" fetched in {}", formatNsTimeToMs(fetchTime));
        }
        break;
    }

    const char* eventTypeStr =
        folly::get_default(kHgEventTypes, eventType, "?");
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
          *evt.path_ref(),
          timeAnnotation);
    } else {
      fmt::print(
          "{} {} {}{}\n",
          eventTypeStr,
          resourceTypeStr,
          *evt.path_ref(),
          timeAnnotation);
    }
  });

  fmt::print("{} was unmounted\n", FLAGS_mountRoot);
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

  StreamingEdenServiceAsyncClient client{std::move(channel)};
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

    const FsEventType eventType = evt.get_type();
    const FuseCall* fuseRequest = evt.get_fuseRequest();
    const NfsCall* nfsRequest = evt.get_nfsRequest();
    const PrjfsCall* prjfsRequest = evt.get_prjfsRequest();
    if (!fuseRequest && !nfsRequest && !prjfsRequest) {
      fprintf(stderr, "Error: trace event must have a non-null *Request\n");
      return;
    }

    uint64_t unique = 0;
    if (fuseRequest) {
      unique = fuseRequest->get_unique();
    } else if (nfsRequest) {
      unique = static_cast<uint32_t>(nfsRequest->get_xid());
    } else {
      unique = prjfsRequest->get_commandId();
    }

    switch (eventType) {
      case FsEventType::UNKNOWN:
        break;
      case FsEventType::START: {
        activeRequests[unique] = evt;
        std::string callString;
        if (fuseRequest) {
          callString =
              formatFuseCall(*evt.get_fuseRequest(), evt.get_arguments());
        } else if (nfsRequest) {
          callString =
              formatNfsCall(*evt.get_nfsRequest(), evt.get_arguments());
        } else {
          callString =
              formatPrjfsCall(*evt.get_prjfsRequest(), evt.get_arguments());
        }
        fmt::print("+ {}\n", callString);
        break;
      }
      case FsEventType::FINISH: {
        std::string formattedCall;
        if (fuseRequest) {
          formattedCall = formatFuseCall(
              *evt.get_fuseRequest(),
              "" /* arguments */,
              evt.get_result() ? std::to_string(*evt.get_result()) : "");
        } else if (nfsRequest) {
          formattedCall =
              formatNfsCall(*evt.get_nfsRequest(), evt.get_arguments());
        } else {
          formattedCall =
              formatPrjfsCall(*evt.get_prjfsRequest(), evt.get_arguments());
        }
        const auto it = activeRequests.find(unique);
        if (it != activeRequests.end()) {
          auto& record = it->second;
          uint64_t elapsedTime =
              evt.get_monotonic_time_ns() - record.get_monotonic_time_ns();
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

std::string formatThriftRequestMetadata(const ThriftRequestMetadata& request) {
  return fmt::format("{}: {}", request.get_requestId(), request.get_method());
}

int trace_thrift(
    folly::ScopedEventBaseThread& evbThread,
    apache::thrift::RocketClientChannel::Ptr channel) {
  StreamingEdenServiceAsyncClient client{std::move(channel)};

  auto future = client.semifuture_debugOutstandingThriftRequests().via(
      evbThread.getEventBase());

  std::move(future)
      .thenValue(
          // Move the client into the callback so that it will be destroyed on
          // an EventBase thread.
          [c = std::move(client)](
              std::vector<ThriftRequestMetadata> outstandingRequests) {
            if (outstandingRequests.empty()) {
              return;
            }
            std::string_view header = "Outstanding Thrift requests"sv;
            fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
            for (const auto& request : outstandingRequests) {
              fmt::print("+ {}\n", formatThriftRequestMetadata(request));
            }
            fmt::print("{}\n", std::string(header.size(), '-'));
          })
      .get();

  return 0;
}

int trace_inode(
    folly::ScopedEventBaseThread& evbThread,
    const AbsolutePath& mountRoot,
    apache::thrift::RocketClientChannel::Ptr channel) {
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  GetRetroactiveInodeEventsParams params{};
  params.mountPoint_ref() = mountRoot.stringPiece();
  auto future = client->semifuture_getRetroactiveInodeEvents(params).via(
      evbThread.getEventBase());

  std::move(future)
      .thenValue([](GetRetroactiveInodeEventsResult allEvents) {
        // For each event, create a corresponding event for the event's
        // start and sort by timestamp
        std::vector<InodeEvent> events;
        auto size = 2 * allEvents.events_ref()->size();
        events.reserve(size);
        for (auto finish : *allEvents.events_ref()) {
          InodeEvent start{};
          start.timestamp_ref() =
              *finish.timestamp_ref() - 1000 * (*finish.duration_ref());
          start.ino_ref() = *finish.ino_ref();
          start.inodeType_ref() = *finish.inodeType_ref();
          start.eventType_ref() = *finish.eventType_ref();
          // Use -1 for the duration to distinguish the event's start
          start.duration_ref() = -1;
          events.push_back(std::move(start));
          events.push_back(std::move(finish));
        }
        std::sort(
            events.begin(), events.end(), [](const auto& a, const auto& b) {
              return a.timestamp_ref() < b.timestamp_ref();
            });

        fmt::print("Last {} inode events\n", size);
        std::string_view header =
            "  Timestamp                   Ino   Type  Event  Duration "sv;
        fmt::print("{}\n{}\n", header, std::string(header.size(), '-'));
        std::string format = "%Y-%m-%d %H:%M:%S";

        for (auto& event : events) {
          time_t seconds = (*event.timestamp_ref()) / 1000000000;
          auto formattedTime = TimeFormat::FormatTime(format, seconds);
          auto milliseconds = *event.timestamp_ref() / 1000 % 1000000;
          fmt::print(
              "{} {}.{:0>6}  {:<5} {}    {}      {}\n",
              // Use arrow emoji based on if the event started or finished
              (*event.duration_ref() < 0) ? kDashedArrowEmoji
                                          : kSolidArrowEmoji,
              formattedTime,
              milliseconds,
              *event.ino_ref(),
              *event.inodeType_ref() == InodeType::TREE ? kTreeEmoji
                                                        : kBlobEmoji,
              kInodeEventTypes.at(*event.eventType_ref()),
              formatMicrosecondTime(*event.duration_ref()));
        }
        fmt::print("{}\n", std::string(header.size(), '-'));
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

  if (FLAGS_trace != "inode" && FLAGS_retroactive) {
    fmt::print("Only eden trace inode currently supports retroactive mode\n");
    return 0;
  }
  if (FLAGS_trace == "inode" && !FLAGS_retroactive) {
    fmt::print(
        "`eden trace inode` is only currently supported in retroactive mode\n");
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
    return trace_hg(evbThread, mountRoot, std::move(channel));
  } else if (FLAGS_trace == "fs") {
    return trace_fs(
        evbThread, mountRoot, std::move(channel), FLAGS_reads, FLAGS_writes);
  } else if (FLAGS_trace == "thrift") {
    return trace_thrift(evbThread, std::move(channel));
  } else if (FLAGS_trace == "inode") {
    return trace_inode(evbThread, mountRoot, std::move(channel));
  } else if (FLAGS_trace.empty()) {
    fmt::print(stderr, "Must specify trace mode\n");
    return 1;
  } else {
    fmt::print(stderr, "Unknown trace mode: {}\n", FLAGS_trace);
    return 1;
  }
}
