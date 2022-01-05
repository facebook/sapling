# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import sh, testtmp  # noqa: F401


(
    sh % "cat"
    << r"""
[extensions]
preventpremegarepoupdateshook=
"""
    >> "$HGRCPATH"
)


sh % "hg init repo" == ""
sh % "cd repo" == ""
sh % "touch a" == ""
sh % "hg commit -A -m pre_megarepo_commit" == "adding a"
sh % "mkdir .megarepo" == ""
sh % "touch .megarepo/remapping_state" == ""
sh % "hg commit -A -m megarepo_merge" == "adding .megarepo/remapping_state"
sh % "touch b" == ""
sh % "hg commit -A -m another_commit" == "adding b"


sh % "hg update .^" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg update --config ui.interactive=true .^" << r"""n
""" == r"""
    Checking out commits from before megarepo merge is discouraged. The resulting checkout will contain just the contents of one git subrepo. Many tools might not work as expected. Do you want to continue (Yn)?   n
    abort: preupdate.preventpremegarepoupdates hook failed
    [255]"""

sh % "hg update --config ui.interactive=false .^" == r"""
    Checking out commits from before megarepo merge is discouraged. The resulting checkout will contain just the contents of one git subrepo. Many tools might not work as expected. Do you want to continue (Yn)?   y
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""

sh % "hg update tip^" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "HGPLAIN=1 hg update --config ui.interactive=true .^" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
