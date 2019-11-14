# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Setup

sh % "cat" << r"""
[extensions]
arcconfig=$TESTDIR/../edenscm/hgext/extlib/phabricator/arcconfig.py
phabstatus=
smartlog=
""" >> "$HGRCPATH"
sh % "hg init repo"
sh % "cd repo"
sh % "touch foo"
sh % "hg ci -qAm 'Differential Revision: https://phabricator.fb.com/D1'"

# With an invalid arc configuration

sh % "hg log -T '{syncstatus}\\n' -r ." == r"""
    arcconfig configuration problem. No diff information can be provided.
    Error info: no .arcconfig found
    Error"""

# Configure arc...

sh % "echo '{}'" > ".arcrc"
sh % 'echo \'{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}\'' > ".arcconfig"

# And now with bad responses:

sh % "cat" << r"""
[{}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == r"""
    Error talking to phabricator. No diff information can be provided.
    Error info: Unexpected graphql response format
    Error"""

sh % "cat" << r"""
[{"errors": [{"message": "failed, yo"}]}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == r"""
    Error talking to phabricator. No diff information can be provided.
    Error info: failed, yo
    Error"""

# Missing status field is treated as an error

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
      ]
    }
  },
  "differential_diffs": {"count": 3},
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == r"""
    Error talking to phabricator. No diff information can be provided.
    Error info: Unexpected graphql response format
    Error"""

# Missing count field is treated as an error

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Approved",
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
      ]
    }
  },
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == r"""
    Error talking to phabricator. No diff information can be provided.
    Error info: Unexpected graphql response format
    Error"""

# Missing hash field is treated as unsync

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Approved",
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0}}"}
      ]
    }
  },
  "differential_diffs": {"count": 3},
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == "unsync"

# And finally, the success case

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Committed",
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
      ]
    }
  },
  "differential_diffs": {"count": 3},
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -T '{syncstatus}\\n' -r ." == "committed"
