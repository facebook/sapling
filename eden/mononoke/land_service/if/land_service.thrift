/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"
include "thrift/annotation/thrift.thrift"
include "thrift/annotation/rust.thrift"

typedef binary ChangesetId
/// The UTF-8 path of the file or directory.
typedef string Path

enum CrossRepoPushSource {
  NATIVE_TO_THIS_REPO = 0,
  PUSH_REDIRECTED = 1,
}

enum BookmarkKindRestrictions {
  ANY_KIND = 0,
  ONLY_SCRATCH = 1,
  ONLY_PUBLISHING = 2,
}

@rust.Exhaustive
struct LandChangesetRequest {
  /// The name of the bookmark to land to.
  1: string bookmark;

  /// The set of changesets to be landed.
  /// MUST BE A STACK
  2: set<ChangesetId> changesets;

  /// The pushvars to use when landing the stack.
  3: optional map<string, binary> pushvars;

  /// Override push source. Leave as the default.
  4: CrossRepoPushSource cross_repo_push_source = CrossRepoPushSource.NATIVE_TO_THIS_REPO;

  /// What kind of bookmark can be pushed.
  5: BookmarkKindRestrictions bookmark_restrictions = BookmarkKindRestrictions.ANY_KIND;

  /// The name of the repository.
  6: string repo_name;

  /// Whether to log new public commits
  7: bool log_new_public_commits_to_scribe;

  /// Service identity to use for this commit creation.
  8: optional string service_identity;
}

@rust.Exhaustive
struct BonsaiHashPairs {
  /// The old bonsai hash.
  1: ChangesetId old_id;

  /// The new bonsai hash.
  2: ChangesetId new_id;
}

@rust.Exhaustive
struct PushrebaseOutcome {
  /// The new changeset for the rebased head.
  1: ChangesetId head;

  /// A list of bonsai hash (changeset) pairs representing old and new ids.
  2: list<BonsaiHashPairs> rebased_changesets;

  /// How far away was the commit rebased.
  3: i64 pushrebase_distance;

  /// How many retries it took to do the rebase successfully, due to race conditions.
  4: i64 retry_num;

  /// The old id where the bookmark was before the pushrebase operation.
  5: optional ChangesetId old_bookmark_value;

  /// The id for the entry in the bookmark update log where the bookmark was written
  6: optional i64 log_id;
}

@rust.Exhaustive
struct LandChangesetsResponse {
  1: PushrebaseOutcome pushrebase_outcome;
}

@rust.Exhaustive
struct PushrebaseConflicts {
  1: Path left;
  2: Path right;
}

@rust.Exhaustive
safe permanent client exception PushrebaseConflictsException {
  @thrift.ExceptionMessage
  1: string reason;
  /// Always non-empty
  2: list<PushrebaseConflicts> conflicts;
}

@rust.Exhaustive
struct HookRejection {
  /// The hook that rejected the output
  1: string hook_name;
  /// The changeset that was reject, in bonsai format.
  2: ChangesetId cs_id;
  /// Why the hook rejected the changeset.
  3: HookOutcomeRejected reason;
}

@rust.Exhaustive
struct HookOutcomeRejected {
  /// A short description for summarizing this failure with similar failures
  1: string description;
  /// A full explanation of what went wrong, suitable for presenting to the user (should include guidance for fixing this failure, where possible)
  2: string long_description;
}

@rust.Exhaustive
safe stateful client exception HookRejectionsException {
  @thrift.ExceptionMessage
  1: string reason;
  /// Always non-empty
  2: list<HookRejection> rejections;
}

@rust.Exhaustive
safe client exception InternalError {
  @thrift.ExceptionMessage
  1: string reason;
  2: optional string backtrace;
  3: list<string> source_chain;
}

@rust.RequestContext
service LandService extends fb303_core.BaseService {
  /// Land a stack of commits via land_changesets.
  LandChangesetsResponse land_changesets(
    1: LandChangesetRequest land_changesets,
  ) throws (
    2: PushrebaseConflictsException pushrebase_conflicts,
    3: HookRejectionsException hook_rejections,
    4: InternalError internal_error,
  );
}
