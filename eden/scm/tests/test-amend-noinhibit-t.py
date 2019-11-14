# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Set up test environment.
sh % "cat" << r"""
[extensions]
amend=
rebase=
[experimental]
evolution = createmarkers
""" >> "$HGRCPATH"
sh % "hg init repo"
sh % "cd repo"

# Perform restack without inhibit extension.
sh % "hg debugbuilddag -m +3"
sh % "hg update 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg amend -m Amended --no-rebase" == r"""
    hint[amend-restack]: descendants of c05912b45f80 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg rebase --restack" == 'rebasing 27d2178f63cc "r2"'
