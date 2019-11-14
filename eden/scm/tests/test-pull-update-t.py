# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "hg init t"
sh % "cd t"
sh % "echo 1" > "foo"
sh % "hg ci -Am m" == "adding foo"

sh % "cd .."
sh % "hg clone t tt" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd tt"
sh % "echo 1.1" > "foo"
sh % "hg ci -Am m"

sh % "cd ../t"
sh % "echo 1.2" > "foo"
sh % "hg ci -Am m"

# Should respect config to disable dirty update
sh % "hg co -qC 0"
sh % "echo 2" > "foo"
sh % "hg --config 'commands.update.check=abort' pull -u ../tt" == r"""
    pulling from ../tt
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 107cefe13e42
    abort: uncommitted changes
    [255]"""
sh % "hg debugstrip --no-backup tip"
sh % "hg co -qC tip"

# Should not update to the other topological branch:

sh % "hg pull -u ../tt" == r'''
    pulling from ../tt
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 107cefe13e42
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    updated to "800c91d5bfc1: m"
    1 other heads for branch "default"'''

sh % "cd ../tt"

# Should not update to the other branch:

sh % "hg pull -u ../t" == r'''
    pulling from ../t
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 800c91d5bfc1
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    updated to "107cefe13e42: m"
    1 other heads for branch "default"'''

sh % "'HGMERGE=true' hg merge" == r"""
    merging foo
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg ci -mm"

sh % "cd ../t"

# Should work:

sh % "hg pull -u ../tt" == r"""
    pulling from ../tt
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 483b76ad4309
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# Similarity between "hg update" and "hg pull -u" in handling bookmark
# ====================================================================

# Test that updating activates the bookmark, which matches with the
# explicit destination of the update.

sh % "echo 4" >> "foo"
sh % "hg commit -m '#4'"
sh % "hg bookmark active-after-pull"
sh % "cd ../tt"

# (1) activating by --rev BOOKMARK

sh % "hg bookmark -f active-before-pull"
sh % "hg bookmarks" == " * active-before-pull        3:483b76ad4309"

sh % "hg pull -u -r active-after-pull" == r"""
    pulling from $TESTTMP/t
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    adding remote bookmark active-after-pull
    new changesets f815b3da6163
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark active-after-pull)"""

sh % "hg parents -q" == "4:f815b3da6163"
sh % "hg bookmarks" == r"""
     * active-after-pull         4:f815b3da6163
       active-before-pull        3:483b76ad4309"""

# (discard pulled changes)

sh % "hg update -q 483b76ad4309"
sh % "hg rollback -q"

# (2) activating by URL#BOOKMARK

sh % "hg bookmark -f active-before-pull"
sh % "hg bookmarks" == " * active-before-pull        3:483b76ad4309"

sh % "hg pull -u '$TESTTMP/t#active-after-pull'" == r"""
    pulling from $TESTTMP/t
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    adding remote bookmark active-after-pull
    new changesets f815b3da6163
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark active-after-pull)"""

sh % "hg parents -q" == "4:f815b3da6163"
sh % "hg bookmarks" == r"""
     * active-after-pull         4:f815b3da6163
       active-before-pull        3:483b76ad4309"""

# (discard pulled changes)

sh % "hg update -q 483b76ad4309"
sh % "hg rollback -q"

# Test that updating deactivates current active bookmark, if the
# destination of the update is explicitly specified, and it doesn't
# match with the name of any existing bookmarks.

sh % "cd ../t"
sh % "hg bookmark -d active-after-pull"
sh % "hg commit -m '#5 (bar #1)' --config 'ui.allowemptycommit=1'"
sh % "cd ../tt"

# (1) deactivating by --rev REV

sh % "hg bookmark -f active-before-pull"
sh % "hg bookmarks" == " * active-before-pull        3:483b76ad4309"

sh % "hg pull -u -r f815b3da6163" == r"""
    pulling from $TESTTMP/t
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets f815b3da6163
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark active-before-pull)"""

sh % "hg parents -q" == "4:f815b3da6163"
sh % "hg bookmarks" == "   active-before-pull        3:483b76ad4309"

# (discard pulled changes)

sh % "hg update -q 483b76ad4309"
sh % "hg rollback -q"

sh % "cd .."
