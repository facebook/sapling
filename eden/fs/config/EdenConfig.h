/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>
#include <optional>
#include <vector>

#include <folly/dynamic.h>
#include <folly/portability/SysStat.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Unistd.h>

//#include "common/rust/shed/hostcaps/hostcaps.h"
#include "eden/fs/config/ConfigSetting.h"
#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

extern const facebook::eden::AbsolutePath kUnspecifiedDefault;

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
      folly::StringPiece userName,
      uid_t userID,
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
      ConfigSource configSource,
      struct stat* configFileStat);

  /**
   * Stringify the EdenConfig for logging or debugging.
   */
  std::string toString() const;

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

  /** Get the system config dir. Default "/etc/eden" */
  const AbsolutePath& getSystemConfigDir() const;

  /** Get the system config path. Default "/etc/eden/edenfs.rc" */
  const AbsolutePath& getSystemConfigPath() const;

  /** Get the path to client certificate. */
  const std::optional<AbsolutePath> getClientCertificate() const;

  void setUserConfigPath(AbsolutePath userConfigPath);
  void setSystemConfigDir(AbsolutePath systemConfigDir);
  void setSystemConfigPath(AbsolutePath systemConfigDir);

  /**
   * Clear all configuration for the given config source.
   */
  void clearAll(ConfigSource);

  /**
   *  Register the configuration setting. The fullKey is used to parse values
   *  from the toml file. It is of the form: "core:userConfigPath"
   */
  void registerConfiguration(ConfigSettingBase* configSetting) override;

  /**
   * Returns the user's home directory
   */
  AbsolutePathPiece getUserHomePath() const;

  /**
   * Returns the user's username
   */
  const std::string& getUserName() const;

  /**
   * Returns the user's UID
   */
  uid_t getUserID() const;

  /**
   * Returns the value in optional string for the given config key.
   * Throws if the config key is ill-formed.
   */
  std::optional<std::string> getValueByFullKey(
      folly::StringPiece configKey) const;

 private:
  /**
   * Utility method for converting ConfigSource to the filename (or cli).
   * @return the string value for the ConfigSource.
   */
  std::string toString(facebook::eden::ConfigSource cs) const;

  void doCopy(const EdenConfig& source);

  void initConfigMap();

  void parseAndApplyConfigFile(
      int configFd,
      AbsolutePathPiece configPath,
      ConfigSource configSource);

  /**
   * Mapping of section name : (map of attribute : config values). The
   * ConfigSetting constructor registration populates this map.
   */
  std::map<std::string, std::map<std::string, ConfigSettingBase*>> configMap_;

  std::string userName_;
  uid_t userID_;
  AbsolutePath userHomePath_;
  AbsolutePath userConfigPath_;
  AbsolutePath systemConfigPath_;
  AbsolutePath systemConfigDir_;

  struct stat systemConfigFileStat_ {};
  struct stat userConfigFileStat_ {};

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

  // [scs]

  /**
   * Source Control Service (scs) tier
   */
  ConfigSetting<bool> useScs{"scs:use-mononoke-scs", false, this};

  ConfigSetting<std::string> scsTierName{
      "scs:tier",
      "mononoke-scs-server",
      this};
  /**
   * Log 1 in `scsThrottleErrorSampleRatio` throttling errors to save log space.
   */
  ConfigSetting<size_t> scsThrottleErrorSampleRatio{
      "scs:throttle-error-sample-ratio",
      1000,
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

  ConfigSetting<uint64_t> localStoreTreeMetaSizeLimit{
      "store:treemeta-size-limit",
      1'000'000'000,
      this};

  ConfigSetting<uint64_t> localStoreHgCommit2TreeSizeLimit{
      "store:hgcommit2tree-size-limit",
      20'000'000,
      this};

  ConfigSetting<bool> useEdenNativePrefetch{
      "store:use-eden-native-prefetch",
      false,
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
      2000,
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

  // [hg]

  /**
   * Controls whether Eden enforces parent commits in a hg status
   * (getScmStatusV2) call
   */
  ConfigSetting<bool> enforceParents{"hg:enforce-parents", true, this};

  /**
   * If this config is set, embed HgId into ObjectId, instead of using proxy
   * hash.
   */
  ConfigSetting<bool> directObjectId{"hg:use-direct-object-id", false, this};

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

  // [experimental]

  /**
   * Controls whether if EdenFS caches blobs in local store.
   */
  ConfigSetting<bool> enableBlobCaching{
      "experimental:enable-blob-caching",
      false,
      this};

  /**
   * Controls whether EdenFS uses EdenApi to import data from remote.
   */
  ConfigSetting<bool> useEdenApi{"experimental:use-edenapi", false, this};

  /**
   * Controls whether EdenFS exports itself as an NFS server.
   */
  ConfigSetting<bool> enableNfsServer{
      "experimental:enable-nfs-server",
      folly::kIsApple,
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

  // [log]

  ConfigSetting<uint64_t> maxLogFileSize{"log:max-file-size", 50000000, this};
  ConfigSetting<uint64_t> maxRotatedLogFiles{"log:num-rotated-logs", 3, this};

  // [prefetch-profiles]

  /**
   * Kill switch for the prefetch profiles feature.
   */
  ConfigSetting<bool> enablePrefetchProfiles{
      "prefetch-profiles:prefetching-enabled",
      false,
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
   * An allowlist to check for logging if the alias environment variable is
   * set
   */
  ConfigSetting<std::vector<std::string>> logFileAccessesAliasAllowlist{
      "prefetch-profiles:file-access-logging-alias-allowlist",
      std::vector<std::string>{},
      this};

  /**
   * The number of globs to use for a predictive prefetch profile,
   * 10,000 by default.
   */
  ConfigSetting<uint32_t> predictivePrefetchProfileSize{
      "predictive-prefetch-profiles:size",
      10000,
      this};

  /**
   * Only used in CLI to control if new clones are using TreeOverlay by default.
   * Adding here to avoid unknown configuration warning.
   */
  ConfigSetting<bool> cliOnlyOverlayEnableTreeOverlay{
      "overlay:enable_tree_overlay",
      false,
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

  // [clone]

  /**
   * Controls the mount protocol that `eden clone` will default to.
   */
  ConfigSetting<MountProtocol> defaultCloneMountProtocol{
      "clone:default-mount-protocol",
      folly::kIsWindows ? MountProtocol::PRJFS : MountProtocol::FUSE,
      this};

  ConfigSetting<bool> enableServiceRouter{
      "facebook:enable-service-router",
      false,
      this};
};

} // namespace facebook::eden
