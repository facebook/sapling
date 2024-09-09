/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/SysTypes.h>
#include <cstdint>
#include <optional>
#include <string>

#include "eden/common/os/ProcessId.h"
#include "eden/common/telemetry/DynamicEvent.h"

namespace facebook::eden {

struct Fsck {
  static constexpr const char* type = "fsck";

  double duration = 0.0;
  bool success = false;
  bool attempted_repair = false;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addBool("attempted_repair", attempted_repair);
  }
};

struct StarGlob {
  static constexpr const char* type = "star_glob";

  std::string glob_request;
  std::string client_cmdline;

  void populate(DynamicEvent& event) const {
    event.addString("glob_request", glob_request);
    event.addString("client_cmdline", client_cmdline);
  }
};

struct SuffixGlob {
  static constexpr const char* type = "suffix_glob";

  double duration = 0.0;
  std::string glob_request;
  std::string client_cmdline;
  bool is_local;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addString("glob_request", glob_request);
    event.addString("client_scope", client_cmdline);
    event.addBool("is_local", is_local);
  }
};

struct ExpensiveGlob {
  static constexpr const char* type = "expensive_glob";

  double duration = 0.0;
  std::string glob_request;
  std::string client_cmdline;
  bool is_local;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addString("glob_request", glob_request);
    event.addString("client_scope", client_cmdline);
    event.addBool("is_local", is_local);
  }
};

struct MissingProxyHash {
  static constexpr const char* type = "missing_proxy_hash";

  void populate(DynamicEvent&) const {}
};

struct FetchHeavy {
  static constexpr const char* type = "fetch_heavy";

  std::string client_cmdline;
  ProcessId pid;
  uint64_t fetch_count;

  void populate(DynamicEvent& event) const {
    event.addString("client_cmdline", client_cmdline);
    event.addInt("client_pid", pid.get());
    event.addInt("fetch_count", fetch_count);
  }
};

struct ParentMismatch {
  static constexpr const char* type = "parent_mismatch";

  std::string mercurial_parent;
  std::string eden_parent;

  void populate(DynamicEvent& event) const {
    event.addString("mercurial_parent", mercurial_parent);
    event.addString("eden_parent", eden_parent);
  }
};

struct DaemonStart {
  static constexpr const char* type = "daemon_start";

  double duration = 0.0;
  bool is_takeover = false;
  bool success = false;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addBool("is_takeover", is_takeover);
    event.addBool("success", success);
  }
};

struct DaemonStop {
  static constexpr const char* type = "daemon_stop";

  double duration = 0.0;
  bool is_takeover = false;
  bool success = false;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addBool("is_takeover", is_takeover);
    event.addBool("success", success);
  }
};

struct FinishedCheckout {
  static constexpr const char* type = "checkout";

  std::string mode;
  double duration = 0.0;
  bool success = false;
  int64_t fetchedTrees = 0;
  int64_t fetchedBlobs = 0;
  int64_t fetchedBlobsMetadata = 0;
  int64_t accessedTrees = 0;
  int64_t accessedBlobs = 0;
  int64_t accessedBlobsMetadata = 0;
  int64_t numConflicts = 0;

  void populate(DynamicEvent& event) const {
    event.addString("mode", mode);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addInt("fetched_trees", fetchedTrees);
    event.addInt("fetched_blobs", fetchedBlobs);
    event.addInt("fetched_blobs_metadata", fetchedBlobsMetadata);
    event.addInt("accessed_trees", accessedTrees);
    event.addInt("accessed_blobs", accessedBlobs);
    event.addInt("accessed_blobs_metadata", accessedBlobsMetadata);
    event.addInt("num_conflicts", numConflicts);
  }
};

struct FinishedMount {
  static constexpr const char* type = "mount";

  std::string backing_store_type;
  std::string repo_type;
  std::string repo_source;
  std::string fs_channel_type;
  bool is_takeover = false;
  double duration = 0.0;
  bool success = false;
  bool clean = false;
  int64_t inode_catalog_type = -1;

  void populate(DynamicEvent& event) const {
    event.addString("repo_type", repo_type);
    event.addString("repo_source", repo_source);
    event.addString("fs_channel_type", fs_channel_type);
    event.addBool("is_takeover", is_takeover);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addBool("clean", clean);
    event.addInt("overlay_type", inode_catalog_type);
  }
};

struct FuseError {
  static constexpr const char* type = "fuse_error";

  int64_t fuse_op = 0;
  int64_t error_code = 0;

  void populate(DynamicEvent& event) const {
    event.addInt("fuse_op", fuse_op);
    event.addInt("error_code", error_code);
  }
};

struct RocksDbAutomaticGc {
  static constexpr const char* type = "rocksdb_autogc";

  double duration = 0.0;
  bool success = false;
  int64_t size_before = 0;
  int64_t size_after = 0;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addInt("size_before", size_before);
    event.addInt("size_after", size_after);
  }
};

struct ThriftError {
  static constexpr const char* type = "thrift_error";

  std::string thrift_method;

  void populate(DynamicEvent& event) const {
    event.addString("thrift_method", thrift_method);
  }
};

struct ThriftAuthFailure {
  static constexpr const char* type = "thrift_auth_failure";

  std::string thrift_method;
  std::string reason;

  void populate(DynamicEvent& event) const {
    event.addString("thrift_method", thrift_method);
    event.addString("reason", reason);
  }
};

struct ServerDataFetch {
  static constexpr const char* type = "server_data_fetch";

  std::string cause;
  OptionalProcessId client_pid;
  std::optional<std::string> client_cmdline;
  std::string fetched_path;
  std::string fetched_object_type;

  void populate(DynamicEvent& event) const {
    event.addString("interface", cause);
    if (client_pid) {
      event.addInt("client_pid", client_pid.value().get());
    }
    if (client_cmdline) {
      event.addString("client_cmdline", client_cmdline.value());
    }
    event.addString("fetched_path", fetched_path);
    event.addString("fetched_object_type", fetched_object_type);
  }
};

struct NfsParsingError {
  std::string proc;
  std::string reason;

  static constexpr const char* type = "nfs_parsing_error";

  void populate(DynamicEvent& event) const {
    event.addString("interface", proc);
    event.addString("reason", reason);
  }
};

struct TooManyNfsClients {
  static constexpr const char* type = "too_many_clients";

  void populate(DynamicEvent& /*event*/) const {}
};

struct MetadataSizeMismatch {
  static constexpr const char* type = "metadata_size_mismatch";

  std::string mount_protocol;
  std::string method;

  void populate(DynamicEvent& event) const {
    event.addString("mount_protocol", mount_protocol);
    event.addString("method", method);
  }
};

struct InodeMetadataMismatch {
  uint64_t mode;
  uint64_t ino;
  uint64_t gid;
  uint64_t uid;
  uint64_t atime;
  uint64_t ctime;
  uint64_t mtime;

  static constexpr const char* type = "inode_metadata_mismatch";

  void populate(DynamicEvent& event) const {
    event.addInt("st_mode", mode);
    event.addInt("ino", ino);
    event.addInt("gid", gid);
    event.addInt("uid", uid);
    event.addInt("atime", atime);
    event.addInt("ctime", ctime);
    event.addInt("mtime", mtime);
  }
};

struct EMenuStartupFailure {
  static constexpr const char* type = "emenu_startup_failure";

  std::string reason;

  void populate(DynamicEvent& event) const {
    event.addString("reason", reason);
  }
};

struct PrjFSFileNotificationFailure {
  static constexpr const char* type = "prjfs_file_notification_failure";

  std::string reason;
  std::string path;

  void populate(DynamicEvent& event) const {
    event.addString("reason", reason);
    event.addString("path", path);
  }
};

struct PrjFSCheckoutReadRace {
  static constexpr const char* type = "prjfs_checkout_read_race";

  std::string client_cmdline;

  void populate(DynamicEvent& event) const {
    event.addString("client_cmdline", client_cmdline);
  }
};

struct WorkingCopyGc {
  static constexpr const char* type = "working_copy_gc";

  double duration = 0.0;
  int64_t numInvalidated = 0;
  bool success = false;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addInt("num_invalidated", numInvalidated);
    event.addBool("success", success);
  }
};

struct SqliteIntegrityCheck {
  static constexpr const char* type = "sqlite_integrity_check";

  double duration = 0.0;
  int64_t numErrors = 0;

  void populate(DynamicEvent& event) const {
    event.addDouble("duration", duration);
    event.addInt("num_errors", numErrors);
  }
};

struct NfsCrawlDetected {
  static constexpr const char* type = "nfs_crawl_detected";

  int64_t readCount = 0;
  int64_t readThreshold = 0;
  int64_t readDirCount = 0;
  int64_t readDirThreshold = 0;
  // root->leaf formatted as:
  //   "[simple_name (pid): full_name] -> [simple_name (pid): full_name] -> ..."
  std::string processHierarchy;

  void populate(DynamicEvent& event) const {
    event.addInt("read_count", readCount);
    event.addInt("read_threshold", readThreshold);
    event.addInt("readdir_count", readDirCount);
    event.addInt("readdir_threshold", readDirThreshold);
    event.addString("process_hierarchy", processHierarchy);
  }
};

struct FetchMiss {
  enum MissType : uint8_t {
    Tree = 0,
    Blob = 1,
    BlobMetadata = 2,
    TreeMetadata = 3
  };

  std::string_view missTypeToString(MissType miss) const {
    switch (miss) {
      case Tree:
        return "Tree";
      case Blob:
        return "Blob";
      case BlobMetadata:
        return "BlobMetadata";
      case TreeMetadata:
        return "TreeMetadata";
      default:
        return "Unknown";
    }
  }

  static constexpr const char* type = "fetch_miss";

  std::string_view repo_source;
  MissType miss_type;
  std::string reason;
  bool retry;

  void populate(DynamicEvent& event) const {
    event.addString("repo_source", std::string(repo_source));
    if (miss_type == Tree) {
      event.addString("miss_type", "tree");
    } else if (miss_type == Blob) {
      event.addString("miss_type", "blob");
    } else if (miss_type == BlobMetadata) {
      event.addString("miss_type", "blob_aux");
    } else if (miss_type == TreeMetadata) {
      event.addString("miss_type", "tree_aux");
    } else {
      throw std::range_error(
          fmt::format("Unknown miss type: {}", missTypeToString(miss_type)));
    }
    event.addString("reason", reason);
    event.addBool("retry", retry);
  }
};

/**
 * So that we know how many hosts have EdenFS handling high numbers of fuse
 * requests at once as we rollout rate limiting.
 *
 * This honestly could be an ODS counter, but we don't have ODS on some
 * platforms (CI), so logging it to scuba so that we have this available to
 * monitor on all platforms.
 */
struct ManyLiveFsChannelRequests {
  static constexpr const char* type = "high_fschannel_requests";

  void populate(DynamicEvent& /*event*/) const {}
};

/**
 * Used to log user actions on e-Menu
 */
struct EMenuActionEvent {
  static constexpr const char* type = "e_menu_action_events";
  enum ActionType : uint8_t { EMenuClick = 0 };

  ActionType action_type;

  void populate(DynamicEvent& event) const {
    if (action_type == EMenuClick) {
      event.addString("action_type", "EMenuClick");
    }
  }
};

} // namespace facebook::eden
