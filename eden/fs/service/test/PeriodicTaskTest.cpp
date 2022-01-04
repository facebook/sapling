/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/PeriodicTask.h"

#include <functional>

#include <folly/io/async/test/Util.h>
#include <folly/logging/test/TestLogHandler.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "eden/fs/service/EdenServer.h"
#include "eden/fs/testharness/TestServer.h"

using folly::EventBase;
using folly::StringPiece;
using folly::TimePoint;
using std::make_shared;
using std::string;
using testing::ElementsAre;
using testing::MatchesRegex;
using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {

class TestTask : public PeriodicTask {
 public:
  explicit TestTask(
      EdenServer* server,
      StringPiece name,
      std::function<void()>&& fn)
      : PeriodicTask(server, name), fn_(std::move(fn)) {}

  void runTask() override {
    fn_();
  }

 private:
  std::function<void()> fn_;
};

/**
 * PreciseEventBase causes the current thread to use an EventBase with a 1ms
 * tick interval while the PreciseEventBase object exists.
 *
 * By default EventBase uses a 10ms tick interval for it's HHWheelTimer.
 * This causes tasks to run up to 10ms behind the scheduled time (and for some
 * reason 20ms behind every once in a while).
 *
 * Set up the EventBase for our thread with a smaller 1ms tick duration so we
 * can check the intervals a little more precisely.  Otherwise we would need
 * to sleep for longer (and make the test longer) to have high confidence that
 * the test intervals are being run correctly.
 */
struct PreciseEventBase {
 public:
  PreciseEventBase() {
    folly::EventBaseManager::get()->setEventBase(
        &eventBase, /*takeOwnership=*/false);
  }
  ~PreciseEventBase() {
    folly::EventBaseManager::get()->clearEventBase();
  }

  folly::EventBase eventBase{1ms};
};

class PeriodicTaskTest : public ::testing::Test {
 protected:
  struct MultiTaskResult {
    // The time the server started
    TimePoint start;
    // A vector with 1 entry per task, containing the times that task was run
    std::vector<std::vector<TimePoint>> taskInvocations;
  };

  EventBase& getEventBase() {
    return preciseEventBase_.eventBase;
  }
  EdenServer& getServer() {
    return testServer_.getServer();
  }

  void runServer() {
    // Add log statements around serve primarily so we can tell in the test
    // output how long the server ran for.  We previously had some test failures
    // because EdenServer took a long time to start, so our 200ms timeout
    // expired before the server had actually run for any significant length of
    // time.
    XLOG(INFO) << "serve start";
    auto& thriftServer = getServer().getServer();
    thriftServer->serve();
    XLOG(INFO) << "serve done";
  }

  /**
   * Run a function from the server's main EventBase thread once the server has
   * started.
   *
   * The goal of this function is to delay running the supplied function until
   * the server is up and running, so we can begin performing timing tests
   * without having them be affected by the latency required to start the
   * server.
   */
  template <typename F>
  void runOnServerStart(F&& fn) {
    class Callback : public EventBase::LoopCallback {
     public:
      explicit Callback(EventBase* evb, F&& fn)
          : eventBase_(evb), fn_(std::forward<F>(fn)) {}
      void runLoopCallback() noexcept override {
        if (delayLoops_ > 0) {
          // Delay for a few iterations of the loop to wait for things to settle
          // down and for any tasks that run immediate on start-up to finish
          // running.
          --delayLoops_;
          eventBase_->runInLoop(this);
        } else {
          XLOG(INFO) << "server started";
          fn_();
          delete this;
        }
      }

     private:
      EventBase* eventBase_;
      size_t delayLoops_{3};
      F fn_;
    };

    auto cb = std::make_unique<Callback>(&getEventBase(), std::forward<F>(fn));
    getEventBase().runInLoop(cb.release());
  }

  /**
   * Run several tasks for the specified number of iterations.
   */
  MultiTaskResult runMultipleTasks(
      size_t numTasks,
      size_t runsPerTask,
      std::chrono::milliseconds interval,
      bool splay);

  PreciseEventBase preciseEventBase_;
  TestServer testServer_;
};

} // namespace

PeriodicTaskTest::MultiTaskResult PeriodicTaskTest::runMultipleTasks(
    size_t numTasks,
    size_t runsPerTask,
    std::chrono::milliseconds interval,
    bool splay) {
  // Prepare up the tasks and a vector for the results
  std::vector<TestTask> tasks;
  tasks.reserve(numTasks);
  std::vector<std::vector<TimePoint>> taskInvocations;
  taskInvocations.resize(numTasks);

  auto& server = getServer();
  size_t tasksRunning = numTasks;
  for (size_t n = 0; n < numTasks; ++n) {
    tasks.emplace_back(&server, folly::to<string>("task", n), [&, n] {
      XLOG(INFO) << "task " << n << " iteration " << taskInvocations[n].size();
      taskInvocations[n].emplace_back();
      if (taskInvocations[n].size() == runsPerTask) {
        XLOG(INFO) << "stopping task " << n;
        tasks[n].updateInterval(0ms);
        --tasksRunning;
        if (tasksRunning == 0) {
          server.stop();
        }
      } else if (taskInvocations[n].size() > runsPerTask) {
        XLOG(FATAL) << "task " << n << " invoked too many times";
      }
    });
  }

  // Start all of the tasks from inside the EventBase
  // once we have started the server.
  std::optional<TimePoint> start;
  runOnServerStart([&] {
    start = TimePoint();
    for (auto& task : tasks) {
      task.updateInterval(interval, splay);
    }
  });

  runServer();

  return MultiTaskResult{start.value(), taskInvocations};
}

TEST_F(PeriodicTaskTest, taskException) {
  // Make sure that the periodic task keeps getting run even after it throws an
  // exception and that the exception isn't propagated up farther to the main
  // thread.
  constexpr auto kInterval = 10ms;
  constexpr size_t kNumInvocations = 5;
  size_t count = 0;
  TestTask task(&getServer(), "test_task", [&] {
    ++count;
    if (count == kNumInvocations) {
      getServer().stop();
    }
    throw std::runtime_error("exception just for testing");
  });
  task.updateInterval(kInterval);

  runServer();
  ASSERT_EQ(kNumInvocations, count);
}

TEST_F(PeriodicTaskTest, slowTask) {
  // Add a log handler to record messages logged by the PeriodicTask code.
  auto logHandler = make_shared<folly::TestLogHandler>();
  folly::LoggerDB::get()
      .getCategory("eden/fs/service/PeriodicTask")
      ->addHandler(logHandler);

  // TODO(T93776519) remove once Buck v2 strips the fbcode prefix.
  folly::LoggerDB::get()
      .getCategory("fbcode/eden/fs/service/PeriodicTask")
      ->addHandler(logHandler);

  // Schedule a slow periodic task.
  // We test to make sure that log messages are generated about the fact that it
  // runs slowly.
  constexpr auto kInterval = 10ms;
  constexpr auto kSlowTime = 70ms;
  constexpr size_t kNumInvocations = 8;
  size_t count = 0;
  TestTask task(&getServer(), "test_task", [&] {
    ++count;
    if (count == kNumInvocations) {
      getServer().stop();
    }
    /* sleep override */ std::this_thread::sleep_for(kSlowTime);
  });
  task.updateInterval(kInterval);

  // Run the server.
  runServer();
  ASSERT_EQ(kNumInvocations, count);

  // The PeriodicTask code should have logged on the 1st, 2nd, and 4th, and 8th
  // invocations of the slow task (it logs every 2^N iterations)
  auto logMessages = logHandler->getMessageValues();
  EXPECT_THAT(
      logHandler->getMessageValues(),
      ElementsAre(
          MatchesRegex("slow periodic task: test_task took .*ms; "
                       "has run slowly 1 times"),
          MatchesRegex("slow periodic task: test_task took .*ms; "
                       "has run slowly 2 times"),
          MatchesRegex("slow periodic task: test_task took .*ms; "
                       "has run slowly 4 times"),
          MatchesRegex("slow periodic task: test_task took .*ms; "
                       "has run slowly 8 times")));
}
