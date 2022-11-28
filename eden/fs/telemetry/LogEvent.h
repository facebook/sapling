/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <string>
#include <unordered_map>

#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook::eden {

class DynamicEvent {
 public:
  using IntMap = std::unordered_map<std::string, int64_t>;
  using StringMap = std::unordered_map<std::string, std::string>;
  using DoubleMap = std::unordered_map<std::string, double>;

  DynamicEvent() = default;
  DynamicEvent(const DynamicEvent&) = default;
  DynamicEvent(DynamicEvent&&) = default;
  DynamicEvent& operator=(const DynamicEvent&) = default;
  DynamicEvent& operator=(DynamicEvent&&) = default;

  void addInt(std::string name, int64_t value);
  void addString(std::string name, std::string value);
  void addDouble(std::string name, double value);

  /**
   * Convenience function that adds boolean values as integer 0 or 1.
   */
  void addBool(std::string name, bool value) {
    addInt(std::move(name), value);
  }

  const IntMap& getIntMap() const {
    return ints_;
  }
  const StringMap& getStringMap() const {
    return strings_;
  }
  const DoubleMap& getDoubleMap() const {
    return doubles_;
  }

 private:
  // Due to limitations in the underlying log database, limit the field types to
  // int64_t, double, string, and vector<string>
  // TODO: add vector<string> support if needed.
  IntMap ints_;
  StringMap strings_;
  DoubleMap doubles_;
};

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

struct MissingProxyHash {
  static constexpr const char* type = "missing_proxy_hash";

  void populate(DynamicEvent&) const {}
};

struct FetchHeavy {
  static constexpr const char* type = "fetch_heavy";

  std::string client_cmdline;
  pid_t pid;
  uint64_t fetch_count;

  void populate(DynamicEvent& event) const {
    event.addString("client_cmdline", client_cmdline);
    event.addInt("client_pid", pid);
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
  int64_t numConflicts = 0;

  void populate(DynamicEvent& event) const {
    event.addString("mode", mode);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addInt("fetched_trees", fetchedTrees);
    event.addInt("fetched_blobs", fetchedBlobs);
    event.addInt("num_conflicts", numConflicts);
  }
};

struct FinishedMount {
  static constexpr const char* type = "mount";

  std::string repo_type;
  std::string repo_source;
  std::string fs_channel_type;
  bool is_takeover = false;
  double duration = 0.0;
  bool success = false;
  bool clean = false;

  void populate(DynamicEvent& event) const {
    event.addString("repo_type", repo_type);
    event.addString("repo_source", repo_source);
    event.addString("fs_channel_type", fs_channel_type);
    event.addBool("is_takeover", is_takeover);
    event.addDouble("duration", duration);
    event.addBool("success", success);
    event.addBool("clean", clean);
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
  std::optional<pid_t> client_pid;
  std::optional<std::string> client_cmdline;
  std::string fetched_path;
  std::string fetched_object_type;

  void populate(DynamicEvent& event) const {
    event.addString("interface", cause);
    if (client_pid) {
      event.addInt("client_pid", client_pid.value());
    }
    if (client_cmdline) {
      event.addString("client_cmdline", client_cmdline.value());
    }
    event.addString("fetched_path", fetched_path);
    event.addString("fetched_object_type", fetched_object_type);
  }
};

struct EdenApiMiss {
  enum MissType : bool {
    Blob = 0,
    Tree = 1,
  };

  static constexpr const char* type = "edenapi_miss";

  std::string repo_name;
  MissType miss_type;
  std::string path;
  std::string hash;

  void populate(DynamicEvent& event) const {
    event.addString("repo_source", repo_name);
    if (miss_type == Blob) {
      event.addString("edenapi_miss_type", "blob");
    } else {
      event.addString("edenapi_miss_type", "tree");
    }
    event.addString("path", path);
    event.addString("hash", hash);
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

} // namespace facebook::eden
