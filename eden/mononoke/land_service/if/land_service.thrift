/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"

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
} (rust.exhaustive)

struct BonsaiHashPairs {
  /// The old bonsai hash.
  1: ChangesetId old_id;

  /// The new bonsai hash.
  2: ChangesetId new_id;
} (rust.exhaustive)

struct PushrebaseOutcome {
  /// The new changeset for the rebased head.
  1: ChangesetId head;

  /// A list of bonsai hash (changeset) pairs represeting old and new ids.
  2: list<BonsaiHashPairs> rebased_commits;

  /// How far away was the commit rebased.
  3: i64 pushrebase_distance;

  /// How many retries it took to do the rebase successfully, due to race conditions.
  4: i64 retry_num;

  /// The old id where the bookmark was before the pushrebase operation.
  5: optional ChangesetId old_bookmark_value;
} (rust.exhaustive)

struct LandChangesetsResponse {
  1: PushrebaseOutcome pushrebase_outcome;
} (rust.exhaustive)

struct PushrebaseConflicts {
  1: Path left;
  2: Path right;
} (rust.exhaustive)

safe permanent client exception PushrebaseConflictsException {
  1: string reason;
  /// Always non-empty
  2: list<PushrebaseConflicts> conflicts;
} (message = "reason")

struct HookRejection {
  /// The hook that rejected the output
  1: string hook_name;
  /// The changeset that was reject, in bonsai format.
  2: ChangesetId cs_id;
  /// Why the hook rejected the changeset.
  3: HookOutcomeRejected reason;
}

struct HookOutcomeRejected {
  /// A short description for summarizing this failure with similar failures
  1: string description;
  /// A full explanation of what went wrong, suitable for presenting to the user (should include guidance for fixing this failure, where possible)
  2: string long_description;
} (rust.exhaustive)

safe stateful client exception HookRejectionsException {
  1: string reason;
  /// Always non-empty
  2: list<HookRejection> rejections;
} (message = "reason")

service LandService extends fb303_core.BaseService {
  /// Land a stack of commits via land_changesets.
  LandChangesetsResponse land_changesets(
    1: LandChangesetRequest land_changesets,
  ) throws (
    2: PushrebaseConflictsException pushrebase_conflicts,
    3: HookRejectionsException hook_rejections,
  );
}
