# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Test changesets filtering during exchanges (some tests are still in
# test-obsolete.t)

sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> "$HGRCPATH"

# Push does not corrupt remote
# ----------------------------

# Create a DAG where a changeset reuses a revision from a file first used in an
# extinct changeset.

sh % "hg init local"
sh % "cd local"
sh % "echo base" > "base"
sh % "hg commit -Am base" == "adding base"
sh % "echo A" > "A"
sh % "hg commit -Am A" == "adding A"
sh % "hg up 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg revert -ar 1" == "adding A"
sh % "hg commit -Am 'A'\\'''"
sh % "hg log -G '--template={desc} {node}'" == r"""
    @  A' f89bcc95eba5174b1ccc3e33a82e84c96e8338ee
    |
    | o  A 9d73aac1b2ed7d53835eaeec212ed41ea47da53a
    |/
    o  base d20a80d4def38df63a4b330b7fb688f3d4cae1e3"""
sh % "hg debugobsolete 9d73aac1b2ed7d53835eaeec212ed41ea47da53a f89bcc95eba5174b1ccc3e33a82e84c96e8338ee" == "obsoleted 1 changesets"

# Push it. The bundle should not refer to the extinct changeset.

sh % "hg init ../other"
sh % "hg push ../other" == r"""
    pushing to ../other
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 2 changesets with 2 changes to 2 files"""
sh % "hg -R ../other verify" == r"""
    checking changesets
    checking manifests
    crosschecking files in changesets and manifests
    checking files
    2 files, 2 changesets, 2 total revisions"""

# Adding a changeset going extinct locally
# ------------------------------------------

# Pull a changeset that will immediatly goes extinct (because you already have a
# marker to obsolete him)
# (test resolution of issue3788)

sh % "hg phase --draft --force f89bcc95eba5"
sh % "hg phase -R ../other --draft --force f89bcc95eba5"
sh % "hg commit --amend -m 'A'\\'''\\'''"
sh % "hg --hidden debugstrip --no-backup f89bcc95eba5"
sh % "hg pull ../other" == r"""
    pulling from ../other
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 1 files
    new changesets f89bcc95eba5"""

# check that bundle is not affected

sh % "hg bundle --hidden --rev f89bcc95eba5 --base 'f89bcc95eba5^' ../f89bcc95eba5.hg" == "1 changesets found"
sh % "hg --hidden debugstrip --no-backup f89bcc95eba5"
sh % "hg unbundle ../f89bcc95eba5.hg" == r"""
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 1 files"""

# check-that bundle can contain markers:

sh % "hg bundle --hidden --rev f89bcc95eba5 --base 'f89bcc95eba5^' ../f89bcc95eba5-obs.hg --config 'experimental.evolution.bundle-obsmarker=1'" == "1 changesets found"
sh % "hg debugbundle ../f89bcc95eba5.hg" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 1, version: 02}
        f89bcc95eba5174b1ccc3e33a82e84c96e8338ee"""
sh % "hg debugbundle ../f89bcc95eba5-obs.hg" == r"""
    Stream params: {Compression: BZ}
    changegroup -- {nbchanges: 1, version: 02}
        f89bcc95eba5174b1ccc3e33a82e84c96e8338ee
    obsmarkers -- {}
        version: 1 (70 bytes)
        9d73aac1b2ed7d53835eaeec212ed41ea47da53a f89bcc95eba5174b1ccc3e33a82e84c96e8338ee 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}"""

sh % "cd .."

# pull does not fetch excessive changesets when common node is hidden (issue4982)
# -------------------------------------------------------------------------------

# initial repo with server and client matching

sh % "hg init pull-hidden-common"
sh % "cd pull-hidden-common"
sh % "touch foo"
sh % "hg -q commit -A -m initial"
sh % "echo 1" > "foo"
sh % "hg commit -m 1"
sh % "echo 2a" > "foo"
sh % "hg commit -m 2a"
sh % "cd .."
sh % "hg clone --pull pull-hidden-common pull-hidden-common-client" == r"""
    requesting all changes
    adding changesets
    adding manifests
    adding file changes
    added 3 changesets with 3 changes to 1 files
    new changesets 96ee1d7354c4:6a29ed9c68de
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# server obsoletes the old head

sh % "cd pull-hidden-common"
sh % "hg -q up -r 1"
sh % "echo 2b" > "foo"
sh % "hg -q commit -m 2b"
sh % "hg debugobsolete 6a29ed9c68defff1a139e5c6fa9696fb1a75783d bec0734cd68e84477ba7fc1d13e6cff53ab70129" == "obsoleted 1 changesets"
sh % "cd .."

# client only pulls down 1 changeset

sh % "cd pull-hidden-common-client"
sh % "hg pull --debug" == r"""
    pulling from $TESTTMP/pull-hidden-common
    query 1; heads
    searching for changes
    taking quick initial sample
    query 2; still undecided: 2, sample size is: 2
    2 total queries in 0.0000s
    1 changesets found
    list of changesets:
    bec0734cd68e84477ba7fc1d13e6cff53ab70129
    listing keys for "bookmarks"
    bundle2-output-bundle: "HG20", 3 parts total
    bundle2-output-part: "changegroup" (params: 1 mandatory 1 advisory) streamed payload
    bundle2-output-part: "listkeys" (params: 1 mandatory) empty payload
    bundle2-output-part: "phase-heads" 24 bytes payload
    bundle2-input-bundle: with-transaction
    bundle2-input-part: "changegroup" (params: 1 mandatory 1 advisory) supported
    adding changesets
    add changeset bec0734cd68e
    adding manifests
    adding file changes
    adding foo revisions
    added 1 changesets with 1 changes to 1 files
    bundle2-input-part: total payload size 476
    bundle2-input-part: "listkeys" (params: 1 mandatory) supported
    bundle2-input-part: "phase-heads" supported
    bundle2-input-part: total payload size 24
    bundle2-input-bundle: 2 parts total
    checking for updated bookmarks
    new changesets bec0734cd68e"""
