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
#include "eden/common/telemetry/LogEvent.h"

namespace facebook::eden {

struct EdenFSEvent : public TypedEvent {
  // Keep populate() and getType() pure virtual to force subclasses
  // to implement them
  virtual void populate(DynamicEvent&) const override = 0;
  virtual const char* getType() const override = 0;
};

struct EdenFSFileAccessEvent : public TypelessEvent {
  // Keep populate() pure virtual to force subclasses to implement it
  virtual void populate(DynamicEvent&) const override = 0;
};

struct Fsck : public EdenFSEvent {
  double duration = 0.0;
  bool success = false;
  bool attempted_repair = false;

  Fsck(double duration, bool success, bool attempted_repair)
      : duration(duration),
        success(success),
        attempted_repair(attempted_repair) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addBool("attempted_repair", attempted_repair);
  }

  const char* getType() const override {
    return "fsck";
  }
};

struct StarGlob : public EdenFSEvent {
  std::string glob_request;
  std::string client_cmdline;

  StarGlob(std::string glob_request, std::string client_cmdline)
      : glob_request(std::move(glob_request)),
        client_cmdline(std::move(client_cmdline)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("glob_request", glob_request);
    event.addString("client_cmdline", client_cmdline);
  }

  const char* getType() const override {
    return "star_glob";
  }
};

struct SuffixGlob : public EdenFSEvent {
  double duration = 0.0;
  std::string glob_request;
  std::string client_cmdline;
  bool is_local;

  SuffixGlob(
      double duration,
      std::string glob_request,
      std::string client_cmdline,
      bool is_local)
      : duration(duration),
        glob_request(std::move(glob_request)),
        client_cmdline(std::move(client_cmdline)),
        is_local(is_local) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addString("glob_request", glob_request);
    event.addString("client_cmdline", client_cmdline);
    event.addBool("is_local", is_local);
  }

  const char* getType() const override {
    return "suffix_glob";
  }
};

struct ExpensiveGlob : public EdenFSEvent {
  double duration = 0.0;
  std::string glob_request;
  std::string client_cmdline;
  bool is_local;

  ExpensiveGlob(
      double duration,
      std::string glob_request,
      std::string client_cmdline,
      bool is_local)
      : duration(duration),
        glob_request(std::move(glob_request)),
        client_cmdline(std::move(client_cmdline)),
        is_local(is_local) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addString("glob_request", glob_request);
    event.addString("client_cmdline", client_cmdline);
    event.addBool("is_local", is_local);
  }

  const char* getType() const override {
    return "expensive_glob";
  }
};

struct MissingProxyHash : public EdenFSEvent {
  void populate(DynamicEvent&) const override {}

  const char* getType() const override {
    return "missing_proxy_hash";
  }
};

struct FetchHeavy : public EdenFSEvent {
  std::string client_cmdline;
  ProcessId pid;
  uint64_t fetch_count;
  std::optional<uint64_t> loaded_inodes;

  FetchHeavy(
      std::string client_cmdline,
      ProcessId pid,
      uint64_t fetch_count,
      std::optional<uint64_t> loaded_inodes)
      : client_cmdline(std::move(client_cmdline)),
        pid(std::move(pid)),
        fetch_count(fetch_count),
        loaded_inodes(loaded_inodes) {}

  void populate(DynamicEvent& event) const override {
    event.addString("client_cmdline", client_cmdline);
    event.addInt("client_pid", pid.get());
    event.addInt("fetch_count", fetch_count);
    if (loaded_inodes.has_value()) {
      event.addTruncatedInt("loaded_inodes", loaded_inodes.value(), 8U);
    }
  }

  const char* getType() const override {
    return "fetch_heavy";
  }
};

struct ParentMismatch : public EdenFSEvent {
  std::string mercurial_parent;
  std::string eden_parent;

  ParentMismatch(std::string mercurial_parent, std::string eden_parent)
      : mercurial_parent(std::move(mercurial_parent)),
        eden_parent(std::move(eden_parent)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("mercurial_parent", mercurial_parent);
    event.addString("eden_parent", eden_parent);
  }

  const char* getType() const override {
    return "parent_mismatch";
  }
};

struct DaemonStart : public EdenFSEvent {
  double duration = 0.0;
  bool is_takeover = false;
  bool success = false;

  DaemonStart(double duration, bool is_takeover, bool success)
      : duration(duration), is_takeover(is_takeover), success(success) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addBool("is_takeover", is_takeover);
    event.addBool("success", success);
  }

  const char* getType() const override {
    return "daemon_start";
  }
};

struct DaemonStop : public EdenFSEvent {
  double duration = 0.0;
  bool is_takeover = false;
  bool success = false;

  DaemonStop(double duration, bool is_takeover, bool success)
      : duration(duration), is_takeover(is_takeover), success(success) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addBool("is_takeover", is_takeover);
    event.addBool("success", success);
  }

  const char* getType() const override {
    return "daemon_stop";
  }
};

struct FinishedCheckout : public EdenFSEvent {
  std::string mode;
  double duration = 0.0;
  bool success = false;
  uint64_t fetchedTrees = 0;
  uint64_t fetchedBlobs = 0;
  uint64_t fetchedBlobsAuxData = 0;
  uint64_t accessedTrees = 0;
  uint64_t accessedBlobs = 0;
  uint64_t accessedBlobsAuxData = 0;
  uint64_t numConflicts = 0;
  uint64_t numLoadedInodes = 0;
  uint64_t numUnloadedInodes = 0;
  uint64_t numPeriodicLinkedUnloadedInodes = 0;
  uint64_t numPeriodicUnlinkedUnloadedInodes = 0;

  FinishedCheckout(
      std::string mode,
      double duration,
      bool success,
      uint64_t fetchedTrees,
      uint64_t fetchedBlobs,
      uint64_t fetchedBlobsAuxData,
      uint64_t accessedTrees,
      uint64_t accessedBlobs,
      uint64_t accessedBlobsAuxData,
      uint64_t numConflicts,
      uint64_t numLoadedInodes,
      uint64_t numUnloadedInodes,
      uint64_t numPeriodicLinkedUnloadedInodes,
      uint64_t numPeriodicUnlinkedUnloadedInodes)
      : mode(std::move(mode)),
        duration(duration),
        success(success),
        fetchedTrees(fetchedTrees),
        fetchedBlobs(fetchedBlobs),
        fetchedBlobsAuxData(fetchedBlobsAuxData),
        accessedTrees(accessedTrees),
        accessedBlobs(accessedBlobs),
        accessedBlobsAuxData(accessedBlobsAuxData),
        numConflicts(numConflicts),
        numLoadedInodes(numLoadedInodes),
        numUnloadedInodes(numUnloadedInodes),
        numPeriodicLinkedUnloadedInodes(numPeriodicLinkedUnloadedInodes),
        numPeriodicUnlinkedUnloadedInodes(numPeriodicUnlinkedUnloadedInodes) {}

  void populate(DynamicEvent& event) const override {
    event.addString("mode", mode);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addInt("fetched_trees", fetchedTrees);
    event.addInt("fetched_blobs", fetchedBlobs);
    event.addInt("fetched_blobs_metadata", fetchedBlobsAuxData);
    event.addInt("accessed_trees", accessedTrees);
    event.addInt("accessed_blobs", accessedBlobs);
    event.addInt("accessed_blobs_metadata", accessedBlobsAuxData);
    event.addInt("num_conflicts", numConflicts);
    event.addInt("loaded_inodes", numLoadedInodes);
    event.addInt("unloaded_inodes", numUnloadedInodes);
    event.addInt("linked_unloaded_inodes", numPeriodicLinkedUnloadedInodes);
    event.addInt("unlinked_unloaded_inodes", numPeriodicUnlinkedUnloadedInodes);
  }

  const char* getType() const override {
    return "checkout";
  }
};

struct StaleContents : public EdenFSEvent {
  std::string path;
  uint64_t ino;

  explicit StaleContents(std::string path, uint64_t ino)
      : path(std::move(path)), ino(ino) {}

  void populate(DynamicEvent& event) const override {
    event.addString("path", path);
    event.addInt("ino", ino);
  }

  const char* getType() const override {
    return "stale_contents";
  }
};

struct NFSStaleError : public EdenFSEvent {
  uint64_t ino;

  explicit NFSStaleError(uint64_t ino) : ino(ino) {}

  void populate(DynamicEvent& event) const override {
    event.addInt("ino", ino);
  }

  const char* getType() const override {
    return "nfs_stale_error";
  }
};

struct FinishedMount : public EdenFSEvent {
  std::string backing_store_type;
  std::string repo_type;
  std::string repo_source;
  std::string fs_channel_type;
  bool is_takeover = false;
  double duration = 0.0;
  bool success = false;
  bool clean = false;
  int64_t inode_catalog_type = -1;

  FinishedMount(
      std::string backing_store_type,
      std::string repo_type,
      std::string repo_source,
      std::string fs_channel_type,
      bool is_takeover,
      double duration,
      bool success,
      bool clean,
      int64_t inode_catalog_type)
      : backing_store_type(std::move(backing_store_type)),
        repo_type(std::move(repo_type)),
        repo_source(std::move(repo_source)),
        fs_channel_type(std::move(fs_channel_type)),
        is_takeover(is_takeover),
        duration(duration),
        success(success),
        clean(clean),
        inode_catalog_type(inode_catalog_type) {}

  void populate(DynamicEvent& event) const override {
    event.addString("backing_store_type", backing_store_type);
    event.addString("repo_type", repo_type);
    event.addString("repo_source", repo_source);
    event.addString("fs_channel_type", fs_channel_type);
    event.addBool("is_takeover", is_takeover);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addBool("clean", clean);
    event.addInt("overlay_type", inode_catalog_type);
  }

  const char* getType() const override {
    return "mount";
  }
};

struct RocksDbAutomaticGc : public EdenFSEvent {
  double duration = 0.0;
  bool success = false;
  int64_t size_before = 0;
  int64_t size_after = 0;

  RocksDbAutomaticGc(
      double duration,
      bool success,
      int64_t size_before,
      int64_t size_after)
      : duration(duration),
        success(success),
        size_before(size_before),
        size_after(size_after) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addInt("size_before", size_before);
    event.addInt("size_after", size_after);
  }

  const char* getType() const override {
    return "rocksdb_autogc";
  }
};

struct ServerDataFetch : public EdenFSEvent {
  std::string cause;
  OptionalProcessId client_pid;
  std::optional<std::string> client_cmdline;
  std::string fetched_path;
  std::string fetched_object_type;

  ServerDataFetch(
      std::string cause,
      OptionalProcessId client_pid,
      std::optional<std::string> client_cmdline,
      std::string fetched_path,
      std::string fetched_object_type)
      : cause(std::move(cause)),
        client_pid(std::move(client_pid)),
        client_cmdline(std::move(client_cmdline)),
        fetched_path(std::move(fetched_path)),
        fetched_object_type(std::move(fetched_object_type)) {}

  void populate(DynamicEvent& event) const override {
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

  const char* getType() const override {
    return "server_data_fetch";
  }
};

struct NfsParsingError : public EdenFSEvent {
  std::string proc;
  std::string reason;

  NfsParsingError(std::string proc, std::string reason)
      : proc(proc), reason(reason) {}

  void populate(DynamicEvent& event) const override {
    event.addString("interface", proc);
    event.addString("reason", reason);
  }

  const char* getType() const override {
    return "nfs_parsing_error";
  }
};

struct TooManyNfsClients : public EdenFSEvent {
  void populate(DynamicEvent& /*event*/) const override {}

  const char* getType() const override {
    return "too_many_clients";
  }
};

struct MetadataSizeMismatch : public EdenFSEvent {
  std::string mount_protocol;
  std::string method;

  MetadataSizeMismatch(std::string mount_protocol, std::string method)
      : mount_protocol(std::move(mount_protocol)), method(std::move(method)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("mount_protocol", mount_protocol);
    event.addString("method", method);
  }

  const char* getType() const override {
    return "metadata_size_mismatch";
  }
};

struct InodeMetadataMismatch : public EdenFSEvent {
  uint64_t mode;
  uint64_t ino;
  uint64_t gid;
  uint64_t uid;
  uint64_t atime;
  uint64_t ctime;
  uint64_t mtime;

  InodeMetadataMismatch(
      uint64_t mode,
      uint64_t ino,
      uint64_t gid,
      uint64_t uid,
      uint64_t atime,
      uint64_t ctime,
      uint64_t mtime)
      : mode(mode),
        ino(ino),
        gid(gid),
        uid(uid),
        atime(atime),
        ctime(ctime),
        mtime(mtime) {}

  void populate(DynamicEvent& event) const override {
    event.addInt("st_mode", mode);
    event.addInt("ino", ino);
    event.addInt("gid", gid);
    event.addInt("uid", uid);
    event.addInt("atime", atime);
    event.addInt("ctime", ctime);
    event.addInt("mtime", mtime);
  }

  const char* getType() const override {
    return "inode_metadata_mismatch";
  }
};

struct InodeLoadingFailed : public EdenFSEvent {
  std::string error;
  uint64_t ino;
  bool causedByX2P = false;

  explicit InodeLoadingFailed(std::string err, uint64_t ino)
      : error(std::move(err)), ino(ino) {
    if (error.find("x-x2pagentd-error")) {
      causedByX2P = true;
    }
  }

  void populate(DynamicEvent& event) const override {
    event.addString("load_error", error);
    event.addInt("ino", ino);
    event.addBool("caused_by_x2p", causedByX2P);
  }

  const char* getType() const override {
    return "inode_loading_failed";
  }
};

struct EMenuStartupFailure : public EdenFSEvent {
  std::string reason;

  explicit EMenuStartupFailure(std::string reason)
      : reason(std::move(reason)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("reason", reason);
  }

  const char* getType() const override {
    return "emenu_startup_failure";
  }
};

struct PrjFSFileNotificationFailure : public EdenFSEvent {
  std::string reason;
  std::string path;

  PrjFSFileNotificationFailure(std::string reason, std::string path)
      : reason(std::move(reason)), path(std::move(path)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("reason", reason);
    event.addString("path", path);
  }

  const char* getType() const override {
    return "prjfs_file_notification_failure";
  }
};

struct PrjFSCheckoutReadRace : public EdenFSEvent {
  std::string client_cmdline;

  explicit PrjFSCheckoutReadRace(std::string client_cmdline)
      : client_cmdline(std::move(client_cmdline)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("client_cmdline", client_cmdline);
  }

  const char* getType() const override {
    return "prjfs_checkout_read_race";
  }
};

struct WorkingCopyGc : public EdenFSEvent {
  double duration = 0.0;
  int64_t numInvalidated = 0;
  bool success = false;
  int64_t numOfDeletedInodes = 0;

  WorkingCopyGc(
      double duration,
      int64_t numInvalidated,
      bool success,
      int64_t numOfDeletedInodes)
      : duration(duration),
        numInvalidated(numInvalidated),
        success(success),
        numOfDeletedInodes(numOfDeletedInodes) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addInt("num_invalidated", numInvalidated);
    event.addBool("success", success);
    event.addInt("num_deleted_inodes", numOfDeletedInodes);
  }

  const char* getType() const override {
    return "working_copy_gc";
  }
};

struct SilentDaemonExit : public EdenFSEvent {
  uint64_t last_daemon_heartbeat = 0;
  uint8_t daemon_exit_signal = 0;
  uint64_t last_mac_boot_timestamp = 0;

  SilentDaemonExit(
      uint64_t last_daemon_heartbeat,
      uint8_t daemon_exit_signal,
      uint64_t last_mac_boot_timestamp = 0)
      : last_daemon_heartbeat(last_daemon_heartbeat),
        daemon_exit_signal(daemon_exit_signal),
        last_mac_boot_timestamp(last_mac_boot_timestamp) {}

  void populate(DynamicEvent& event) const override {
    event.addInt("last_daemon_heartbeat", last_daemon_heartbeat);
    event.addInt("exit_signal", daemon_exit_signal);
    event.addInt("last_mac_boot_timestamp", last_mac_boot_timestamp);
  }

  const char* getType() const override {
    return "silent_daemon_exit";
  }
};

struct AccidentalUnmountRecovery : public EdenFSEvent {
  std::string error;
  bool success = false;
  std::string repo_source;

  AccidentalUnmountRecovery(
      std::string error,
      bool success,
      std::string repo_source)
      : error(std::move(error)),
        success(success),
        repo_source(std::move(repo_source)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("remount_error", error);
    event.addBool("success", success);
    event.addString("repo_source", repo_source);
  }

  const char* getType() const override {
    return "accidental_unmount_recovery";
  }
};

struct SqliteIntegrityCheck : public EdenFSEvent {
  double duration = 0.0;
  int64_t numErrors = 0;

  SqliteIntegrityCheck(double duration, int64_t numErrors)
      : duration(duration), numErrors(numErrors) {}

  void populate(DynamicEvent& event) const override {
    event.addDouble("duration", duration);
    event.addInt("num_errors", numErrors);
  }

  const char* getType() const override {
    return "sqlite_integrity_check";
  }
};

struct NfsCrawlDetected : public EdenFSEvent {
  int64_t readCount = 0;
  int64_t readThreshold = 0;
  int64_t readDirCount = 0;
  int64_t readDirThreshold = 0;
  // root->leaf formatted as:
  //   "[simple_name (pid): full_name] -> [simple_name (pid): full_name] -> ..."
  std::string processHierarchy;

  NfsCrawlDetected(
      int64_t readCount,
      int64_t readThreshold,
      int64_t readDirCount,
      int64_t readDirThreshold,
      std::string processHierarchy)
      : readCount(readCount),
        readThreshold(readThreshold),
        readDirCount(readDirCount),
        readDirThreshold(readDirThreshold),
        processHierarchy(std::move(processHierarchy)) {}

  void populate(DynamicEvent& event) const override {
    event.addInt("read_count", readCount);
    event.addInt("read_threshold", readThreshold);
    event.addInt("readdir_count", readDirCount);
    event.addInt("readdir_threshold", readDirThreshold);
    event.addString("process_hierarchy", processHierarchy);
  }

  const char* getType() const override {
    return "nfs_crawl_detected";
  }
};

struct FetchMiss : public EdenFSEvent {
  enum MissType : uint8_t {
    Tree = 0,
    Blob = 1,
    BlobAuxData = 2,
    TreeAuxData = 3
  };

  std::string_view missTypeToString(MissType miss) const {
    switch (miss) {
      case Tree:
        return "Tree";
      case Blob:
        return "Blob";
      case BlobAuxData:
        return "BlobAuxData";
      case TreeAuxData:
        return "TreeAuxData";
      default:
        return "Unknown";
    }
  }

  std::string_view repo_source;
  MissType miss_type;
  std::string reason;
  bool retry;
  bool dogfooding_host;

  FetchMiss(
      std::string_view repo_source,
      MissType miss_type,
      std::string reason,
      bool retry,
      bool dogfooding_host)
      : repo_source(repo_source),
        miss_type(miss_type),
        reason(std::move(reason)),
        retry(retry),
        dogfooding_host(dogfooding_host) {}

  void populate(DynamicEvent& event) const override {
    event.addString("repo_source", std::string(repo_source));
    if (miss_type == Tree) {
      event.addString("miss_type", "tree");
    } else if (miss_type == Blob) {
      event.addString("miss_type", "blob");
    } else if (miss_type == BlobAuxData) {
      event.addString("miss_type", "blob_aux");
    } else if (miss_type == TreeAuxData) {
      event.addString("miss_type", "tree_aux");
    } else {
      throw std::range_error(
          fmt::format("Unknown miss type: {}", missTypeToString(miss_type)));
    }
    event.addString("reason", reason);
    event.addBool("retry", retry);
    event.addBool("dogfooding_host", dogfooding_host);
  }

  const char* getType() const override {
    return "fetch_miss";
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
struct ManyLiveFsChannelRequests : public EdenFSEvent {
  void populate(DynamicEvent& /*event*/) const override {}

  const char* getType() const override {
    return "high_fschannel_requests";
  }
};

/**
 * Used to log user actions on e-Menu
 */
struct EMenuActionEvent : public EdenFSEvent {
  enum ActionType : uint8_t { EMenuClick = 0 };

  ActionType action_type;

  explicit EMenuActionEvent(ActionType action_type)
      : action_type(action_type) {}

  void populate(DynamicEvent& event) const override {
    if (action_type == EMenuClick) {
      event.addString("action_type", "EMenuClick");
    }
  }

  const char* getType() const override {
    return "e_menu_action_events";
  }
};

/**
 * Indicates that a FS request (through FUSE, NFS, PrjFS) took longer than a set
 * threshold.
 */
struct LongRunningFSRequest : public EdenFSEvent {
  double duration = 0.0;
  std::string causeDetail;

  LongRunningFSRequest(double duration, std::string_view detail)
      : duration(duration), causeDetail(detail) {}

  void populate(DynamicEvent& event) const override {
    // Duration in nanoseconds
    event.addDouble("duration", duration);
    event.addString("causeDetail", causeDetail);
  }

  const char* getType() const override {
    return "long_running_fs_request";
  }
};

struct FileAccessEvent : public EdenFSFileAccessEvent {
  std::string repo;
  std::string directory;
  std::string filename;
  std::string source;
  std::string source_detail;

  FileAccessEvent(
      std::string repo,
      std::string directory,
      std::string filename,
      std::string source,
      std::string source_detail)
      : repo(std::move(repo)),
        directory(std::move(directory)),
        filename(std::move(filename)),
        source(std::move(source)),
        source_detail(std::move(source_detail)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("repo", repo);
    event.addString("directory", directory);
    event.addString("filename", filename);
    event.addString("source", source);
    event.addString("source_detail", source_detail);
  }
};

struct CheckoutUpdateError : public EdenFSEvent {
  std::string path;
  std::string reason;

  CheckoutUpdateError(std::string path, std::string reason)
      : path(std::move(path)), reason(std::move(reason)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("path", path);
    event.addString("reason", reason);
  }

  const char* getType() const override {
    return "checkout_update_error";
  }
};

struct ChangesSince : public EdenFSEvent {
  std::string client_cmdline;
  std::string position;
  std::string mount;
  std::string root;
  std::vector<std::string> included_roots;
  std::vector<std::string> excluded_roots;
  std::vector<std::string> included_suffixes;
  std::vector<std::string> excluded_suffixes;
  bool include_vcs;
  uint64_t num_small_changes;
  uint64_t num_state_changes;
  uint64_t num_renamed_directories;
  uint64_t num_commit_transitions;
  std::optional<uint64_t> lost_changes;
  uint64_t num_filtered_changes;

  ChangesSince(
      std::string client_cmdline,
      std::string position,
      std::string mount,
      std::string root,
      std::vector<std::string> included_roots,
      std::vector<std::string> excluded_roots,
      std::vector<std::string> included_suffixes,
      std::vector<std::string> excluded_suffixes,
      bool include_vcs,
      uint64_t num_small_changes,
      uint64_t num_state_changes,
      uint64_t num_renamed_directories,
      uint64_t num_commit_transitions,
      std::optional<uint64_t> lost_changes,
      uint64_t num_filtered_changes)
      : client_cmdline(std::move(client_cmdline)),
        position(std::move(position)),
        mount(std::move(mount)),
        root(std::move(root)),
        included_roots(std::move(included_roots)),
        excluded_roots(std::move(excluded_roots)),
        included_suffixes(std::move(included_suffixes)),
        excluded_suffixes(std::move(excluded_suffixes)),
        include_vcs(include_vcs),
        num_small_changes(num_small_changes),
        num_state_changes(num_state_changes),
        num_renamed_directories(num_renamed_directories),
        num_commit_transitions(num_commit_transitions),
        lost_changes(lost_changes),
        num_filtered_changes(num_filtered_changes) {}

  void populate(DynamicEvent& event) const override {
    event.addString("client_cmdline", client_cmdline);
    event.addString("position", position);
    event.addString("mount", mount);
    event.addString("root", root);
    event.addStringVec("included_roots", included_roots);
    event.addStringVec("excluded_roots", excluded_roots);
    event.addStringVec("included_suffixes", included_suffixes);
    event.addStringVec("excluded_suffixes", excluded_suffixes);
    event.addBool("include_vcs", include_vcs);
    event.addInt("num_small_changes", num_small_changes);
    event.addInt("num_state_changes", num_state_changes);
    event.addInt("num_renamed_directory", num_renamed_directories);
    event.addInt("num_commit_transition", num_commit_transitions);
    if (lost_changes.has_value()) {
      event.addInt("lost_changes", lost_changes.value());
    }
    event.addInt("num_filtered_changes", num_filtered_changes);
  }

  const char* getType() const override {
    return "changes_since";
  }
};

} // namespace facebook::eden
