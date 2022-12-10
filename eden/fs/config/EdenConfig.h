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

#include <folly/portability/SysStat.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Unistd.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>

#include "common/rust/shed/hostcaps/hostcaps.h"
#include "eden/fs/config/ConfigSetting.h"
#include "eden/fs/config/ConfigVariables.h"
#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/utils/PathFuncs.h"

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
  /**
   * Manually construct a EdenConfig object. Users can subsequently use the
   * load methods to populate the EdenConfig.
   */
  explicit EdenConfig(
      ConfigVariables substitutions,
      AbsolutePath userHomePath,
      AbsolutePath userConfigPath,
      AbsolutePath systemConfigDir,
      AbsolutePath systemConfigPath);

  explicit EdenConfig(const EdenConfig& source);

  explicit EdenConfig(EdenConfig&& source) = delete;

  EdenConfig& operator=(const EdenConfig& source);

  EdenConfig& operator=(EdenConfig&& source) = delete;

  /**
   * Create an EdenConfig for testing usage>
   */
  static std::shared_ptr<EdenConfig> createTestEdenConfig();

  /**
   * Update EdenConfig by loading the system configuration.
   */
  void loadSystemConfig();

  /**
   * Update EdenConfig by loading the user configuration.
   */
  void loadUserConfig();

  /**
   * Load the configuration based on the passed path. The configuation source
   * identifies whether the config file is a system or user config file and
   * apply setting over-rides appropriately. The passed configFile stat is
   * updated with the config files fstat results.
   */
  void loadConfig(
      AbsolutePathPiece path,
      ConfigSourceType configSourceType,
      std::optional<FileStat>& configFileStat);

  /**
   * Return the config data as a EdenConfigData structure that can be
   * thrift-serialized.
   */
  EdenConfigData toThriftConfigData() const;

  /** Determine if user config has changed, fstat userConfigFile.*/
  FileChangeReason hasUserConfigFileChanged() const;

  /** Determine if user config has changed, fstat systemConfigFile.*/
  FileChangeReason hasSystemConfigFileChanged() const;

  /** Get the user config path. Default "userHomePath/.edenrc" */
  const AbsolutePath& getUserConfigPath() const;

  /** Get the system config path. Default "/etc/eden/edenfs.rc" */
  const AbsolutePath& getSystemConfigPath() const;

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

  void doCopy(const EdenConfig& source);

  void initConfigMap();

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
  AbsolutePath userConfigPath_;
  AbsolutePath systemConfigPath_;

  std::optional<FileStat> systemConfigFileStat_;
  std::optional<FileStat> userConfigFileStat_;

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
      false,
      this};

  /**
   * If EdenFS should force a non-graceful restart, if necessary, to auto
   * migrate FUSE repos to NFS on all versions of macOS.  Only used in the CLI
   * and edenfs_restarter, including here to get rid of warnings.
   */
  ConfigSetting<bool> migrateToNFSAllMacOS{
      "core:migrate_existing_to_nfs_all_macos",
      false,
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

  // [ssl]

  ConfigSetting<AbsolutePath> clientCertificate{
      "ssl:client-certificate",
      kUnspecifiedDefault,
      this};
  ConfigSetting<std::vector<AbsolutePath>> clientCertificateLocations{
      "ssl:client-certificate-locations",
      std::vector<AbsolutePath>{},
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
   * Files with an atime older than this will be invalidated during GC.
   *
   * Default to a day. A value of 0 will invalidate all non-materialized files.
   * On Windows, the atime is  updated only once an hour, so values below 1h
   * may over-invalidate.
   */
  ConfigSetting<std::chrono::nanoseconds> gcCutoff{
      "mount:garbage-collection-cutoff",
      std::chrono::hours(24),
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

  // [fuse]

  /**
   * The maximum number of concurrent FUSE requests we allow the kernel to send
   * us.
   *
   * Linux FUSE defaults to 12, but EdenFS can handle a great deal of
   * concurrency.
   */
  ConfigSetting<int32_t> fuseMaximumRequests{
      "fuse:max-concurrent-requests",
      1000,
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
   * Whether Eden should attempt to load and use /dev/edenfs in preference
   * to other fuse implementations that may be available on the system.
   */
  ConfigSetting<bool> fuseUseEdenFS{"fuse:use-edenfs", false, this};

  /**
   * The maximum number of concurrent requests allowed into userspace from the
   * kernel. This corresponds to fuse_init_out::max_background. The
   * documentation this applies to only readaheads and async direct IO, but
   * empirically we have observed the number of concurrent requests is limited
   * to 12 (FUSE_DEFAULT_MAX_BACKGROUND) unless this is set high.
   */
  ConfigSetting<uint32_t> maximumFuseRequests{"fuse:max-requests", 1000, this};

  // [nfs]

  /**
   * The maximum time duration allowed for a NFS request. If a request exceeds
   * this amount of time, an NFS3ERR_JUKEBOX error will be returned to the
   * client to avoid blocking forever.
   */
  ConfigSetting<std::chrono::nanoseconds> nfsRequestTimeout{
      "nfs:request-timeout",
      std::chrono::minutes(1),
      this};

  /**
   * Controls whether Mountd will register itself against rpcbind.
   */
  ConfigSetting<bool> registerMountd{"nfs:register-mountd", false, this};

  /**
   * Number of threads that will service the NFS requests.
   */
  ConfigSetting<uint64_t> numNfsThreads{"nfs:num-servicing-threads", 8, this};

  /**
   * Maximum number of pending NFS requests. If more requests are inflight, the
   * NFS code will block.
   */
  ConfigSetting<uint64_t> maxNfsInflightRequests{
      "nfs:max-inflight-requests",
      1000,
      this};

  /**
   * Buffer size for read and writes requests. Default to 1 MiB.
   */
  ConfigSetting<uint32_t> nfsIoSize{"nfs:iosize", 1024 * 1024, this};

  /**
   * Whether EdenFS NFS sockets should bind themself to unix sockets instead of
   * TCP ones.
   *
   * Unix sockets bypass the overhead of TCP and are thus significantly faster.
   * This is only supported on macOS.
   */
  ConfigSetting<bool> useUnixSocket{"nfs:use-uds", false, this};

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
   * When set to true, we will use readdirplus instead of readdir. Readdirplus
   * will be enabled for all nfs mounts. If set to false, regular readdir is
   * used instead.
   */
  ConfigSetting<bool> useReaddirplus{"nfs:use-readdirplus", false, this};

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

  // [hg]

  /**
   * Controls whether Eden enforces parent commits in a hg status
   * (getScmStatusV2) call
   */
  ConfigSetting<bool> enforceParents{"hg:enforce-parents", true, this};

  /**
   * Controls whether EdenFS reads blob metadata directly from hg
   */
  ConfigSetting<bool> useAuxMetadata{"hg:use-aux-metadata", true, this};

  /**
   * Which object ID format should the HgBackingStore use?
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
   * HgBackingStore
   */
  ConfigSetting<uint32_t> importBatchSize{"hg:import-batch-size", 1, this};

  /**
   * Controls the number of tree import requests we batch in HgBackingStore
   */
  ConfigSetting<uint32_t> importBatchSizeTree{
      "hg:import-batch-size-tree",
      1,
      this};

  /**
   * Whether fetching trees should fall back on an external hg importer process.
   */
  ConfigSetting<bool> hgTreeFetchFallback{"hg:tree-fetch-fallback", true, this};

  /**
   * Whether fetching blobs should fall back on an external hg importer process.
   */
  ConfigSetting<bool> hgBlobFetchFallback{"hg:blob-fetch-fallback", true, this};

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
  ConfigSetting<uint32_t> ActivityBufferMaxEvents{
      "telemetry:activitybuffer-max-events",
      100,
      this};

  // [experimental]

  /**
   * Controls whether interrupted checkouts can be resumed.
   */
  ConfigSetting<bool> allowResumeCheckout{
      "experimental:allow-resume-checkout",
      false,
      this};

  /**
   * Controls whether if EdenFS caches blobs in local store.
   */
  ConfigSetting<bool> enableBlobCaching{
      "experimental:enable-blob-caching",
      false,
      this};

  /**
   * Controls whether EdenFS uses EdenApi to import data from remote.
   *
   * TODO: Remove once this config value is no longer written.
   */
  ConfigSetting<bool> useEdenApi{"experimental:use-edenapi", true, this};

  /**
   * Controls whether EdenFS exports itself as an NFS server.
   */
  ConfigSetting<bool> enableNfsServer{
      "experimental:enable-nfs-server",
      folly::kIsApple,
      this};

  /**
   * Controls whether EdenFS will periodically garbage collect the working
   * directory.
   *
   * For now, this really only makes sense on Windows, with unknown behavior on
   * Linux and macOS.
   */
  ConfigSetting<bool> enableGc{
      "experimental:enable-garbage-collection",
      false,
      this};

  // [treecache]

  /**
   * Controls whether if EdenFS caches tree in memory.
   */
  ConfigSetting<bool> enableInMemoryTreeCaching{
      "treecache:enable-in-memory-tree-caching",
      true,
      this};

  /**
   * Number of bytes worth of data to keep in memory
   */
  ConfigSetting<size_t> inMemoryTreeCacheSize{
      "treecache:cache-size",
      40 * 1024 * 1024,
      this};

  /**
   * The minimum number of recent tree to keep cached. Trumps
   * inMemoryTreeCacheSize
   */
  ConfigSetting<size_t> inMemoryTreeCacheMinElements{
      "treecache:min-cache-elements",
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
      false,
      this};

  /**
   * Whether notifications are completely disabled
   */
  ConfigSetting<bool> enableNotifications{
      "notifications:enable-notifications",
      false,
      this};

  /**
   * Whether the debug menu is shown in the Eden Menu
   */
  ConfigSetting<bool> enableEdenDebugMenu{
      "notifications:enable-debug",
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
   * Used to control file access logging for predicitve prefetch
   * profiles.
   */
  ConfigSetting<bool> logFileAccesses{
      "prefetch-profiles:file-access-logging-enabled",
      false,
      this};

  /**
   * A number from 0 - x to determine how often we should log file access
   * events. This is currectly agnostic to the type of file access. If this
   * is not at 100%, we will not log filenames and we will only log directory
   * paths. In the following equation, 1/x = percentage, x is this variable.
   * For 50% rollout, 1/x = .5, so x = 2, so this would be set to 2. 0
   * indicates that the feature is off.
   */
  ConfigSetting<uint32_t> logFileAccessesSamplingDenominator{
      "prefetch-profiles:file-access-logging-sampling-denominator",
      0,
      this};

  /**
   * The number of globs to use for a predictive prefetch profile,
   * 1500 by default.
   */
  ConfigSetting<uint32_t> predictivePrefetchProfileSize{
      "predictive-prefetch-profiles:size",
      1500,
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
   * The synchronous mode used when using tree overlay. Currently it only
   * supports "off" or "normal". Setting this to off may cause data loss.
   */
  ConfigSetting<std::string> overlaySynchronousMode{
      "overlay:synchronous-mode",
      "normal",
      this};

  /**
   * This option controls how often we run SQLite WAL checkpoint in tree
   * overlay. This option is ignored in other overlay types.
   */
  ConfigSetting<std::chrono::nanoseconds> overlayMaintenanceInterval{
      "overlay:maintenance-interval",
      std::chrono::minutes(1),
      this};

  /**
   * Determines if EdenFS should enable the option to buffer overlay writes.
   * This only works with tree overlays.
   */
  ConfigSetting<bool> overlayBuffered{"overlay:buffered", false, this};

  /**
   * Number of bytes worth of Overlay data to keep in memory before pausing
   * enqueues to the BufferedSqliteInodeCatalog's worker thread. This is a per
   * overlay setting.
   */
  ConfigSetting<size_t> overlayBufferSize{
      "overlay:buffer-size",
      64 * 1024 * 1024,
      this};

  // [clone]

  /**
   * Controls the mount protocol that `eden clone` will default to.
   */
  ConfigSetting<MountProtocol> defaultCloneMountProtocol{
      "clone:default-mount-protocol",
      folly::kIsWindows ? MountProtocol::PRJFS : MountProtocol::FUSE,
      this};

  // [fsck]

  /**
   * True if Windows FSCK should use the new, more thorough version.
   */
  ConfigSetting<bool> useThoroughFsck{"fsck:use-thorough-fsck", true, this};

  /**
   * When FSCK is running, how often should we log about the number of scanned
   * directories. This is the number of directories that are scanned in between
   * logs.
   */
  ConfigSetting<uint64_t> fsckLogFrequency{"fsck:log-frequency", 10000, this};

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
