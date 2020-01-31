# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
arcconfig=$TESTDIR/../edenscm/hgext/extlib/phabricator/arcconfig.py
""" >> "$HGRCPATH"

# Sanity check expectations when there is no arcconfig

sh % "hg init repo"
sh % "cd repo"
sh % "hg debugarcconfig" == r"""
    abort: no .arcconfig found
    [255]"""

# Show that we can locate and reflect the contents of the .arcconfig from
# the repo dir

sh % 'echo \'{"hello": "world"}\'' > ".arcconfig"
sh % "hg debugarcconfig" == '{"_arcconfig_path": "$TESTTMP/repo", "hello": "world"}'

# We expect to see the combination of the user arcrc and the repo rc

sh % "echo '{\"user\": true}'" > "$HOME/.arcrc"
sh % "hg debugarcconfig" == '{"_arcconfig_path": "$TESTTMP/repo", "hello": "world", "user": true}'
