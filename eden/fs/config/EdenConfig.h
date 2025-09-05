/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>
#include <optional>
#include <vector>

#include <thrift/lib/cpp/concurrency/ThreadManager.h>

#include "common/rust/shed/hostcaps/hostcaps.h"

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/ConfigSetting.h"
#include "eden/fs/config/ConfigSource.h"
#include "eden/fs/config/ConfigVariables.h"
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/config/InodeCatalogType.h"
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/config/ReaddirPrefetch.h"
#include "eden/fs/eden-config.h"

namespace re2 {
class RE2;
} // namespace re2

namespace facebook::eden {

extern const AbsolutePath kUnspecifiedDefault;

/**
 * EdenConfig holds the Eden configuration settings. It is constructed from
 * cli settings, user configuration files, system configuration files and
 * default values. It provides methods to determine if configuration files
 * have been changed (fstat on the source files).
 *
 * To augment configuration, add a ConfigurationSetting member variable to
 * this class. ConfigurationSettings require a key to identify the setting
 * in configuration files. For example, "core:edenDirectory".
 */
class EdenConfig : private ConfigSettingManager {
 public:
  using SourceVector = std::vector<std::shared_ptr<ConfigSource>>;

  /**
   * Manually construct a EdenConfig object with some default values and
   * ConfigSources. Duplicate ConfigSources for the same ConfigSourceType are
   * disallowed. ConfigSources are immediately applied to the ConfigSettings.
   */
  explicit EdenConfig(
      ConfigVariables substitutions,
      AbsolutePathPiece userHomePath,
      AbsolutePathPiece systemConfigDir,
      SourceVector configSources);

  /**
   * EdenConfig is heap-allocated and not copyable or moveable in general. This
   * constructor clones an existing EdenConfig and is called when a config file
   * is reloaded.
   */
  explicit EdenConfig(const EdenConfig& source);
  explicit EdenConfig(EdenConfig&& source) = delete;

  EdenConfig& operator=(const EdenConfig& source) = delete;
  EdenConfig& operator=(EdenConfig&& source) = delete;

  /**
   * Create an EdenConfig for testing usage
   */
  static std::shared_ptr<EdenConfig> createTestEdenConfig();

  /**
   * Return the config data as a EdenConfigData structure that can be
   * thrift-serialized.
   */
  EdenConfigData toThriftConfigData() const;

  /** Get the path to client certificate. */
  const std::optional<AbsolutePath> getClientCertificate() const;

  /**
   * Clear all configuration for the given config source.
   */
  void clearAll(ConfigSourceType);

  /**
   *  Register the configuration setting. The fullKey is used to parse values
   *  from the toml file. It is of the form: "core:userConfigPath"
   */
  void registerConfiguration(ConfigSettingBase* configSetting) override;

  /**
   * Returns the value in optional string for the given config key.
   * Throws if the config key is ill-formed.
   */
  std::optional<std::string> getValueByFullKey(
      std::string_view configKey) const;

  /**
   * Unconditionally apply all ConfigSources to the ConfigSettings.
   */
  void reload();

  /**
   * If any ConfigSources are stale, clones this EdenConfig and applies the
   * updated sources to the ConfigSettings.
   *
   * If no sources have changed, returns nullptr.
   */
  std::shared_ptr<const EdenConfig> maybeReload() const;

  /**
   * Returns the system config dir (--etc-eden-dir) path that EdenFS was passed
   * during startup.
   */
  const AbsolutePath& getSystemConfigDir() const {
    return systemConfigDir_;
  }

 private:
  /**
   * Utility method for converting ConfigSourceType to the filename (or cli).
   * @return the string value for the ConfigSourceType.
   */
  std::string toString(ConfigSourceType cs) const;

  /**
   * Returns a Thrift-suitable path corresponding to the given source's config
   * file.
   */
  std::string toSourcePath(ConfigSourceType cs) const;

  void parseAndApplyConfigFile(
      int configFd,
      AbsolutePathPiece configPath,
      ConfigSourceType configSourceType);

  /**
   * Mapping of section name : (map of attribute : config values). The
   * ConfigSetting constructor registration populates this map.
   */
  std::map<std::string, std::map<std::string, ConfigSettingBase*>> configMap_;

  std::shared_ptr<ConfigVariables> substitutions_;

  static constexpr size_t kConfigSourceLastIndex =
      static_cast<size_t>(apache::thrift::TEnumTraits<ConfigSourceType>::max());

  /**
   * Each ConfigSourceType has exactly zero or one ConfigSource. These slots are
   * iterated in order during reload to populate EdenConfig.
   */
  std::array<std::shared_ptr<ConfigSource>, kConfigSourceLastIndex + 1>
      configSources_;

  /**
   * The daemon sometimes needs to invoke the CLI. In doing so, it needs to pass
   * information about which system config dir (--etc-eden-dir) it's using.
   * Without this information, the CLI cannot properly load configs and will
   * default to using the default system config location.
   */
  const AbsolutePath systemConfigDir_;

  /*
   * Settings follow. Their initialization registers themselves with the
   * EdenConfig object. We make use of the registration to iterate over
   * ConfigSettings generically for parsing, copy, assignment and move
   * operations. We update the property value in the constructor (since we don't
   * have the home directory here).
   *
   * The following fields must come after configMap_.
   */

 public:
  // [core]

  ConfigSetting<AbsolutePath> edenDir{
      "core:edenDirectory",
      kUnspecifiedDefault,
      this};

  ConfigSetting<AbsolutePath> systemIgnoreFile{
      "core:systemIgnoreFile",
      kUnspecifiedDefault,
      this};

  ConfigSetting<AbsolutePath> userIgnoreFile{
      "core:ignoreFile",
      kUnspecifiedDefault,
      this};

  /**
   * How often to check the on-disk lock file to ensure it is still valid.
   * EdenFS will exit if the lock file is no longer valid.
   */
  ConfigSetting<std::chrono::nanoseconds> checkValidityInterval{
      "core:check-validity-interval",
      std::chrono::minutes(5),
      this};

  /**
   * If EdenFS should auto migrate FUSE repos to NFS on Ventura.
   * only used in the CLI, including here to get rid of warnings.
   */
  ConfigSetting<bool> migrateToNFSVentura{
      "core:migrate_existing_to_nfs",
      true,
      this};

  /**
   * If EdenFS should force a non-graceful restart, if necessary, to auto
   * migrate FUSE repos to NFS on all versions of macOS.  Only used in the CLI
   * and edenfs_restarter, including here to get rid of warnings.
   */
  ConfigSetting<bool> migrateToNFSAllMacOS{
      "core:migrate_existing_to_nfs_all_macos",
      true,
      this};

  /**
   * How many threads to use for the misc EdenCPUThreadPool.
   */
  ConfigSetting<uint8_t> edenCpuPoolNumThreads{
      "core:eden-cpu-pool-num-threads",
      12,
      this};

  /**
   * Whether graceful takeover client should purposefully return an exception
   * during the takeover process. Intended to be used only in integration tests.
   *
   * NOTE: if you want the takeover server to throw an exception, you can use
   * FaultInjector with the key/value pair: ("takeover", "error during send")
   */
  ConfigSetting<bool> clientThrowsDuringTakeover{
      "core:client-throws-during-takeover",
      false,
      this};

  /**
   * Takeover receive timeout in seconds. This is the time that the client will
   * wait for the server to send the takeover data. This timeout applies to each
   * chunk of data when sending data in chunks.
   */
  ConfigSetting<std::chrono::nanoseconds> takeoverReceiveTimeout{
      "core:takeover-receive-timeout",
      std::chrono::seconds(150),
      this};

  /**
   * Temporary config to control roll out of
   * TakeoverCapabilities::CHUNKED_MESSAGE protocol
   * Delete this config when rollout is 100% complete
   */
  ConfigSetting<bool> shouldChunkTakeoverData{
      "core:should-chunk-takeover-data",
      false,
      this};

  /**
   * If EdenFS should auto migrate non inmemory inode catalogs to inmemory on
   * Windows.
   */
  ConfigSetting<bool> migrateToInMemoryCatalog{
      "core:migrate_existing_to_in_memory_catalog",
      true,
      this};

  /**
   * At startup, EdenFS will attempt to set its memory priority to the following
   * value. If the value is null, EdenFS will not attempt to set its priority.
   *
   * On macOS, this corresponds to a Jetsam Priority value (as of June
   * 2025, these values range between 0 (lowest, most likely to be killed), and
   * 210 (highest, least likely to be killed).
   *
   * On Linux, this corresponds to the
   * oom_score_adj value (as of June 2025, these values range between -1000
   * (highest, least likely to be killed) and 1000 (most likely to be killed).
   */
  ConfigSetting<std::optional<int32_t>> daemonTargetMemoryPriority{
      "core:daemon-target-memory-priority",
      std::nullopt,
      this};

  /**
   * Similar to the above config, but sets the PrivHelper's priority instead.
   */
  ConfigSetting<std::optional<int32_t>> privHelperTargetMemoryPriority{
      "core:priv-helper-target-memory-priority",
      std::nullopt,
      this};

  /**
   * Timeout value for a clean shutdown on SIGTERM. If the timeout elapses, we
   * exit immediately. Set to zero to revert to the old behavior where we
   * unregister the signal handler unconditionally, causing the next signal to
   * instantly kill us.
   */
  ConfigSetting<std::chrono::nanoseconds> sigtermShutdownTimeout{
      "core:sigterm-shutdown-timeout",
      std::chrono::seconds(15),
      this};

  // [config]

  /**
   * How often the on-disk config information should be checked for changes.
   */
  ConfigSetting<std::chrono::nanoseconds> configReloadInterval{
      "config:reload-interval",
      std::chrono::minutes(5),
      this};

  // [thrift]

  ConfigSetting<bool> allowUnixGroupRequests{
      "thrift:allow-unix-group-requests",
      false,
      this};

  /**
   * Whether Eden should implement its own unix domain socket permission checks
   * or rely on filesystem permissions.
   */
  ConfigSetting<bool> thriftUseCustomPermissionChecking{
      "thrift:use-custom-permission-checking",
      true,
      this};

  /**
   * If Eden is using custom permission checking, the list of methods that any
   * user can call.
   */
  ConfigSetting<std::vector<std::string>> thriftFunctionsAllowlist{
      "thrift:functions-allowlist",
      std::vector<std::string>{
          "BaseService.getCounter",
          "BaseService.getCounters",
          "BaseService.getRegexCounters",
          "BaseService.getSelectedCounters"},
      this};

  /**
   * The number of Thrift worker threads.
   */
  ConfigSetting<size_t> thriftNumWorkers{
      "thrift:num-workers",
      std::thread::hardware_concurrency(),
      this};

  /**
   * Maximum number of active Thrift requests.
   */
  ConfigSetting<uint32_t> thriftMaxRequests{
      "thrift:max-requests",
      apache::thrift::concurrency::ThreadManager::DEFAULT_MAX_QUEUE_SIZE,
      this};

  /**
   * Request queue timeout (rounded down to the nearest millisecond).
   */
  ConfigSetting<std::chrono::nanoseconds> thriftQueueTimeout{
      "thrift:queue-timeout",
      std::chrono::seconds(30),
      this};

  /**
   * Use a SmallSerialExecutor for serial Thrift requests.
   */
  ConfigSetting<bool> thriftUseSmallSerialExecutor{
      "thrift:use-small-serial-executor",
      true,
      this};

  /**
   * Whether Eden should use resource pools
   */
  ConfigSetting<bool> thriftUseResourcePools{
      "thrift:use-resource-pools",
      false,
      this};

  /**
   * Whether Eden should use serial execution for each request. Resource pools
   * must be enabled for this to take effect
   */
  ConfigSetting<bool> thriftUseSerialExecution{
      "thrift:use-serial-execution",
      false,
      this};

  /**
   * Whether Eden should use a dedicated executor for checkout requests. This is
   * meant to help with checkoutRevision performance while using serial
   * execution for other Thrift requests. This feature can be used even if not
   * using serial execution for other Thrift requests, but if serial execution
   * is being used, its a good idea to turn on this config as well.
   */
  ConfigSetting<bool> thriftUseCheckoutExecutor{
      "thrift:use-checkout-executor",
      false,
      this};

  /**
   * Number of threads that will service the checkoutRevision Thrift endpoint
   * when using its own executor.
   */
  ConfigSetting<uint64_t> numCheckoutThreads{
      "thrift:checkout-revision-num-servicing-threads",
      std::thread::hardware_concurrency(),
      this};

  /**
   * How often to collect Thrift server metrics. The default value mirrors the
   * value from facebook::fb303::TServerCounters::kDefaultSampleRate
   */
  ConfigSetting<uint32_t> thriftServerObserverSamplingRate{
      "thrift:server-observer-sampling-rate",
      32,
      this};

  /**
   * How often to publish Thrift server metrics in milliseconds.
   */
  ConfigSetting<std::chrono::nanoseconds> thriftServerObserverPublishInterval{
      "thrift:server-observer-publish-interval",
      std::chrono::milliseconds(1000),
      this};

  /**
   * Whether the Thrift server should be configured to leak outstanding requests
   * when the server is stopped during shutdown/restart.
   */
  ConfigSetting<bool> thriftLeakOutstandingRequestsWhenServerStops{
      "thrift:leak-outstanding-requests-when-server-stops",
      false,
      this};

  /**
   * Configures the amount of time workers should spend completing outstanding
   * requests during Thrift server shutdown. If the timeout is reached, the
   * server exits immediately (i.e. crashes).
   *
   * Must be used with thriftLeakOutstandingRequestsWhenServerStops to have any
   * effect.
   */
  ConfigSetting<std::chrono::nanoseconds> thriftWorkersJoinTimeout{
      "thrift:workers-join-timeout",
      std::chrono::seconds(120),
      this};

  // [ssl]

  ConfigSetting<AbsolutePath> clientCertificate{
      "ssl:client-certificate",
      kUnspecifiedDefault,
      this};

  ConfigSetting<std::vector<std::string>> clientCertificateLocations{
      "ssl:client-certificate-locations",
      std::vector<std::string>{},
      this};

  ConfigSetting<bool> useMononoke{"mononoke:use-mononoke", false, this};

  // [mount]

  /**
   * After checkout completes with conflicts, how many conflicts should be
   * printed in the edenfs.log?
   */
  ConfigSetting<uint64_t> numConflictsToLog{
      "mount:num-conflicts-to-log",
      10,
      this};

  /**
   * How often will a garbage collection on the working copy will run.
   *
   * Default to every hour.
   */
  ConfigSetting<std::chrono::nanoseconds> gcPeriod{
      "mount:garbage-collection-period",
      std::chrono::hours(1),
      this};

  /**
   * If the number of inodes is greater than this threshold, the garbage
   * collection cutoff will be more aggressive.
   *
   * Set to zero to disable aggressive GC.
   *
   * See aggressiveGcCutoff for more details.
   */
  ConfigSetting<uint64_t> aggressiveGcThreshold{
      "mount:agrrssive-gc-threshold",
      0,
      this};

  /**
   * Inodes with a last used time (atime on Windows) older than cutoff will be
   * invalidated during GC.
   *
   * When total number of inodes is greater than a threshold, cutoff will be
   * more aggressive.
   *
   * See aggressiveGcThreshold for more details.
   */
  ConfigSetting<OneHourMinDuration> aggressiveGcCutoff{
      "mount:aggressive-gc-cutoff",
      OneHourMinDuration(std::chrono::hours(1)),
      this};

  /**
   * Inodes with a last used time (atime on Windows) older than cutoff will be
   * invalidated during GC.
   *
   * Default to a day. On Windows, the atime is  updated only once an hour, so
   * values below 1h are disallowed.
   */
  ConfigSetting<OneHourMinDuration> gcCutoff{
      "mount:garbage-collection-cutoff",
      OneHourMinDuration(std::chrono::hours(24)),
      this};

  /**
   * Specifies which directory children will be prefetched upon readdir.
   */
  ConfigSetting<ReaddirPrefetch> readdirPrefetch{
      "mount:readdir-prefetch",
      ReaddirPrefetch::None,
      this};

  /**
   * Specify the interval of periodic accidental unmount recovery.
   */
  ConfigSetting<std::chrono::nanoseconds> accidentalUnmountRecoveryInterval{
      "mount:accidental-unmount-recovery-interval",
      std::chrono::minutes(0),
      this};

  /**
   * The soong build system used in AOSP loves to crawls the entirety of the
   * repository, including the .eden directory. In doing so, it infinitely
   * recurse into the this-dir.
   *
   * Per kemurphy@, sending a pull request to soong will be refused, thus we
   * need to workaround it in EdenFS.
   *
   * DO NOT SET UNLESS YOU ARE RUNNING AOSP ON EDENFS.
   *
   * TODO(T147468271): Remove this once soong has been taught to recognize
   * EdenFS/Mercurial correctly.
   */
  ConfigSetting<bool> findIgnoreInDotEden{
      "mount:find-ignore-in-dot-eden",
      false,
      this};

  // [store]

  /**
   * How often to compute stats and perform garbage collection management
   * for the LocalStore.
   */
  ConfigSetting<std::chrono::nanoseconds> localStoreManagementInterval{
      "store:stats-interval",
      std::chrono::minutes(1),
      this};

  /*
   * The following settings control the maximum sizes of the local store's
   * caches, per object type.
   *
   * Automatic garbage collection will be triggered when the size exceeds the
   * thresholds.
   */

  ConfigSetting<uint64_t> localStoreBlobSizeLimit{
      "store:blob-size-limit",
      15'000'000'000,
      this};

  ConfigSetting<uint64_t> localStoreBlobMetaSizeLimit{
      "store:blobmeta-size-limit",
      1'000'000'000,
      this};

  ConfigSetting<uint64_t> localStoreTreeSizeLimit{
      "store:tree-size-limit",
      3'000'000'000,
      this};

  ConfigSetting<uint64_t> localStoreTreeAuxSizeLimit{
      "store:treeaux-size-limit",
      1'000'000'000,
      this};

  ConfigSetting<uint64_t> localStoreHgCommit2TreeSizeLimit{
      "store:hgcommit2tree-size-limit",
      20'000'000,
      this};

  /**
   * The minimum duration between logging occurrences of failed HgProxyHash
   * loads.
   */
  ConfigSetting<std::chrono::nanoseconds> missingHgProxyHashLogInterval{
      "store:missing-hgproxyhash-log-interval",
      std::chrono::minutes{10},
      this};

  /**
   * If the number of fetching requests of a process reaches this number,
   * a FetchHeavy event will be sent to Scuba.
   */
  ConfigSetting<uint32_t> fetchHeavyThreshold{
      "store:fetch-heavy-threshold",
      100000,
      this};

  /**
   * The maximum number of tree prefetch operations to allow in parallel for any
   * checkout.  Setting this to 0 will disable prefetch operations.
   */
  ConfigSetting<uint64_t> maxTreePrefetches{
      "store:max-tree-prefetches",
      5,
      this};

  /**
   * The maximum number blob SHA-1s and sizes to keep in memory per mount. See
   * the comment on `ObjectStore::metadataCache_` for more details.
   */
  ConfigSetting<uint64_t> metadataCacheSize{
      "store:metadata-cache-size",
      1'000'000,
      this};

  /**
   * Number of shards to use for the metadata cache.
   *
   * This is used to reduce lock contention on the metadata cache. Higher number
   * means lower contention, but more imperfect LRU property (each shard has its
   * own LRU).
   */
  ConfigSetting<uint64_t> metadataCacheShards{
      "store:metadata-cache-shards",
      32,
      this};

  /**
   * Controls if RocksDbLocalStore operations should run asynchronously or
   * synchronously.
   *
   * This is a temporary option to help us mitigate S433447.
   */
  ConfigSetting<bool> asyncRocksDbLocalStore{
      "store:async-rocksdb-local-store",
      false,
      this};

  /**
   * Controls the number of threads to use when processing RocksDbLocalStore
   * operations. At the time of writing, this is also used to drive the
   * RocksDbLocalStore's periodic GC.
   */
  ConfigSetting<uint8_t> rocksDbIoPoolNumThreads{
      "store:rocksdb-io-pool-num-threads",
      12,
      this};

  ConfigSetting<bool> warmTreeAuxCacheIfTreeFromLocalStore{
      "store:warm-aux-cache-tree-local-store",
      false,
      this};

  ConfigSetting<bool> warmTreeAuxLocalCacheIfTreeFromBackingStore{
      "store:warm-aux-local-cache-tree-backing-store",
      false,
      this};

  ConfigSetting<bool> warmTreeAuxMemCacheIfTreeFromBackingStore{
      "store:warm-aux-mem-cache-tree-backing-store",
      false,
      this};
  // [fuse]

  /**
   * The maximum number of concurrent background FUSE requests we allow the
   * kernel to send us. background should mean things like readahead prefetches
   * and direct I/O, but may include things that seem like more traditionally
   * foreground I/O. What counts as "background" seems to be up to the
   * discretion of the kernel.
   *
   * Linux FUSE defaults to 12, but EdenFS can handle a great deal of
   * concurrency.
   */
  ConfigSetting<int32_t> fuseMaximumBackgroundRequests{
      "fuse:max-concurrent-requests",
      1000,
      this};

  /**
   * The number of FUSE dispatcher threads to spawn.
   */
  ConfigSetting<int32_t> fuseNumDispatcherThreads{
      "fuse:num-dispatcher-threads",
      16,
      this};

  /**
   * The maximum time duration allowed for a fuse request. If a request exceeds
   * this amount of time, an ETIMEDOUT error will be returned to the kernel to
   * avoid blocking forever.
   */
  ConfigSetting<std::chrono::nanoseconds> fuseRequestTimeout{
      "fuse:request-timeout",
      std::chrono::minutes(1),
      this};

  /**
   * The maximum time duration that the kernel should allow for a fuse request.
   * If a request exceeds this amount of time, it may take aggressive
   * measures to shut down the fuse channel.
   * This value is only applicable to the macOS fuse implementation.
   */
  ConfigSetting<std::chrono::nanoseconds> fuseDaemonTimeout{
      "fuse:daemon-timeout",
      std::chrono::nanoseconds::max(),
      this};

  /**
   * The maximum number of concurrent requests allowed into userspace from the
   * kernel. This corresponds to fuse_init_out::max_background. The
   * documentation this applies to only readaheads and async direct IO, but
   * empirically we have observed the number of concurrent requests is limited
   * to 12 (FUSE_DEFAULT_MAX_BACKGROUND) unless this is set high.
   */
  ConfigSetting<uint32_t> maximumFuseRequests{"fuse:max-requests", 1000, this};

  /**
   * The string we use in the vfs type when mounting the fuse mount. Others will
   * see this in the mount table on the system.
   */
  ConfigSetting<std::string> fuseVfsType{"fuse:vfs-type", "fuse", this};

  // [nfs]

  /**
   * Controls whether Eden will run it's own rpcbind/portmapper server. On
   * Linux there is one built into the kernel that is always running, and on
   * mac there is one built into the kernel you just have to poke into running.
   * There is not one built into Windows and no good (and discoverable by
   * kmancini) external option to use. So we built our own.
   *
   * Rpcbind/portmapper runs on a fixed port (111), so two port mappers will
   * conflict with each other. Thus we don't want to run rpcbind if there is
   * already one running.
   *
   * Long story short, you never want to set this to true on Linux or mac,
   * but do on windows (with care).
   */
  ConfigSetting<bool> runInternalRpcbind{
      "nfs:run-internal-rpcbind",
      false,
      this};

  /**
   * Controls whether Mountd will register itself against rpcbind.
   */
  ConfigSetting<bool> registerMountd{"nfs:register-mountd", false, this};

  /**
   * Whether EdenFS should unload NFS inodes. NFSv3 does not notify us when
   * file handles are closed. We have no definitive info from NFS on how many
   * open handles there are for already removed inodes.
   *
   * This enables background inode unloads to keep our inode memory and
   * disk usage bounded.
   */
  ConfigSetting<bool> unloadUnlinkedInodes{
      "nfs:unload-unlinked-inodes",
      false,
      this};

  ConfigSetting<std::chrono::nanoseconds> postCheckoutDelayToUnloadInodes{
      "nfs:post-checkout-inode-unloading-delay",
      std::chrono::seconds{10},
      this};

  /**
   * On macOS, ._ (AppleDouble) are sprinkled all over the place. Enabling this
   * allows these file to be created. When disabled, the AppleDouble files
   * won't be created.
   */
  ConfigSetting<bool> allowAppleDouble{"nfs:allow-apple-double", true, this};

  /**
   * ============== NFS MOUNT OPTIONS ==============
   *
   * See `man mount_nfs` for more information on these options.
   *
   * https://www.unix.com/man-page/osx/8/mount_nfs/
   */

  /**
   * The maximum time duration allowed for a NFS request. If a request exceeds
   * this amount of time, an NFS3ERR_JUKEBOX error will be returned to the
   * client to avoid blocking forever. NOTE: This is currently unimplemented.
   */
  ConfigSetting<std::chrono::nanoseconds> nfsRequestTimeout{
      "nfs:request-timeout",
      std::chrono::minutes(1),
      this};

  /**
   * ========= DEPRECATED: DO NOT USE =========
   *
   * Buffer size for read and write requests. Default to 16 KiB.
   *
   * 16KiB was determined to offer the best tradeoff of random write speed to
   * streaming writes on macOS, use the benchmarks/random_writes.cpp before
   * changing this default value.
   */
  ConfigSetting<uint32_t> nfsIoSize{"nfs:iosize", 16 * 1024, this};

  /**
   * Buffer size for read requests. Default to 16 KiB.
   *
   * 16KiB was determined to offer the best tradeoff of random write speed to
   * streaming writes on macOS, use the benchmarks/random_writes.cpp before
   * changing this default value.
   */
  ConfigSetting<uint32_t> nfsReadIoSize{"nfs:read-iosize", 16 * 1024, this};

  /**
   * Buffer size for write requests. Default to 16 KiB.
   *
   * 16KiB was determined to offer the best tradeoff of random write speed to
   * streaming writes on macOS, use the benchmarks/random_writes.cpp before
   * changing this default value.
   */
  ConfigSetting<uint32_t> nfsWriteIoSize{"nfs:write-iosize", 16 * 1024, this};

  /**
   * Whether EdenFS NFS sockets should bind themself to unix sockets instead of
   * TCP ones.
   *
   * Unix sockets bypass the overhead of TCP and are thus significantly faster.
   * This is only supported on macOS.
   *
   * Note: Using UDS for binding is currently believed to be buggy. Reads would
   * randomly fail with some error due to a bug in the kernel not retrying
   * some internal error (like buffer being too small).
   */
  ConfigSetting<bool> useUnixSocket{"nfs:use-uds", false, this};

  /**
   * ========== MACOS ONLY ==========
   *
   * Set the directory read size to the specified value. The value should
   * normally be a multiple of DIRBLKSIZ that is <= the read size for the mount.
   * The default is 8192 for UDP mounts and 32768 for TCP mounts.
   */
  ConfigSetting<std::optional<uint32_t>> nfsDirectoryReadSize{
      "nfs:dir-read-size",
      std::nullopt,
      this};

  /**
   * ========== MACOS ONLY ==========
   *
   * Set the maximum read-ahead count to the specified value. This may be in the
   * range of 0 - 128, and determines how many Read RPCs will be read ahead when
   * a large file is being read sequentially. Trying larger values for this is
   * suggested for mounts with a large bandwidth * delay product.
   */
  ConfigSetting<uint8_t> nfsReadAhead{"nfs:read-ahead", 16, this};

  /**
   * NOTE: This config currently is limited to multiples of 10 deciseconds due
   * to a bug in the EdenFS mount implementation.
   *
   * Set the initial retransmit timeout to the specified value. (Normally, the
   * dumbtimer option should be specified when using this option to manually
   * tune the timeout interval). The value is in tenths of a second.
   */
  ConfigSetting<int32_t> nfsRetransmitTimeoutTenthSeconds{
      "nfs:retransmit-timeout-tenths",
      10,
      this};

  /**
   * Set the retransmit timeout count for soft mounts to the specified value.
   */
  ConfigSetting<uint32_t> nfsRetransmitAttempts{
      "nfs:retransmit-attempts",
      10,
      this};

  /**
   * ========== MACOS ONLY ==========
   *
   * If the mount is still unresponsive X seconds after it is initially
   * reported unresponsive, then mark the mount as dead so that it will be
   * forcibly unmounted.  Note: mounts which are both soft and read-only will
   * also have the deadtimeout mount option set to 60 seconds.  This can be
   * explicitly overridden by setting deadtimeout=0.
   */
  ConfigSetting<int32_t> nfsDeadTimeoutSeconds{
      "nfs:dead-timeout-seconds",
      0,
      this};

  /**
   * Turn off the dynamic retransmit timeout estimator.  This may be useful for
   * UDP mounts that exhibit high retry rates, since it is possible that the
   * dynamically estimated timeout interval is too short.
   */
  ConfigSetting<std::optional<bool>> nfsDumbtimer{
      "nfs:dumbtimer",
      std::nullopt,
      this};

  /**
   * Whether we should validate that files on disk match their inode state after
   * checkout. We won't validate all of the loaded files or even the ones
   * changed by checkout, but just a handful of the files that were loaded and
   * changed by checkout. The next few configs control how many files and how
   * we select them.
   TODO: This is to collect data for S439820. We can remove this once SEV
   closed.
   */
  ConfigSetting<bool> verifyFilesAfterCheckout{
      "nfs:verify-files-after-checkout",
      false,
      this};

  /**
   * We aim to invalidate maxNumberOfInvlidationsToVerify on every checkout
   * operation. If there are less than maxNumberOfInvlidationsToVerify files
   * invalidated by a checkout operation then we might verify less. But most
   * operations should verify this many files.
   TODO: This is to collect data for S439820. We can remove this once SEV
   closed.
   */
  ConfigSetting<size_t> maxNumberOfInvlidationsToVerify{
      "nfs:max-number-invalidations-to-verify",
      10,
      this};

  /**
   * Controls how we sample which files to verify after checkout.
   *
   * If there are less than `maxNumberOfInvlidationsToVerify` files invalidated
   * by checkout then we verify all of them, otherwise we will verify every nth
   * file that was invalidated.
   * If there are more than the max, we will try to validate files that were
   * invalidated the latest. This gives us the best chance of catching the file
   * content being in the wrong state.
   TODO: This is to collect data for S439820. We can remove this once SEV
   closed.
   */
  ConfigSetting<size_t> verifyEveryNInvalidations{
      "nfs:verify-every-n-invalidations",
      100,
      this};

  /**
   * Number of threads used to run validation after checkout. We should only
   * be verifying a small number of files, so it's fine for this to be a small
   * number of files. It's very important that this thread is not on the path of
   * any fuse requests because it will do calls into the filesystem.
   * we do not want to cause a deadlock.
   TODO: This is to collect data for S439820. We can remove this once SEV
   closed.
   */
  ConfigSetting<size_t> numVerifierThreads{
      "nfs:number-verifier-threads",
      1,
      this};

  /**
   * We only verify invalidation for files that are smaller than this size
   * to avoid reading large files into memory.
   TODO: This is to collect data for S439820. We can remove this once SEV
   closed.
   */
  ConfigSetting<size_t> maxSizeOfFileToVerifyInvalidation{
      "nfs:max-size-of-file-to-verify-invalidation",
      100 * 1024 * 1024, // 100MB
      this};

  /**
   * When set to true, we will use readdirplus instead of readdir. Readdirplus
   * will be enabled for all nfs mounts. If set to false, regular readdir is
   * used instead.
   */
  ConfigSetting<bool> useReaddirplus{"nfs:use-readdirplus", false, this};

  /**
   * When set to true, NFS mounts are mounted with the "soft" mount option. This
   * setting applies to all NFS mounts. Behavior when set to false differs
   * between platforms:
   *
   * - macOS: Hard mount with INTR mount option is used.
   * - Linux: Hard mount is used (no INTR). Intr is unsupported after Linux
   *          kernel version 2.6.25.
   *
   * Note: setting to "true" does not turn off the "INTR" option on macOS.
   */
  ConfigSetting<bool> useSoftMounts{
      "nfs:use-soft-mounts",
      folly::kIsLinux ? true : false,
      this};

  // [prjfs]

  /**
   * The maximum time duration allowed for a ProjectedFS callback. If a request
   * exceeds this amount of time, the request will fail to avoid blocking
   * forever. A notification will also be shown to alert the user of the
   * timeout.
   */
  ConfigSetting<std::chrono::nanoseconds> prjfsRequestTimeout{
      "prjfs:request-timeout",
      std::chrono::minutes(1),
      this};

  /**
   * Enable ProjectedFS's negative path caching to reduce the number of
   * requests non existent files.
   * Only applicable on Windows
   */
  ConfigSetting<bool> prjfsUseNegativePathCaching{
      "prjfs:use-negative-path-caching",
      true,
      this};

  /**
   * Controls the number of threads per mount dedicated to running directory
   * invalidation.
   */
  ConfigSetting<uint8_t> prjfsNumInvalidationThreads{
      "prjfs:num-invalidation-threads",
      1,
      this};

  /**
   * Not sure if a Windows behavior, or a ProjectedFS one, but symlinks
   * aren't created atomically, they start their life as a directory, and
   * then a reparse tag is added to them to change them to a symlink. This
   * is an issue for EdenFS as the call to symlink_status above will race
   * with this 2 step process and thus may detect a symlink as a
   * directory...
   *
   * This is bad for EdenFS for a number of reason. The main one being that
   * EdenFS will attempt to recursively add all the childrens of that
   * directory to the inode hierarchy. If the symlinks points to a very
   * large directory, this can be extremely slow, leading to a very poor
   * user experience.
   *
   * How to solve this? Since notifications are handled completely
   * asynchronously, we can simply wait a bit and retry if the notification
   * has been received semi-recently. We still run the
   * risk of winning the race if the system is overloaded, but the
   * probability should be much lower.
   *
   * This config controls how long to wait after a directory creation
   * notification has been received.
   */
  ConfigSetting<std::chrono::nanoseconds> prjfsDirectoryCreationDelay{
      "prjfs:directory-creation-delay",
      std::chrono::milliseconds(100),
      this};

  /**
   * Listen to pre convert to full notifications from ProjFS. By the spec these
   * should not give us any information that the other notifications all ready
   * cover. However, ProjFS currently (Feb 2023) has a bug: we do not receive
   * the file closed and modified notification. We can listen to this instead
   * to ensure our in memory state reflects file truncations.
   */
  ConfigSetting<bool> prjfsListenToPreConvertToFull{
      "prjfs:listen-to-pre-convert-to-full",
      false,
      this};

  /**
   * With out this FSCK will not attempt to "fix" renamed files. This can
   * leave EdenFS out of sync with the filesystem. However this make FSCK
   * slower.
   */
  ConfigSetting<bool> prjfsFsckDetectRenames{
      "prjfs:fsck-detect-renames",
      true,
      this};

  /**
   * Controls how frequently we log to the EdenFS log file and scuba tables
   * about torn reads - i.e. when Prjfs attempts to read a file that was
   * modified in the middle of an operation.
   */
  ConfigSetting<std::chrono::nanoseconds> prjfsTornReadLogInterval{
      "prjfs:torn-read-log-interval",
      std::chrono::seconds{10},
      this};

  ConfigSetting<std::chrono::nanoseconds> tornReadCleanupDelay{
      "prjfs:torn-read-cleanup-delay",
      std::chrono::seconds{1},
      this};

  // [fschannel]

  /**
   * Number of threads that will service the background FS channel requests.
   */
  ConfigSetting<uint64_t> numFsChannelThreads{
      "fschannel:num-servicing-threads",
      std::thread::hardware_concurrency(),
      this};

  /**
   * Maximum number of pending FSChannel requests. This is currently only
   * enforced in the FUSE FSChannel implementation. This value is also used as
   * the threshold when determining when to log high number of pending requests.
   * Logging is currently is only enabled in FUSE and NFS FSChannel
   * implementations. When set to 0, no limit is enforced and no logging will
   * occur.
   */
  ConfigSetting<uint64_t> maxFsChannelInflightRequests{
      "fschannel:max-inflight-requests",
      0,
      this};

  ConfigSetting<std::chrono::nanoseconds> highFsRequestsLogInterval{
      "fschannel:high-fs-requests-log-interval",
      std::chrono::minutes{30},
      this};

  // [hg]

  /**
   * Controls whether Eden enforces parent commits in a hg status
   * (getScmStatusV2) call
   */
  ConfigSetting<bool> enforceParents{"hg:enforce-parents", true, this};

  /**
   * Controls whether EdenFS reads blob metadata directly from hg
   *
   * TODO: Delete once this config is no longer written.
   */
  ConfigSetting<bool> useAuxMetadata{"hg:use-aux-metadata", true, this};

  /**
   * Controls whether EdenFS will attempt to fetch aux metadata or always
   * fallback to fetching blobs when the metadata is not present in local
   * caches.
   */
  ConfigSetting<bool> fetchHgAuxMetadata{"hg:fetch-aux-metadata", true, this};

  /**
   * Which object ID format should the SaplingBackingStore use?
   */
  ConfigSetting<HgObjectIdFormat> hgObjectIdFormat{
      "hg:object-id-format",
      HgObjectIdFormat::WithPath,
      this};

  /**
   * In general, hg does not guarantee that blob IDs and contents are 1:1 [1].
   * EdenFS can be slightly faster if they are, so this switch exists for the
   * future possibility that they are, or if someone wants to live dangerously
   * for the sake of performance.
   *
   * [1] hg blob IDs sometimes include file history.
   */
  ConfigSetting<bool> hgBijectiveBlobIds{
      "hg:has-bijective-blob-ids",
      false,
      this};

  /**
   * Controls the number of blob or prefetch import requests we batch in
   * SaplingBackingStore
   */
  ConfigSetting<uint32_t> importBatchSize{"hg:import-batch-size", 1, this};

  /**
   * Controls the number of tree import requests we batch in SaplingBackingStore
   */
  ConfigSetting<uint32_t> importBatchSizeTree{
      "hg:import-batch-size-tree",
      1,
      this};

  /**
   * Controls the max number of blob aux data import requests we batch in
   * SaplingBackingStore
   */
  ConfigSetting<uint32_t> importBatchSizeBlobMeta{
      "hg:import-batch-size-blobmeta",
      1024,
      this};

  /**
   * Controls the max number of tree aux data import requests we batch in
   * SaplingBackingStore
   */
  ConfigSetting<uint32_t> importBatchSizeTreeMeta{
      "hg:import-batch-size-treemeta",
      1024,
      this};

  ConfigSetting<uint32_t> hgActivityBufferSize{
      "hg:activity-buffer-size",
      100,
      this};

  ConfigSetting<bool> hgForceDisableLocalStoreCaching{
      "hg:force-disable-localstore",
      false,
      this};

  ConfigSetting<bool> hgEnableBlobMetaLocalStoreCaching{
      "hg:cache-blob-metadata-in-localstore",
      true,
      this};

  ConfigSetting<bool> hgEnableTreeMetaLocalStoreCaching{
      "hg:cache-tree-metadata-in-localstore",
      true,
      this};

  ConfigSetting<bool> hgEnableTreeLocalStoreCaching{
      "hg:cache-trees-in-localstore",
      true,
      this};

  ConfigSetting<bool> hgEnableBlobLocalStoreCaching{
      "hg:cache-blobs-in-localstore",
      false,
      this};

  /**
   * Should we use the cached `sl status` results or not
   */
  ConfigSetting<bool> hgEnableCachedResultForStatusRequest{
      "hg:enable-scm-status-cache",
      false,
      this};

  /**
   *  The maximum size of SCM status cache in bytes:
   *  1. Generally, a file path is about 50 chars long.
   *  2. We only cache status when the number of diff files is less than 10k.
   *  3. And we allow at most 5 such "huge" status
   */
  ConfigSetting<size_t> scmStatusCacheMaxSize{
      "hg:scm-status-cache-max-size",
      50 * 10 * 1024 * 5, // 2.5 MB
      this};

  /**
   *  The minimum number of items to keep in SCM status cache
   */
  ConfigSetting<size_t> scmStatusCacheMinimumItems{
      "hg:scm-status-cache-min-items",
      3,
      this};

  /**
   *  The maximum number of entries we want to cache within a single status
   */
  ConfigSetting<size_t> scmStatusCacheMaxEntriesPerItem{
      // @lint-ignore SPELL
      "hg:scm-status-cache-max-entires-per-item",
      10000,
      this};

  // [backingstore]

  /**
   * Number of threads that will pull backingstore requests off the queue.
   */
  ConfigSetting<uint8_t> numBackingstoreThreads{
      "backingstore:num-servicing-threads",
      32,
      this};

  // [telemetry]

  /**
   * Location of scribe_cat binary on the system. If not specified, scribe
   * logging will be disabled.
   */
  ConfigSetting<std::string> scribeLogger{"telemetry:scribe-cat", "", this};

  /**
   * Scribe category is the first argument passed to the scribe_cat binary.
   */
  ConfigSetting<std::string> scribeCategory{
      "telemetry:scribe-category",
      "",
      this};

  /**
   * Scribe category is the first argument passed to the scribe_cat binary. This
   * is used by the FileAccessStructuredLogger
   */
  ConfigSetting<std::string> fileAccessScribeCategory{
      "telemetry:file-access-scribe-category",
      "",
      this};

  /**
   * Scribe category is the first argument passed to the scribe_cat binary. This
   * is used by the NotificationsStructuredLogger
   */
  ConfigSetting<std::string> notificationsScribeCategory{
      "telemetry:notifications-scribe-category",
      "",
      this};

  /**
   * Controls which paths eden will log data fetches for when this is set.
   * Fetches for any paths that match the regex will be logged.
   */
  ConfigSetting<std::optional<std::shared_ptr<RE2>>> logObjectFetchPathRegex{
      "telemetry:log-object-fetch-path-regex",
      std::nullopt,
      this};

  /**
   * Controls sample denominator for each request sampling group.
   * We assign request types into sampling groups based on their usage and
   * set a sample denominator for each sampling group so that we have the
   * flexibility of up/down-sampling different requests but also avoid having to
   * set a sampling rate for each of the dozens of request types. For example,
   * `mkdir` and `rmdir` can be assigned to a sampling group that have a high
   * sampling rate while `getattr` and `getxattr` to another sampling group with
   * low sampling rate as they happen very frequently.
   *
   * Sampling rates are calculated from sampling denominators. A denominator of
   * 0 indicates dropping all requests in the group. Group 0's value is ignored
   * as it's always considered as having denominator of 0. A positive
   * denominator means that the requests in the group are sampled at 1/x (so
   * denominator of 1 drops no events).
   *
   * We use sampling group as indexes into this vector to look
   * up their denominators. Thus, the size of this vector should match the
   * number of sampling groups defined by the enum `SamplingGroup`. If the
   * vector has fewer elements than the number of sampling groups, look-ups will
   * fail for the higher sampling groups and we will consider them having
   * denominator of 0. For example, if the vector has size of 3, all requests of
   * sampling group 4 will be dropped.
   * Keeping this vector in ascending order is recommended but not required.
   * e.g. {0, 10, 100, 1000, 10000}
   */
  ConfigSetting<std::vector<uint32_t>> requestSamplingGroupDenominators{
      "telemetry:request-sampling-group-denominators",
      std::vector<uint32_t>{0, 0, 0, 0, 0},
      this};

  /**
   * Controls the max number of requests per minute per mount that can be sent
   * for logging.
   * A request is first sampled based on its sampling group denominators. Then
   * if we have not reached this cap, the request is sent for logging.
   */
  ConfigSetting<uint32_t> requestSamplesPerMinute{
      "telemetry:request-samples-per-minute",
      0,
      this};

  /**
   * Minimum interval between NFS stats updates in seconds. NFS stat collection
   * only happens on macOS. Change this to 0 to disable NFS stat collection.
   */
  ConfigSetting<std::chrono::nanoseconds> updateNFSStatsInterval{
      "telemetry:update-nfs-stats-interval",
      std::chrono::seconds{0},
      this};

  /**
   * OBC API is used for a few counters on EdenFS to make them enable on
   * sandcastle. For now, OBC API is only works on prod/Linux. Then this config
   * should only be set to true on prod/Linux.
   * TODO: change this config when EngEnv team enable OBC API on macOS and
   * Windows.
   */
  ConfigSetting<bool> enableOBCOnEden{
      "telemetry:enable-obc-on-eden",
      false,
      this};

  /**
   * Controls which configs we want to send with the request logging.
   * The elements are full config keys, e.g. "hg:import-batch-size".
   * Elements not valid or not present in the config map are silently ignored.
   * This is only intended for facilitating A/B testing and should be empty if
   * there is no active experiment.
   */
  ConfigSetting<std::vector<std::string>> requestSamplingConfigAllowlist{
      "telemetry:request-sampling-config-allowlist",
      std::vector<std::string>{},
      this};

  /**
   * Controls the capacity of the internal buffer for NFS Tracebus.
   */
  ConfigSetting<size_t> nfsTraceBusCapacity{
      "telemetry:nfs-tracebus-capacity",
      25000,
      this};

  ConfigSetting<size_t> HgTraceBusCapacity{
      "telemetry:hg-tracebus-capacity",
      100000,
      this};

  ConfigSetting<size_t> InodeTraceBusCapacity{
      "telemetry:inode-tracebus-capacity",
      25000,
      this};

  ConfigSetting<size_t> ThriftTraceBusCapacity{
      "telemetry:thrift-tracebus-capacity",
      25000,
      this};

  ConfigSetting<size_t> FuseTraceBusCapacity{
      "telemetry:fuse-tracebus-capacity",
      25000,
      this};

  ConfigSetting<size_t> PrjfsTraceBusCapacity{
      "telemetry:prjfs-tracebus-capacity",
      25000,
      this};

  /**
   * Controls whether EdenFS logs inode state changes to Tracebus or not.
   */
  ConfigSetting<bool> enableInodeTraceBus{
      "telemetry:enable-inodetracebus",
      true,
      this};

  /**
   * Controls whether EdenFS makes use of ActivityBuffers to store past
   * events in memory.
   */
  ConfigSetting<bool> enableActivityBuffer{
      "telemetry:enable-activitybuffer",
      true,
      this};

  /**
   * Sets the maximum number of events an ActivityBuffer can store before
   * evicting old events
   */
  ConfigSetting<uint32_t> activityBufferMaxEvents{
      "telemetry:activitybuffer-max-events",
      100,
      this};

  // TODO: Understand why long running requests are hit so frequently on Windows
  // hosts. For now, disable the config on Windows because too many Scuba
  // samples are generated.
  ConfigSetting<std::chrono::nanoseconds> longRunningFSRequestThreshold{
      "telemetry:long-running-fs-request-threshold",
      folly::kIsWindows ? std::chrono::nanoseconds(0)
                        : std::chrono::seconds{45},
      this};

  // [experimental]

  /**
   * Controls whether EdenFS detects processes that crawl an NFS checkout. Only
   * affects EdenFS if experimental:enable-nfs-server is also true. When NFS
   * crawl detection is enabled, EdenFS will perioically check
   * (experimental:nfs-crawl-interval) whether NFS read or readdir counters
   * exceed the configured thresholds (experimental:nfs-crawl-read-threshold and
   * experimental:nfs-crawl-readdir-threshold, respectively). If they do exceed,
   * EdenFS will determine which processes appear to be performing the crawl and
   * record their information to the log file and structured logging. Currently,
   * this information is expected to be useful in diagnosing slowness when using
   * NFS. Future, work in this area may provided more immediate feedback to
   * users initiating these processes.
   */
  ConfigSetting<bool> enableNfsCrawlDetection{
      "experimental:enable-nfs-crawl-detection",
      false,
      this};

  /**
   * Sets the interval at which EdenFS detects NFS crawling.
   */
  ConfigSetting<std::chrono::nanoseconds> nfsCrawlInterval{
      "experimental:nfs-crawl-interval",
      std::chrono::minutes(1),
      this};

  /**
   * Sets the read threshold at which EdenFS determines NFS crawling is
   * occurring.
   */
  ConfigSetting<uint32_t> nfsCrawlReadThreshold{
      "experimental:nfs-crawl-read-threshold",
      1000,
      this};

  /**
   * Sets the readdir threshold at which EdenFS determines NFS crawling is
   * occurring.
   */
  ConfigSetting<uint32_t> nfsCrawlReadDirThreshold{
      "experimental:nfs-crawl-readdir-threshold",
      250,
      this};

  /**
   * Sets the process name exclusions NFS crawling to ignore.
   */
  ConfigSetting<std::unordered_set<std::string>> nfsCrawlExcludedProcessNames{
      "experimental:nfs-crawl-excluded-process-names",
      {},
      this};

  /**
   * Controls whether EdenFS exports itself as an NFS server.
   */
  ConfigSetting<bool> enableNfsServer{
      "experimental:enable-nfs-server",
      folly::kIsApple,
      this};

  /**
   * Specify the interval of updating eden heartbeat file. The heartbeat file
   * should have the latest timestamp. If eden crash with a SIGKILL, the
   * timestamp of the heartbeat file can be used to determine approximate time
   * of the crash.
   * Zero means disable the updating feature.
   */
  ConfigSetting<std::chrono::nanoseconds> updateEdenHeartbeatFileInterval{
      "experimental:update-eden-heartbeat-file-interval",
      std::chrono::minutes(15),
      this};

  /**
   * Controls whether EdenFS will periodically garbage collect the working
   * directory.
   *
   * For now, periodic GC only makes sense on Windows. On macOS, it is
   * currently in a limited dogfooding phase and its behavior is not
   * yet fully defined. Behavior on Linux is unknown.
   */
  ConfigSetting<bool> enableGc{
      "experimental:enable-garbage-collection",
      folly::kIsWindows,
      this};

  /**
   * The interval for background periodic unloading of inodes from inodeMap.
   * 0 means periodic unloading is disabled.
   *
   * Note 1: Periodic inode unloading and Garbage Collection (GC) are mutually
   * exclusive. If GC is enabled, periodic unloading should be disabled or
   * vice versa.
   * Note 2: Periodic inode unloading is no-op on Windows.
   */
  ConfigSetting<uint32_t> periodicUnloadIntervalMinutes{
      "experimental:periodic-inode-map-unload-interval",
      0,
      this};

  /**
   * Controls whether EdenFS eagerly invalidates directories during checkout or
   * whether it only does when children were modified.
   *
   * On Windows, this must be set to true for correctness reasons.
   */
  ConfigSetting<bool> alwaysInvalidateDirectory{
      "experimental:always-invalidate-directories",
      folly::kIsWindows,
      this};

  /**
   * Controls whether EdenFS symlinks are enabled on Windows.
   *
   * Currently this is disabled because of a Windows bug. Directories with
   * long symlinks become un-list-able.
   * https://fb.workplace.com/groups/edenfswindows/permalink/1427359391513268/
   */
  ConfigSetting<bool> windowsSymlinksEnabled{
      "experimental:windows-symlinks",
      false,
      this};

  /**
   * Controls whether EdenFS propagates errors during the core checkout
   * operation. The old behavior was to "propagate" as conflict errors (which
   * Sapling might ignore). The new behavior is to propagate errors as top-level
   * command errors.
   *
   * Once the new behavior is validated, this flag (and the old code) should be
   * removed.
   */
  ConfigSetting<bool> propagateCheckoutErrors{
      "experimental:propagate-checkout-errors",
      false,
      this};

  /**
   * Controls whether we optimize blob prefetching with the Sapling
   * IGNORE_RESULT flag, which reduces work by not propagating the actual
   * blob result.
   *
   * This is an escape hatch in case something goes wrong.
   */
  ConfigSetting<bool> ignorePrefetchResult{
      "experimental:ignore-prefetch-result",
      true,
      this};

  /**
   * Controls whether eden rm command attempts to clean up mount directory
   * recursively. eden rm currently assumes nothing exist after unmounting and
   * directly removes the directory, which leads to ENOTEMPTY for lots of users.
   *
   * This will be rolled out once we have a good understanding of what exist
   * behind our mount.
   */
  ConfigSetting<bool> removeMountRecursively{
      "experimental:remove-mount-recursively",
      false,
      this};

  // [blobcache]

  /**
   * Controls whether if EdenFS caches blobs in memory.
   */
  ConfigSetting<bool> enableInMemoryBlobCaching{
      "blobcache:enable-in-memory-blob-caching",
      true,
      this};

  /**
   * How many bytes worth of blobs to keep in memory, at most.
   */
  ConfigSetting<size_t> inMemoryBlobCacheSize{
      "blobcache:cache-size",
      40 * 1024 * 1024,
      this};

  /**
   * The minimum number of recent blobs to keep cached. Trumps
   * maximumBlobCacheSize.
   */
  ConfigSetting<size_t> inMemoryBlobCacheMinimumItems{
      "blobcache:minimum-items",
      16,
      this};

  // [treecache]

  /**
   * Controls whether if EdenFS caches trees in memory.
   */
  ConfigSetting<bool> enableInMemoryTreeCaching{
      "treecache:enable-in-memory-tree-caching",
      true,
      this};

  /**
   * Number of bytes worth of data to keep in memory.
   */
  ConfigSetting<size_t> inMemoryTreeCacheSize{
      "treecache:cache-size",
      40 * 1024 * 1024,
      this};

  /**
   * The minimum number of recent tree to keep cached. Trumps
   * inMemoryTreeCacheSize.
   */
  ConfigSetting<size_t> inMemoryTreeCacheMinimumItems{
      "treecache:minimum-items",
      16,
      this};

  // [notifications]

  /**
   * A command to run to warn the user of a generic problem encountered
   * while trying to process a request.
   * The command is executed by the shell.
   * If blank, no command will be run.
   */
  ConfigSetting<std::string> genericErrorNotificationCommand{
      "notifications:generic-connectivity-notification-cmd",
      "",
      this};

  /**
   * Don't show a notification more often than once in the specified interval
   */
  ConfigSetting<std::chrono::nanoseconds> notificationInterval{
      "notifications:interval",
      std::chrono::minutes(1),
      this};

  /**
   * Whether the E-Menu should be created when the EdenFS daemon is started
   */
  ConfigSetting<bool> enableEdenMenu{
      "notifications:enable-eden-menu",
      true,
      this};

  /**
   * Whether notifications are completely disabled
   */
  ConfigSetting<bool> enableNotifications{
      "notifications:enable-notifications",
      true,
      this};

  /**
   * Whether the debug menu is shown in the Eden Menu
   */
  ConfigSetting<bool> enableEdenDebugMenu{
      "notifications:enable-debug",
      true,
      this};

  /**
   * Whether health report notifications should be shown via Windows
   * notifications. This is only used in the CLI, it is included here to
   * get rid of warnings.
   */
  ConfigSetting<bool> notifyHealthReportIssues{
      "notifications:notify-health-report-issues",
      false,
      this};

  /**
   * The age threshold that the health-report command should utilize to check if
   * the running EdenFS version is stale.
   */
  ConfigSetting<size_t> healthReportStaleVersionThresholdDays{
      "notifications:health-report-stale-version-threshold-days",
      45,
      this};

  /**
   * Whether EdenFS ready status should be shown via Windows
   * notifications
   */
  ConfigSetting<bool> notifyEdenReady{
      "notifications:notify-eden-ready",
      false,
      this};

  // [log]

  ConfigSetting<uint64_t> maxLogFileSize{"log:max-file-size", 50000000, this};
  ConfigSetting<uint64_t> maxRotatedLogFiles{"log:num-rotated-logs", 3, this};

  // [prefetch-profiles]

  /**
   * Kill switch for the prefetch profiles feature.
   */
  ConfigSetting<bool> enablePrefetchProfiles{
      "prefetch-profiles:prefetching-enabled",
      true,
      this};

  /**
   * Kill switch for predictive prefetch profiles feature.
   */
  ConfigSetting<bool> enablePredictivePrefetchProfiles{
      "prefetch-profiles:predictive-prefetching-enabled",
      false,
      this};

  /**
   * Used to control file access logging for predictive prefetch
   * profiles.
   */
  ConfigSetting<bool> logFileAccesses{
      "prefetch-profiles:file-access-logging-enabled",
      false,
      this};

  /**
   * A number from 0 - x to determine how often we should log file access
   * events. This is currently agnostic to the type of file access. If this
   * is not at 100%, we will not log filenames and we will only log directory
   * paths. In the following equation, 1/x = percentage, x is this variable.
   * For 50% rollout, 1/x = .5, so x = 2, so this would be set to 2. 0
   * indicates that the feature is off.
   */
  ConfigSetting<uint32_t> logFileAccessesSamplingDenominator{
      "prefetch-profiles:file-access-logging-sampling-denominator",
      0,
      this};

  // [predictive-prefetch-profiles]

  /**
   * The number of globs to use for a predictive prefetch profile,
   * 1500 by default.
   */
  ConfigSetting<uint32_t> predictivePrefetchProfileSize{
      "predictive-prefetch-profiles:size",
      1500,
      this};

  // [redirections]

  /**
   * Whether to use symlinks, APFS volumes, or disk images for bind
   * redirections on macOS.
   */
  ConfigSetting<std::string> darwinRedirectionType{
      "redirections:darwin-redirection-type",
      "apfs",
      this};

  // [overlay]

  /**
   * The `InodeCatalogType` to use when creating new `Overlay`s, unless
   * specified by parameter during creation via CLI or otherwise.
   */
  ConfigSetting<InodeCatalogType> inodeCatalogType{
      "overlay:inode-catalog-type",
      kInodeCatalogTypeDefault,
      this};

  /**
   * DANGER: this option will put overlay into memory and skip persisting any
   * actual data to disk. This will guarantee to cause EdenFS corruption after
   * restart. Use with caution.
   */
  ConfigSetting<bool> unsafeInMemoryOverlay{
      "overlay:unsafe-in-memory-overlay",
      false,
      this};

  /**
   * The synchronous mode used when using a SQLite backed overlay. Currently it
   * only supports "off" or "normal". Setting this to off may cause data loss.
   */
  ConfigSetting<std::string> overlaySynchronousMode{
      "overlay:synchronous-mode",
      folly::kIsWindows ? "off" : "normal",
      this};

  /**
   * This option controls how often we run SQLite WAL checkpoint in a SQLite
   * backed overlay. This option is ignored in other overlay types.
   */
  ConfigSetting<std::chrono::nanoseconds> overlayMaintenanceInterval{
      "overlay:maintenance-interval",
      std::chrono::minutes(1),
      this};

  /**
   * Determines if EdenFS should enable the option to buffer overlay writes.
   * This only works with SQLite backed overlays.
   */
  ConfigSetting<bool> overlayBuffered{"overlay:buffered", true, this};

  /**
   * Number of bytes worth of Overlay data to keep in memory before pausing
   * enqueues to the BufferedSqliteInodeCatalog's worker thread. This is a per
   * overlay setting.
   */
  ConfigSetting<size_t> overlayBufferSize{
      "overlay:buffer-size",
      64 * 1024 * 1024,
      this};

  /**
   * Number of OverlayFile and metadata cached in memory.
   */
  ConfigSetting<size_t> overlayFileAccessCacheSize{
      "overlay:file-access-cache-size",
      100,
      this};

  // [clone]

  /**
   * Controls the mount protocol that `eden clone` will default to.
   */
  ConfigSetting<MountProtocol> defaultCloneMountProtocol{
      "clone:default-mount-protocol",
      folly::kIsWindows ? MountProtocol::PRJFS : MountProtocol::FUSE,
      this};

  /**
   * Controls whether clone sets up the working copy's ".hg" repo dir as a
   * symlink into the state dir ("off mount").
   */
  ConfigSetting<bool> offMountRepoDir{"clone:off-mount-repo-dir", false, this};

  /**
   * Controls the timeout (in seconds) that is set when calling the Thrift mount
   * endpoint from the CLI during clone. A value of 0 means no timeout.
   */
  ConfigSetting<size_t> cloneMountTimeout{"clone:mount-timeout", 20, this};

  // [fsck]

  /**
   * When FSCK is running, how often should we log about the number of scanned
   * directories. This is the number of directories that are scanned in between
   * logs.
   */
  ConfigSetting<uint64_t> fsckLogFrequency{"fsck:log-frequency", 10000, this};

  /**
   * Should FSCK be run on multiple threads, or serialized. This option is
   * specific to Windows.
   */
  ConfigSetting<bool> multiThreadedFsck{"fsck:multi-threaded", true, this};

  /**
   * The number of threads that the OverlayChecker will use when performing
   * error discovery.
   */
  ConfigSetting<uint64_t> fsckNumErrorDiscoveryThreads{
      "fsck:num-error-discovery-threads",
      4,
      this};

  // [glob]

  /**
   * Whether glob requests should use the mount's case sensitivity setting (i.e.
   * interpret patterns as case-insensitive on case-insensitive systems). If
   * false, globs are always case-sensitive.
   * TODO: Remove this killswitch after the feature proves to be safe.
   */
  ConfigSetting<bool> globUseMountCaseSensitivity{
      "glob:use-mount-case-sensitivity",
      true,
      this};

  /**
   * Controls whether EdenFS uses EdenAPI to make suffix queries
   */
  ConfigSetting<bool> enableEdenAPISuffixQuery{
      "glob:use-edenapi-suffix-query",
      false,
      this};

  /**
   * Allowed suffix queries for offloading to EdenAPI
   */
  ConfigSetting<std::unordered_set<std::string>> allowedSuffixQueries{
      "glob:allowed-suffix-queries",
      {},
      this};

  // [doctor]

  /**
   * Class names of doctor problems that should not be reported to the user or
   * automatically fixed.
   */
  ConfigSetting<std::vector<std::string>> doctorIgnoredProblemClassNames{
      "doctor:ignored-problem-class-names",
      {},
      this};

  /**
   * Whether edenfsctl doctor should check for Kerberos certificate issues.
   */
  ConfigSetting<bool> doctorEnableKerberosCheck{
      "doctor:enable-kerberos-check",
      false,
      this};

  /**
   * The minimum kernel version required for EdenFS to work correctly.
   */
  ConfigSetting<std::string> doctorMinimumKernelVersion{
      "doctor:minimum-kernel-version",
      "4.11.3-67",
      this};

  /**
   * Known bad kernel versions for which we should print a warning in `edenfsctl
   * doctor`.
   */
  ConfigSetting<std::string> doctorKnownBadKernelVersions{
      "doctor:known-bad-kernel-versions",
      "TODO,TEST",
      this};

  /**
   * Known bad edenfs versions for which we should print a warning in `edenfsctl
   * doctor`. Currently not used in Daemon.
   * Format:
   * [<bad_version_1>|<sev_1(optional):reason_1>,<bad_version_2>|<sev_2(optional):reason_2>]
   */
  ConfigSetting<std::vector<std::string>> doctorKnownBadEdenFsVersions{
      "doctor:known-bad-edenfs-versions",
      {},
      this};

  /**
   * Extensions that may do bad things that we want to warn about in
   * doctor.
   */
  ConfigSetting<std::vector<std::string>> doctorExtensionWarnList{
      "doctor:vscode-extensions-warn-list",
      {},
      this};

  /**
   * Extensions that will do bad things that we definitely want to advise
   * against using.
   */
  ConfigSetting<std::vector<std::string>> doctorExtensionBlockList{
      "doctor:vscode-extensions-block-list",
      {},
      this};

  /**
   * Extensions that we know are fine and should not be warned against.
   */
  ConfigSetting<std::vector<std::string>> doctorExtensionAllowList{
      "doctor:vscode-extensions-allow-list",
      {},
      this};

  /**
   * Extensions authors that are known and we should not warn about.
   */
  ConfigSetting<std::vector<std::string>> doctorExtensionAuthorAllowList{
      "doctor:vscode-extensions-author-allow-list",
      {},
      this};

  // [rage]
  /**
   * The tool that will be used to upload eden rages. Only currently used in the
   * CLI.
   */
  ConfigSetting<std::string> rageReporter{"rage:reporter", "", this};

  // [hash]
  /**
   * The key to use for blake3 hash computation.
   * The key must be exactly 32 bytes.
   *
   * !!!IMPORTANT!!!
   * The value of this config must be kept in sync with the source control
   * server as well as all the tools that will compute a Blake3 hash from file
   * content. To this effect, this config is overwritten early at startup in
   * Meta's environment to prevent any mismatch.
   */
  ConfigSetting<std::optional<std::string>> blake3Key{
      "hash:blake3-key",
      std::nullopt,
      this};

  // [notify]
  /**
   * This is the maximum number of changes that changesSinceV2 will return.
   */
  ConfigSetting<uint64_t> notifyMaxNumberOfChanges{
      "notify:max-num-changes",
      10000,
      this};

  /**
   * Vector of known VCS directories - used to filter changesSinceV2 results.
   */
  ConfigSetting<std::vector<RelativePath>> vcsDirectories{
      "notify:vcs-directories",
      {RelativePath{".hg"}, RelativePath{".git"}, RelativePath{".sl"}},
      this};

  /**
   * In-repo location for notifications states
   */
  ConfigSetting<RelativePath> notificationsStateDirectory{
      "notify:state-directory",
      RelativePath{".edenfs-notifications-state"},
      this};

// [facebook]
// Facebook internal

/**
 * (Facebook Internal) Determines if EdenFS should use ServiceRouter.
 */
#ifdef EDEN_HAVE_SERVICEROUTER
  ConfigSetting<bool> enableServiceRouter{
      "facebook:enable-service-router",
      fb_has_servicerouter(),
      this};
#else
  ConfigSetting<bool> enableServiceRouter{
      "facebook:enable-service-router",
      false,
      this};
#endif
};

} // namespace facebook::eden
