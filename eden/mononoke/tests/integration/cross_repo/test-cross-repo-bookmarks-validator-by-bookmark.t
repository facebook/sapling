# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:bookmarks_validator_prefix_polling": true
  >   },
  >   "ints": {
  >     "scm/mononoke:bookmarks_validator_max_log_records": 1,
  >     "scm/mononoke:bookmarks_validator_sleep_ms": 500
  >   }
  > }
  > EOF

  $ XREPOSYNC=1 init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

Create and forward-sync a bookmark that will later be deleted only from the
source repository.

  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_C
  > # exists: S_B $S_B
  > # message: S_C "bookmark to delete"
  > # modify: S_C deleted_file "deleted"
  > # bookmark: S_C deleted-source
  > EOF
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once > "$TESTTMP/xrepo-sync.out" 2>&1
  $ mononoke_admin bookmarks -R large-mon get bookprefix/deleted-source > /dev/null

  $ quiet enable_pushredirect "$SMALL_REPO_ID"

  $ start_bookmarks_validator() {
  >   "$BOOKMARKS_VALIDATOR" \
  >     "${CACHE_ARGS[@]}" \
  >     "${COMMON_ARGS[@]}" \
  >     --debug \
  >     --source-repo-id "$LARGE_REPO_ID" \
  >     --target-repo-id "$SMALL_REPO_ID" \
  >     --mononoke-config-path "$TESTTMP/mononoke-config" \
  >     --scuba-log-file "$TESTTMP/bookmarks-validator-scuba.json" \
  >     > "$TESTTMP/bookmarks-validator.out" 2>&1 &
  >   BOOKMARKS_VALIDATOR_PID=$!
  >   echo "$BOOKMARKS_VALIDATOR_PID" >> "$DAEMON_PIDS"
  > }

  $ wait_for_validator_log() {
  >   local pattern="$1"
  >   for _ in $(seq 1 300); do
  >     grep -q "$pattern" "$TESTTMP/bookmarks-validator.out" && return 0
  >     sleep 0.1
  >   done
  >   tail -100 "$TESTTMP/bookmarks-validator.out"
  >   return 1
  > }

With the JK enabled, the validator discovers and validates the common bookmark
and the independently forward-synced bookmark.

  $ start_bookmarks_validator
  $ wait_for_validator_log "validating 2 bookmarks"

A source bookmark that is absent from the target is reported independently.

  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_D
  > # exists: S_B $S_B
  > # message: S_D "missing target bookmark"
  > # modify: S_D missing_file "missing"
  > # bookmark: S_D missing-target
  > EOF
  $ quiet testtool_drawdag -R large-mon <<EOF
  > L_C-L_D
  > # exists: L_C $L_C
  > # message: L_D "mapped commit without bookmark"
  > # modify: L_D smallrepofolder/missing_file "missing"
  > EOF
  $ add_synced_commit_mapping_entry "$SMALL_REPO_ID" "$S_D" "$LARGE_REPO_ID" "$L_D" test_version
  $ wait_for_validator_log "validation failed for missing-target: bookprefix/missing-target points to None in large-mon"
  $ grep -m 1 "validation failed for missing-target:" "$TESTTMP/bookmarks-validator.out"
  *validation failed for missing-target: bookprefix/missing-target points to None in large-mon, but points to Some(*) in small-mon (glob)

A target-only bookmark is discovered as a source-side deletion and reported by
the same per-bookmark validation path.

  $ reset_pushredirect
  $ quiet mononoke_admin bookmarks -R small-mon delete deleted-source
  $ quiet enable_pushredirect "$SMALL_REPO_ID"
  $ wait_for_validator_log "validation failed for deleted-source: bookprefix/deleted-source points to Some"
  $ grep -m 1 "validation failed for deleted-source:" "$TESTTMP/bookmarks-validator.out"
  *validation failed for deleted-source: bookprefix/deleted-source points to Some(*) in large-mon, but points to None in small-mon (glob)
