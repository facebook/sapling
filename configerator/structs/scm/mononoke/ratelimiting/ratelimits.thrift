// @generated SignedSource<<06781c42b9f358cf01d26fbcdc8f2639>>
// DO NOT EDIT THIS FILE MANUALLY!
// This file is a mechanical copy of the version in the configerator repo. To
// modify it, edit the copy in the configerator repo instead and copy it over by
// running the following in your fbcode directory:
//
// configerator-thrift-updater scm/mononoke/ratelimiting/ratelimits.thrift
include "thrift/annotation/cpp.thrift"
include "thrift/annotation/rust.thrift"

typedef i64 RepoId

// Which clients should the rate limiting or load shedding rule apply to?
//
// Multiple Targets can be combined to express specific clients, such as
// "10% of sandcastle and mactest hosts".
union Target {
  // Invert the enclosed Target
  @cpp.Ref{type = cpp.RefType.Unique}
  @rust.Box
  1: Target not_target;
  // Apply this associated rule if the client matches all Targets
  2: list<Target> and_target;
  // Apply this associated rule if the client matches any Targets
  3: list<Target> or_target;
  // A client's identity, such as MACHINE_TIER:sandcastle
  4: string identity;
  // A static slice of hosts that are chosen by hashing the client's hostname
  5: StaticSlice static_slice;
}

@rust.Exhaustive
struct StaticSlice {
  // The percentage of hosts this slice applies to. 0 <= slice_pct <= 100
  1: i32 slice_pct;
  // The nonce can be used to rotate hosts in a slice
  2: string nonce;
}

enum RateLimitStatus {
  // Don't run this code at all.
  Disabled = 0,
  // Track this limit, but don't enforce it.
  Tracked = 1,
  // Enforce this limit.
  Enforced = 2,
}

// RegionalMetrics are tracked and contributed to by all servers in a region.
// The counters are explicitly bumped by Mononoke's code and are backed by SCS.
enum RegionalMetric {
  // The amount of bytes egressed by Mononoke servers
  EgressBytes = 0,
  // The number of manifests served
  TotalManifests = 1,
  // The number of files served
  GetpackFiles = 2,
  // The number of commits served
  Commits = 3,
}

@rust.Exhaustive
struct RateLimitBody {
  // Whether the rate limit is enabled
  1: RateLimitStatus status;
  // The limit above which requests will be rate limited
  2: double limit;
  // The window over which to count the metric
  3: i64 window;
}

@rust.Exhaustive
struct RateLimit {
  // The regional metric to monitor
  1: RegionalMetric metric;
  // The target of the RateLimit. If this is null then the RateLimit will
  // apply to all clients
  2: optional Target target;
  3: RateLimitBody limit;
}

@rust.Exhaustive
struct LoadShedLimit {
  // The key used to loadshed, such as
  // "mononoke.lfs.download.size_bytes_sent.sum.15"
  1: string metric;
  // Whether the rate limit is enabled
  2: RateLimitStatus status;
  // The target of the RateLimit. If this is null then the RateLimit will
  // apply to all clients
  3: optional Target target;
  // The limit above which requests will be rate limited
  4: i64 limit;
}

@rust.Exhaustive
struct MononokeRateLimits {
  // The RateLimits that should be checked
  1: list<RateLimit> rate_limits;
  // The LoadShedLimits that should be checked
  2: list<LoadShedLimit> load_shed_limits;
  // The number of servers in each region, e.g.
  // {"frc: 60}
  3: map<string, i32> datacenter_prefix_capacity;
  // A rate limit for the number of commits that a single author can land
  4: RateLimitBody commits_per_author;
  // A rate limit for the number of files that can be changed
  5: optional RateLimitBody total_file_changes;
}