# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Load extensions

sh % "cat" << r"""
[extensions]
arcconfig=$TESTDIR/../edenscm/hgext/extlib/phabricator/arcconfig.py
arcdiff=
""" >> "$HGRCPATH"

# Diff with no revision

sh % "hg init repo"
sh % "cd repo"
sh % "touch foo"
sh % "hg add foo"
sh % "hg ci -qm 'No rev'"
sh % "hg diff --since-last-submit" == r"""
    abort: local changeset is not associated with a differential revision
    [255]"""

sh % "hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == r"""
    abort: local changeset is not associated with a differential revision
    [255]"""

# Fake a diff

sh % "echo bleet" > "foo"
sh % "hg ci -qm 'Differential Revision: https://phabricator.fb.com/D1'"
sh % "hg diff --since-last-submit" == r"""
    abort: no .arcconfig found
    [255]"""

sh % "hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == r"""
    abort: no .arcconfig found
    [255]"""

# Prep configuration

sh % "echo '{}'" > ".arcrc"
sh % 'echo \'{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}\'' > ".arcconfig"

# Now progressively test the response handling for variations of missing data

sh % "cat" << r"""
[{}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit" == r"""
    Error calling graphql: Unexpected graphql response format
    abort: unable to determine previous changeset hash
    [255]"""

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == r"""
    Error calling graphql: Unexpected graphql response format
    abort: unable to determine previous changeset hash
    [255]"""

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Needs Review",
  "differential_diffs": {"count": 3},
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit" == r"""
    abort: unable to determine previous changeset hash
    [255]"""

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == r"""
    abort: unable to determine previous changeset hash
    [255]"""

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Needs Review",
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit" == r"""
    abort: unable to determine previous changeset hash
    [255]"""

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == r"""
    abort: unable to determine previous changeset hash
    [255]"""

# This is the case when the diff is up to date with the current commit;
# there is no diff since what was landed.

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Needs Review",
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"2e6531b7dada2a3e5638e136de05f51e94a427f4\"}}"}
      ]
    }
  },
  "differential_diffs": {"count": 1},
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit"

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == "2e6531b7dada2a3e5638e136de05f51e94a427f4 Differential Revision: https://phabricator.fb.com/D1"

# This is the case when the diff points at our parent commit, we expect to
# see the bleet text show up.  There's a fake hash that I've injected into
# the commit list returned from our mocked phabricator; it is present to
# assert that we order the commits consistently based on the time field.

sh % "cat" << r"""
[{"data": {"query": [{"results": {"nodes": [{
  "number": 1,
  "diff_status_name": "Needs Review",
  "latest_active_diff": {
    "local_commit_info": {
      "nodes": [
        {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"88dd5a13bf28b99853a24bddfc93d4c44e07c6bd\"}}"}
      ]
    }
  },
  "differential_diffs": {"count": 1},
  "is_landing": false,
  "created_time": 123,
  "updated_time": 222
}]}}]}}]
""" > "$TESTTMP/mockduit"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit --nodates" == r"""
    diff -r 88dd5a13bf28 -r 2e6531b7dada foo
    --- a/foo
    +++ b/foo
    @@ -0,0 +1,1 @@
    +bleet"""

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(.)' -T '{node} {desc}\\n'" == "88dd5a13bf28b99853a24bddfc93d4c44e07c6bd No rev"

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit-2o" == r"""
    Phabricator rev: 88dd5a13bf28b99853a24bddfc93d4c44e07c6bd
    Local rev: 2e6531b7dada2a3e5638e136de05f51e94a427f4 (.)
    Changed: foo
    | ...
    | +bleet"""

# Make a new commit on top, and then use -r to look at the previous commit
sh % "echo other" > "foo"
sh % "hg commit -m 'Other commmit'"
sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg diff --since-last-submit --nodates -r 2e6531b" == r"""
    diff -r 88dd5a13bf28 -r 2e6531b7dada foo
    --- a/foo
    +++ b/foo
    @@ -0,0 +1,1 @@
    +bleet"""

sh % "'HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit' hg log -r 'lastsubmitted(2e6531b)' -T '{node} {desc}\\n'" == "88dd5a13bf28b99853a24bddfc93d4c44e07c6bd No rev"
