#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Setup

  $ eagerepo
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > arcconfig=$TESTDIR/../sapling/ext/extlib/phabricator/arcconfig.py
  > fbcodereview=
  > smartlog=
  > EOF
  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg ci -qAm 'Differential Revision: https://phabricator.fb.com/D1'

# With an invalid arc configuration

  $ hg log -T '{syncstatus}\n' -r .
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
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: Unexpected graphql response format
  Error

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"errors": [{"message": "failed, yo"}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yo
  Error

# Missing status field is treated as an error

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "abcd"
  >   },
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: Unexpected graphql response format for D1
  Error

# Missing hash doesn't make us explode

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabcommit}\n' -r .

# Hash field displayed

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "abcd"
  >   },
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabcommit}\n' -r .
  abcd

# Matching hash is sync

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "c4f28933f13b414e18aa5896ec9e86b0a7c85c6c"
  >   },
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  sync

# Non-matching hash is unsync

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "abcd"
  >   },
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  unsync

# Missing hash field is treated as unsync

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  unsync

# Non-matching hash when committed shows as committed

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Committed",
  >   "latest_active_phabricator_version": {
  >     "commit_hash_best_effort": "abcd"
  >   },
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  committed
