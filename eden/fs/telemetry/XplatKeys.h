/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string_view>

/*
 * Key constants for XplatLogger's DynamicEvent key-value bags.
 *
 * XplatLogger logs to arbitrary Scuba tables via a generic
 * logEvent(category, DynamicEvent) method. A DynamicEvent is a bag of
 * key-value pairs carrying event-specific fields — fields that differ
 * across tables. (Shared identity fields like username, hostname, and
 * session_id are populated automatically by the transform layer from
 * EdenTelemetryIdentity.)
 *
 * These constants are shared between call sites (which populate the
 * DynamicEvent) and transform functions (which extract values to build
 * typed Thrift structs). Centralising them here prevents typos and
 * keeps key names in one place. Key names match Scuba column names
 * exactly for debuggability.
 *
 * When adding a new table, add its event-specific key constants here.
 *
 * Rust counterpart:
 * eden/fs/rust/facebook/edenfs-telemetry/src/xplat_keys.rs. Keep shared
 * XplatLogger key names and logger config constants in sync between the C++
 * and Rust files when adding or renaming fields.
 */
namespace facebook::eden::xplat_keys {

// --- edenfs_file_accesses category ---
inline constexpr std::string_view kFileAccessCategory{
    "perfpipe_edenfs_file_accesses"};

// --- edenfs_file_accesses fields ---
inline constexpr std::string_view kRepo = "repo";
inline constexpr std::string_view kDirectory = "directory";
inline constexpr std::string_view kFilename = "filename";
inline constexpr std::string_view kSource = "source";
inline constexpr std::string_view kSourceDetail = "source_detail";
inline constexpr std::string_view kWeight = "weight";

// ===================================================================
// edenfs_events category
// ===================================================================
inline constexpr std::string_view kEventsCategory{"perfpipe_edenfs_events"};

// --- Common fields (used across many event types) ---
inline constexpr std::string_view kType = "type";
inline constexpr std::string_view kDuration = "duration";
inline constexpr std::string_view kSuccess = "success";
inline constexpr std::string_view kReason = "reason";
inline constexpr std::string_view kPath = "path";
inline constexpr std::string_view kClientCmdline = "client_cmdline";
inline constexpr std::string_view kClientPid = "client_pid";
inline constexpr std::string_view kIsTakeover = "is_takeover";
inline constexpr std::string_view kRepoSource = "repo_source";
inline constexpr std::string_view kError = "error";
inline constexpr std::string_view kInterface = "interface";
inline constexpr std::string_view kIno = "ino";
inline constexpr std::string_view kActionType = "action_type";

// --- Fsck ---
inline constexpr std::string_view kAttemptedRepair = "attempted_repair";

// --- Glob events (StarGlob, SuffixGlob, ExpensiveGlob) ---
inline constexpr std::string_view kGlobRequest = "glob_request";
inline constexpr std::string_view kIsLocal = "is_local";

// --- FetchHeavy ---
inline constexpr std::string_view kFetchCount = "fetch_count";
inline constexpr std::string_view kLoadedInodes = "loaded_inodes";

// --- ParentMismatch ---
inline constexpr std::string_view kMercurialParent = "mercurial_parent";
inline constexpr std::string_view kEdenParent = "eden_parent";

// --- DaemonStart ---
inline constexpr std::string_view kDaemonMountNamespace =
    "daemon_mount_namespace";
inline constexpr std::string_view kDaemonPidNamespace = "daemon_pid_namespace";
inline constexpr std::string_view kPrivhelperMountNamespace =
    "privhelper_mount_namespace";
inline constexpr std::string_view kPrivhelperPidNamespace =
    "privhelper_pid_namespace";
inline constexpr std::string_view kIsDaemonInRootMountNamespace =
    "is_daemon_in_root_mount_namespace";
inline constexpr std::string_view kIsPrivhelperInRootMountNamespace =
    "is_privhelper_in_root_mount_namespace";
inline constexpr std::string_view kCgroup = "cgroup";

// --- FinishedCheckout ---
inline constexpr std::string_view kMode = "mode";
inline constexpr std::string_view kFetchedTrees = "fetched_trees";
inline constexpr std::string_view kFetchedBlobs = "fetched_blobs";
inline constexpr std::string_view kFetchedBlobsMetadata =
    "fetched_blobs_metadata";
inline constexpr std::string_view kAccessedTrees = "accessed_trees";
inline constexpr std::string_view kAccessedBlobs = "accessed_blobs";
inline constexpr std::string_view kAccessedBlobsMetadata =
    "accessed_blobs_metadata";
inline constexpr std::string_view kNumConflicts = "num_conflicts";
inline constexpr std::string_view kUnloadedInodes = "unloaded_inodes";
inline constexpr std::string_view kLinkedUnloadedInodes =
    "linked_unloaded_inodes";
inline constexpr std::string_view kUnlinkedUnloadedInodes =
    "unlinked_unloaded_inodes";
inline constexpr std::string_view kDurationLookupTrees =
    "duration_lookup_trees";
inline constexpr std::string_view kDurationDiff = "duration_diff";
inline constexpr std::string_view kDurationAcquireRenameLock =
    "duration_acquire_rename_lock";
inline constexpr std::string_view kDurationCheckout = "duration_checkout";
inline constexpr std::string_view kDurationFinish = "duration_finish";

// --- ThriftCancellation ---
inline constexpr std::string_view kEndpoint = "endpoint";

// --- FinishedMount ---
inline constexpr std::string_view kBackingStoreType = "backing_store_type";
inline constexpr std::string_view kRepoType = "repo_type";
inline constexpr std::string_view kFsChannelType = "fs_channel_type";
inline constexpr std::string_view kFuseTransport = "fuse_transport";
inline constexpr std::string_view kClean = "clean";
inline constexpr std::string_view kOverlayType = "overlay_type";

// --- ServerDataFetch ---
inline constexpr std::string_view kFetchedPath = "fetched_path";
inline constexpr std::string_view kFetchedObjectType = "fetched_object_type";

// --- MetadataSizeMismatch ---
inline constexpr std::string_view kMountProtocol = "mount_protocol";
inline constexpr std::string_view kMethod = "method";

// --- InodeMetadataMismatch ---
inline constexpr std::string_view kStMode = "st_mode";
inline constexpr std::string_view kGid = "gid";
inline constexpr std::string_view kUid = "uid";
inline constexpr std::string_view kAtime = "atime";
inline constexpr std::string_view kCtime = "ctime";
inline constexpr std::string_view kMtime = "mtime";

// --- InodeLoadingFailed ---
inline constexpr std::string_view kLoadError = "load_error";
inline constexpr std::string_view kCausedByX2p = "caused_by_x2p";

// --- WorkingCopyGc ---
inline constexpr std::string_view kNumInvalidated = "num_invalidated";
inline constexpr std::string_view kNumDeletedInodes = "num_deleted_inodes";

// --- SilentDaemonExit ---
inline constexpr std::string_view kLastDaemonHeartbeat =
    "last_daemon_heartbeat";
inline constexpr std::string_view kExitSignal = "exit_signal";
inline constexpr std::string_view kSystemBootTimestamp =
    "system_boot_timestamp";
inline constexpr std::string_view kIsMemoryPressureKill =
    "is_memory_pressure_kill";
inline constexpr std::string_view kSystemLogCheckError =
    "system_log_check_error";
inline constexpr std::string_view kDaemonDowntimeS = "daemon_downtime_s";

// --- AccidentalUnmountRecovery ---
inline constexpr std::string_view kRemountError = "remount_error";

// --- EdenMountHealthIssue ---
inline constexpr std::string_view kMountPath = "mount_path";
inline constexpr std::string_view kPathType = "path_type";

// --- SqliteIntegrityCheck ---
inline constexpr std::string_view kNumErrors = "num_errors";

// --- NfsCrawlDetected ---
inline constexpr std::string_view kReadCount = "read_count";
inline constexpr std::string_view kReadThreshold = "read_threshold";
inline constexpr std::string_view kReaddirCount = "readdir_count";
inline constexpr std::string_view kReaddirThreshold = "readdir_threshold";
inline constexpr std::string_view kProcessHierarchy = "process_hierarchy";

// --- FetchMiss ---
inline constexpr std::string_view kMissType = "miss_type";
inline constexpr std::string_view kRetry = "retry";
inline constexpr std::string_view kDogfoodingHost = "dogfooding_host";

// --- LongRunningFSRequest ---
inline constexpr std::string_view kCauseDetail = "causeDetail";
inline constexpr std::string_view kAcceptedNs = "acceptedNs";
inline constexpr std::string_view kQueueWaitNs = "queueWaitNs";
inline constexpr std::string_view kProcessingNs = "processingNs";
inline constexpr std::string_view kWriteWaitNs = "writeWaitNs";

// --- ChangesSince ---
inline constexpr std::string_view kPosition = "position";
inline constexpr std::string_view kMount = "mount";
inline constexpr std::string_view kRoot = "root";
inline constexpr std::string_view kIncludedRoots = "included_roots";
inline constexpr std::string_view kExcludedRoots = "excluded_roots";
inline constexpr std::string_view kIncludedSuffixes = "included_suffixes";
inline constexpr std::string_view kExcludedSuffixes = "excluded_suffixes";
inline constexpr std::string_view kIncludeVcs = "include_vcs";
inline constexpr std::string_view kNumSmallChanges = "num_small_changes";
inline constexpr std::string_view kNumStateChanges = "num_state_changes";
inline constexpr std::string_view kNumRenamedDirectory =
    "num_renamed_directory";
inline constexpr std::string_view kNumCommitTransition =
    "num_commit_transition";
inline constexpr std::string_view kLostChanges = "lost_changes";
inline constexpr std::string_view kNumFilteredChanges = "num_filtered_changes";

// --- StaleRedirectionCleanup ---
inline constexpr std::string_view kCheckoutPath = "checkout_path";
inline constexpr std::string_view kStaleRedirectionsFound =
    "stale_redirections_found";
inline constexpr std::string_view kStaleRedirectionsSucceeded =
    "stale_redirections_succeeded";
inline constexpr std::string_view kStaleRedirectionsFailed =
    "stale_redirections_failed";
inline constexpr std::string_view kStaleCheckoutMountUnmounted =
    "stale_checkout_mount_unmounted";

// --- CheckoutUpdateError (uses kPath and kReason from common fields) ---

// --- edenfs_errors category
inline constexpr std::string_view kErrorsCategory{"perfpipe_edenfs_errors"};

// --- edenfs_errors fields (match DaemonError::populate() keys exactly) ---
inline constexpr std::string_view kComponent = "component";
inline constexpr std::string_view kErrorMessage = "error_message";
inline constexpr std::string_view kExceptionType = "exception_type";
inline constexpr std::string_view kErrorCode = "error_code";
inline constexpr std::string_view kErrorName = "error_name";
inline constexpr std::string_view kStackTrace = "stack_trace";
inline constexpr std::string_view kInode = "inode";
inline constexpr std::string_view kFilePath = "file_path";
inline constexpr std::string_view kMountPoint = "mount_point";
inline constexpr std::string_view kMountStatus = "mount_status";
inline constexpr std::string_view kErrorType = "error_type";
inline constexpr std::string_view kExtras = "extras";

} // namespace facebook::eden::xplat_keys
