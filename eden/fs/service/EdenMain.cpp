/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenMain.h"
#include "eden/fs/telemetry/SessionId.h"

#include <optional>

#include <fb303/FollyLoggingHandler.h>
#include <fb303/TFunctionStatHandler.h>
#include <folly/Conv.h>
#include <folly/MapUtil.h>
#include <folly/experimental/FunctionScheduler.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/Unistd.h>
#include <folly/ssl/Init.h>
#include <folly/stop_watch.h>
#include <thrift/lib/cpp2/Flags.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/privhelper/PrivHelperImpl.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/EdenServiceHandler.h" // for kServiceName
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/service/StartupStatusSubscriber.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/FilteredBackingStore.h"
#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/fs/store/filter/HgSparseFilter.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/IHiveLogger.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/UserInfo.h"
#include "eden/fs/utils/WinStackTrace.h"

#ifdef EDEN_HAVE_GIT
#include "eden/fs/store/git/GitBackingStore.h" // @manual
#endif

DEFINE_bool(edenfs, false, "This legacy argument is ignored.");
DEFINE_bool(allowRoot, false, "Allow running eden directly as root");

THRIFT_FLAG_DECLARE_bool(server_header_reject_framed);

using folly::StringPiece;
using std::string;

namespace facebook::eden {

namespace {

EdenStatsPtr getGlobalEdenStats() {
  // A running EdenFS daemon only needs a single EdenStats instance. Avoid
  // atomic reference counts with RefPtr::singleton. We could use
  // folly::Singleton but that makes unit testing harder.
  static EdenStats* gEdenStats = new EdenStats;
  return EdenStatsPtr::singleton(*gEdenStats);
}

SessionInfo makeSessionInfo(
    const UserInfo& userInfo,
    std::string hostname,
    std::string edenVersion) {
  SessionInfo env;
  env.username = userInfo.getUsername();
  env.hostname = std::move(hostname);
  env.sandcastleInstanceId = getSandcastleInstanceId();
  env.os = getOperatingSystemName();
  env.osVersion = getOperatingSystemVersion();
  env.edenVersion = std::move(edenVersion);
#if defined(__APPLE__)
  env.systemArchitecture = getOperatingSystemArchitecture();
#endif
  return env;
}

constexpr int kExitCodeSuccess = 0;
constexpr int kExitCodeError = 1;
constexpr int kExitCodeUsage = 2;

} // namespace

std::shared_ptr<BackingStore> DefaultBackingStoreFactory::createBackingStore(
    BackingStoreType type,
    const CreateParams& params) {
  if (auto* fn = folly::get_ptr(registered_, type)) {
    return (*fn)(params);
  }

  throw std::domain_error(
      folly::to<std::string>("unsupported backing store type: ", type));
}

void DefaultBackingStoreFactory::registerFactory(
    BackingStoreType type,
    DefaultBackingStoreFactory::Factory factory) {
  auto [it, inserted] = registered_.emplace(type, std::move(factory));
  if (!inserted) {
    EDEN_BUG() << "attempted to register BackingStore " << type << " twice";
  }
}

void EdenMain::runServer(const EdenServer& server) {
  // ThriftServer::serve() will drive the current thread's EventBase.
  // Verify that we are being called from the expected thread, and will end up
  // driving the EventBase returned by EdenServer::getMainEventBase().
  XCHECK_EQ(
      server.getMainEventBase(),
      folly::EventBaseManager::get()->getEventBase());

  fb303::fbData->setExportedValue("build_package_name", EDEN_PACKAGE_NAME);
  fb303::fbData->setExportedValue("build_package_version", EDEN_VERSION);
  fb303::fbData->setExportedValue("build_package_release", EDEN_RELEASE);
  fb303::fbData->setExportedValue("build_revision", EDEN_BUILD_REVISION);
  fb303::fbData->setExportedValue(
      "build_time_unix", folly::to<std::string>(EDEN_BUILD_TIME_UNIX));

  fb303::withThriftFunctionStats(
      kServiceName, server.getHandler().get(), [&] { server.serve(); });
}

namespace {
std::shared_ptr<HgQueuedBackingStore> createHgQueuedBackingStore(
    const BackingStoreFactory::CreateParams& params,
    const AbsolutePath& repoPath,
    std::shared_ptr<ReloadableConfig> reloadableConfig) {
  auto underlyingStore = std::make_unique<HgBackingStore>(
      repoPath,
      params.localStore,
      params.serverState->getThreadPool().get(),
      reloadableConfig,
      params.sharedStats.copy(),
      params.serverState->getStructuredLogger(),
      &params.serverState->getFaultInjector());

  return std::make_shared<HgQueuedBackingStore>(
      params.localStore,
      params.sharedStats.copy(),
      std::move(underlyingStore),
      reloadableConfig,
      params.serverState->getStructuredLogger(),
      std::make_unique<BackingStoreLogger>(
          params.serverState->getStructuredLogger(),
          params.serverState->getProcessInfoCache()));
}
} // namespace

void EdenMain::registerStandardBackingStores() {
  using CreateParams = BackingStoreFactory::CreateParams;

  registerBackingStore(BackingStoreType::EMPTY, [](const CreateParams&) {
    return std::make_shared<EmptyBackingStore>();
  });

  registerBackingStore(BackingStoreType::HG, [](const CreateParams& params) {
    const auto repoPath = realpath(params.name);
    auto reloadableConfig = params.serverState->getReloadableConfig();

    auto hgQueuedBackingStore =
        createHgQueuedBackingStore(params, repoPath, reloadableConfig);

    auto localStoreCaching = reloadableConfig->getEdenConfig()
                                 ->hgEnableBlobMetaLocalStoreCaching.getValue()
        ? LocalStoreCachedBackingStore::CachingPolicy::TreesAndBlobMetadata
        : LocalStoreCachedBackingStore::CachingPolicy::Trees;
    return std::make_shared<LocalStoreCachedBackingStore>(
        std::move(hgQueuedBackingStore),
        params.localStore,
        params.sharedStats.copy(),
        localStoreCaching);
  });

  registerBackingStore(
      BackingStoreType::FILTEREDHG,
      [](const BackingStoreFactory::CreateParams& params) {
        const auto repoPath = realpath(params.name);
        auto reloadableConfig = params.serverState->getReloadableConfig();
        auto localStoreCaching =
            reloadableConfig->getEdenConfig()
                ->hgEnableBlobMetaLocalStoreCaching.getValue()
            ? LocalStoreCachedBackingStore::CachingPolicy::TreesAndBlobMetadata
            : LocalStoreCachedBackingStore::CachingPolicy::Trees;
        auto hgSparseFilter = std::make_unique<HgSparseFilter>(repoPath);

        auto hgQueuedBackingStore =
            createHgQueuedBackingStore(params, repoPath, reloadableConfig);
        auto wrappedStore = std::make_shared<FilteredBackingStore>(
            std::move(hgQueuedBackingStore), std::move(hgSparseFilter));
        return std::make_shared<LocalStoreCachedBackingStore>(
            std::move(wrappedStore),
            params.localStore,
            params.sharedStats.copy(),
            localStoreCaching);
      });

  registerBackingStore(
      BackingStoreType::GIT,
      [](const CreateParams& params) -> std::shared_ptr<BackingStore> {
#ifdef EDEN_HAVE_GIT
        const auto repoPath = realpath(params.name);
        return std::make_shared<LocalStoreCachedBackingStore>(
            std::make_shared<GitBackingStore>(repoPath),
            params.localStore,
            params.sharedStats.copy(),
            LocalStoreCachedBackingStore::CachingPolicy::TreesAndBlobMetadata);
#else // EDEN_HAVE_GIT
        (void)params;
        throw std::domain_error(
            "support for Git was not enabled in this EdenFS build");
#endif // EDEN_HAVE_GIT
      });
}

std::string DefaultEdenMain::getEdenfsBuildName() {
  StringPiece version(EDEN_VERSION);
  StringPiece release(EDEN_RELEASE);

  if (!version.empty()) {
    return folly::to<string>("edenfs ", version, "-", release);
  }

  // Assume this is a development build if EDEN_VERSION is unset.
  return "edenfs (dev build)";
}

std::string DefaultEdenMain::getEdenfsVersion() {
  StringPiece version(EDEN_VERSION);

  if (!version.empty()) {
    return folly::to<string>(version);
  }

  return "(dev build)";
}

std::string DefaultEdenMain::getLocalHostname() {
  return getHostname();
}

void DefaultEdenMain::didFollyInit() {}

void DefaultEdenMain::prepare(const EdenServer& /*server*/) {
  fb303::registerFollyLoggingOptionHandlers();

  registerStandardBackingStores();
}

ActivityRecorderFactory DefaultEdenMain::getActivityRecorderFactory() {
  return [](std::shared_ptr<const EdenMount>) {
    return std::make_unique<NullActivityRecorder>();
  };
}

std::shared_ptr<IHiveLogger> DefaultEdenMain::getHiveLogger(
    SessionInfo /*sessionInfo*/,
    std::shared_ptr<EdenConfig> /*edenConfig*/) {
  return std::make_shared<NullHiveLogger>();
}

int runEdenMain(EdenMain&& main, int argc, char** argv) {
  ////////////////////////////////////////////////////////////////////
  // There are two options for running test instances or development builds of
  // EdenFS:
  //
  // 1. EdenFS uses a system (or pre-installed) privhelper so that `sudo` is not
  // required to run the privhelper as root. When installed, the privhelper is
  // setuid-root and thus the EdenFS daemon never runs as root.
  //
  // 2. EdenFS is started with sudo in order to execute a dev instance of
  // privhelper as root.
  //
  // #1 is the default behavior, but #2 can be achieved through the use of
  // environment variables. See prepare_edenfs_privileges() in fs/cli/daemon.py
  // for more information on how this works.
  //
  // Since this code can be started with root privileges, we should be very
  // careful about anything EdenFS does here before it drops privileges.  In
  // general do not add any new code here at the start of main: new
  // initialization logic should only go after the "Root privileges dropped"
  // comment below.
  ////////////////////////////////////////////////////////////////////

  // Start the privhelper process, then drop privileges in the main process.
  // This should be done as early as possible, so that everything else EdenFS
  // does runs only with normal user privileges. Note: as mentioned above, this
  // is not an issue in the default case since EdenFS will not be run as root,
  // and only the privhelper daemon will be run as a setuid-root binary.
  //
  // EdenFS does this even before calling folly::init().  The privhelper server
  // process will call folly::init() on its own.
  //
  // If the privileged parent edenfs process has already started a privhelper
  // process, then the --privhelper_fd flag is given and this child process will
  // use it to connect to the existing privhelper.
  auto identity = UserInfo::lookup();
  auto privHelper = startOrConnectToPrivHelper(identity, argc, argv);
  identity.dropPrivileges();

  ////////////////////////////////////////////////////////////////////
  //// Root privileges dropped
  ////////////////////////////////////////////////////////////////////

#ifdef _WIN32
  installWindowsExceptionFilter();
#endif

  folly::stop_watch<> daemonStart;

  std::vector<std::string> originalCommandLine{argv, argv + argc};

  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);
  if (argc != 1) {
    fprintf(stderr, "error: unexpected trailing command line arguments\n");
    return kExitCodeUsage;
  }

  if (identity.getUid() == 0 && !FLAGS_allowRoot) {
    fprintf(
        stderr,
        "error: you appear to be running eden as root, "
        "rather than using\n"
        "sudo or a setuid binary.  This is normally undesirable.\n"
        "Pass in the --allowRoot flag if you really mean to run "
        "eden as root.\n");
    return kExitCodeUsage;
  }

  auto loggingConfig = folly::parseLogConfig("eden=DBG2; default:async=true");
  folly::LoggerDB::get().updateConfig(loggingConfig);

  main.didFollyInit();

  // Temporary hack until client is migrated to supported channel
  THRIFT_FLAG_SET_MOCK(server_header_reject_framed, false);

  std::shared_ptr<EdenConfig> edenConfig;
  try {
    edenConfig = getEdenConfig(identity);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return kExitCodeError;
  }

  main.prepareConfig(*edenConfig);

  auto startupStatusChannel = std::make_shared<StartupStatusChannel>();
  auto logPath = getLogPath(edenConfig->edenDir.getValue());
  auto startupLogger = daemonizeIfRequested(
      logPath, privHelper.get(), originalCommandLine, startupStatusChannel);
  std::optional<EdenServer> server;
  auto prepareFuture = folly::Future<folly::Unit>::makeEmpty();
  try {
    // If stderr was redirected to a log file, inform the privhelper
    // to make sure it logs to our current stderr.
    if (!logPath.empty()) {
      privHelper->setLogFileBlocking(
          folly::File(STDERR_FILENO, /*ownsFd=*/false));
    }

    privHelper->setDaemonTimeoutBlocking(
        edenConfig->fuseDaemonTimeout.getValue());

    // Since we are a daemon, and we don't ever want to be in a situation
    // where we hold any open descriptors through a fuse mount that points
    // to ourselves (which can happen during takeover), we chdir to `/`
    // to avoid having our cwd reference ourselves if the user runs
    // `eden daemon --takeover` from within an eden mount
    folly::checkPosixError(chdir("/"), "failed to chdir(/)");

    // Set some default glog settings, to be applied unless overridden on the
    // command line
    gflags::SetCommandLineOptionWithMode(
        "logtostderr", "1", gflags::SET_FLAGS_DEFAULT);
    gflags::SetCommandLineOptionWithMode(
        "minloglevel", "1", gflags::SET_FLAGS_DEFAULT);

    startupLogger->log(
        "Starting ",
        main.getEdenfsBuildName(),
        ", pid ",
        getpid(),
        ", session_id ",
        getSessionId());

    auto sessionInfo = makeSessionInfo(
        identity, main.getLocalHostname(), main.getEdenfsVersion());

    auto hiveLogger = main.getHiveLogger(sessionInfo, edenConfig);

    server.emplace(
        std::move(originalCommandLine),
        std::move(identity),
        getGlobalEdenStats(),
        std::move(sessionInfo),
        std::move(privHelper),
        std::move(edenConfig),
        main.getActivityRecorderFactory(),
        main.getBackingStoreFactory(),
        std::move(hiveLogger),
        std::move(startupStatusChannel),
        main.getEdenfsVersion());

    main.prepare(server.value());

    prepareFuture = server->prepare(startupLogger);
  } catch (const std::exception& ex) {
    auto startTimeInSeconds =
        std::chrono::duration<double>{daemonStart.elapsed()}.count();
    if (server) {
      server->getServerState()->getStructuredLogger()->logEvent(
          DaemonStart{startTimeInSeconds, FLAGS_takeover, false /*success*/});
    }
    startupLogger->exitUnsuccessfully(
        kExitCodeError, "error starting EdenFS: ", folly::exceptionStr(ex));
  }

  std::move(prepareFuture)
      .thenTry([startupLogger,
                structuredLogger =
                    server->getServerState()->getStructuredLogger(),
                daemonStart](folly::Try<folly::Unit>&& result) {
        // If an error occurred this means that we failed to mount all of
        // the mount points or there was an issue opening the LocalStore.
        //
        // LocalStore errors mean that Eden can't operate correctly, so we
        // need to exit.
        //
        // Mount errors are fine. We have still started and will
        // continue running, so we can report successful startup.
        if (result.hasException()) {
          if (auto* err = result.tryGetExceptionObject<
                          EdenServer::LocalStoreOpenError>()) {
            auto startTimeInSeconds =
                std::chrono::duration<double>{daemonStart.elapsed()}.count();
            structuredLogger->logEvent(DaemonStart{
                startTimeInSeconds, FLAGS_takeover, false /*success*/});
            // Note: this will cause EdenFs to exit abruptly. We are not using
            // normal shutdown procedures. This is consistent with other
            // pre-mount startup errors. Admittedly this will leave hung mounts
            // during graceful restarts:
            // TODO(T164077169): attempt to cleanup mounts left behind by a
            // graceful restart when EdenFS fails to startup after recieving
            // takeover data.
            startupLogger->exitUnsuccessfully(
                kExitCodeError,
                "error starting EdenFS: ",
                folly::exceptionStr(*err));
          }
          // Log an overall error message here.
          // We will have already logged more detailed messages for each
          // mount failure when it occurred.
          startupLogger->warn(
              "did not successfully remount all repositories: ",
              result.exception().what());
        }
        auto startTimeInSeconds =
            std::chrono::duration<double>{daemonStart.elapsed()}.count();
        startupLogger->success(startTimeInSeconds);
      })
      .ensure(
          [daemonStart,
           structuredLogger = server->getServerState()->getStructuredLogger(),
           takeover = FLAGS_takeover] {
            // This value is slightly different from `startTimeInSeconds`
            // we pass into `startupLogger->success()`, but should be
            // identical.
            auto startTimeInSeconds =
                std::chrono::duration<double>{daemonStart.elapsed()}.count();
            // Here we log a success even if we did not successfully remount
            // all repositories (if prepareFuture had an exception). In the
            // future it would be helpful to log number of successful vs
            // unsuccessful remounts
            structuredLogger->logEvent(
                DaemonStart{startTimeInSeconds, takeover, true /*success*/});
          });

  while (true) {
    main.runServer(server.value());
    if (server->performCleanup()) {
      break;
    }
    // performCleanup() returns false if a takeover shutdown attempt
    // failed.  Continue and re-run the server in this case.
  }

  main.cleanup();

  XLOG(INFO) << "EdenFS exiting successfully";
  return kExitCodeSuccess;
}

} // namespace facebook::eden
