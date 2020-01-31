# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Set up extension
sh % "cat" << r"""
[extensions]
debugcommitmessage=
""" >> "$HGRCPATH"

# Set up repo
sh % "hg init repo"
sh % "cd repo"

# Test extension
sh % "hg debugcommitmessage" == r"""


    HG: Enter commit message.  Lines beginning with 'HG:' are removed.
    HG: Leave message empty to abort commit.
    HG: --
    HG: user: test
    HG: branch 'default'
    HG: no files changed"""
sh % "hg debugcommitmessage --config 'committemplate.changeset.commit.normal.normal=Test Specific Message\\n'" == "Test Specific Message"
sh % "hg debugcommitmessage --config 'committemplate.changeset.commit=Test Generic Message\\n'" == "Test Generic Message"
sh % "hg debugcommitmessage commit.amend.normal --config 'committemplate.changeset.commit=Test Generic Message\\n'" == "Test Generic Message"
sh % "hg debugcommitmessage randomform --config 'committemplate.changeset.commit=Test Generic Message\\n'" == r"""


    HG: Enter commit message.  Lines beginning with 'HG:' are removed.
    HG: Leave message empty to abort commit.
    HG: --
    HG: user: test
    HG: branch 'default'
    HG: no files changed"""
