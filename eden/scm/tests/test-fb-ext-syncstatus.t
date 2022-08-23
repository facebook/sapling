#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Setup

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > arcconfig=$TESTDIR/../edenscm/ext/extlib/phabricator/arcconfig.py
  > phabstatus=
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
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3},
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: Unexpected graphql response format
  Error

# Missing count field is treated as an error

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   },
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: Unexpected graphql response format
  Error

# Missing hash doesn't make us explode

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3},
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
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"commit\": \"ffffff\", \"time\": 0}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3},
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabcommit}\n' -r .
  ffffff

# Missing hash field is treated as unsync

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3},
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  unsync

# And finally, the success case

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Committed",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3},
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  committed
