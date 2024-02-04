#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Setup

  $ enable fbcodereview smartlog amend
  $ hg init repo
  $ cd repo
  $ echo 1 > foo
  $ hg ci -qAm 'base'
  $ echo 2 > foo
  $ hg amend -qm 'Differential Revision: https://phabricator.fb.com/D1'
  $ echo 3 > foo
  $ hg amend -q
  $ echo 4 > foo
  $ hg amend -q
  $ echo 5 > foo
  $ hg amend -q

# With an invalid arc configuration

  $ hg log -T '{diffversion}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: no .arcconfig found
  Error

# Configure arc...

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

# And now with bad responses:

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{diffversion}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: Unexpected graphql response format
  Error

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"errors": [{"message": "failed, yo"}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{diffversion}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yo
  Error

# Test diff version matches all hashes.

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "ceef473a2ce1e50f05341e7b9f7ad8b4335248ac"
  >   },
  >   "phabricator_versions": {
  >     "nodes": [
  >       {
  >         "ordinal_label": {"abbreviated": "V1"},
  >         "commit_hash_best_effort": "ceef473a2ce1e50f05341e7b9f7ad8b4335248ac"
  >       }
  >     ]
  >   },
  >   "unpublished_phabricator_versions": [{
  >     "phabricator_version_migration": {
  >         "ordinal_label": {"abbreviated": "V0.1"},
  >         "commit_hash_best_effort": "fb01aa127551402e09df1d6e976e8ecea4983ea3"
  >       }
  >   }],
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{node|short}: {diffversion}\n' -r 'predecessors(.)'
  e08c689390e1: 
  fb01aa127551: V0.1
  aa5c3457d4ab: V0.1 (+ local changes)
  ceef473a2ce1: V1 (latest)
  589e1c5df135: V1 (latest + local changes)
