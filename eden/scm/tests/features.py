# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

"""single place to control features used by tests

This file provides an alternative way to tweak "legacy" configs for
test compatibility. Eventually ideally this file does not exist.
But for now it's easier than changing configs at the top of individual
tests.
"""

ignorerevnumincompatiblelist = """
    test-annotate.t
    test-bisect.t
    test-bookmarks.t
    test-bookmark-strip.t
    test-bundle-r.t
    test-bundle.t
    test-bundle-vs-outgoing.t
    test-clone-r.t
    test-command-template.t
    test-commitcloud-hide.t
    test-commitcloud-move.t
    test-commit-interactive.t
    test-context-metadata.t
    test-contrib-perf.t
    test-debugbuilddag.t
    test-debugindexdot-t.py
    test-debugstrip.t
    test-dirstate-race.t
    test-empty-group-t.py
    test-eol-clone.t
    test-eol-hook.t
    test-eol-update.t
    test-export.t
    test-remotefilelog-wireproto.t
    test-treemanifest-prefetch.t
    test-treemanifest.t
    test-tweakdefaults-pullrebaseremotenames.t
    test-tweakdefaults-remotenames.t
    test-tweakdefaults.t
    test-fileset.t
    test-git-export.t
    test-glog-topological.t
    test-glog.t
    test-graft.t
    test-histedit-commute.t
    test-histedit-edit.t
    test-histedit-no-change.t
    test-import-bypass.t
    test-import-merge.t
    test-import.t
    test-log.t
    test-manifest.t
    test-merge10-t.py
    test-merge1.t
    test-merge-ancestor-mergestate.t
    test-merge-commit.t
    test-merge-tools.t
    test-mv-cp-st-diff.t
    test-pager.t
    test-parse-date.t
    test-perftweaks-remotenames.t
    test-pull-pull-corruption.t
    test-pull-r.t
    test-pull-update.t
    test-push.t
    test-rebase-issue-noparam-single-rev.t
    test-rebase-parameters.t
    test-rebase-pull.t
    test-rebase-scenario-global.t
    test-remotenames-bookmarks.t
    test-remotenames-pull-rebase.t
    test-remotenames-push.t
    test-remotenames-shared-repo.t
    test-remotenames-tracking.t
    test-revert.t
    test-revnum-deprecate.t
    test-revset2.t
    test-revset.t
    test-shelve.t
    test-sparse.t
    test-ssh-clone-r.t
    test-template-revf64.t
    test-url-rev.t
    test-casecollision-merge.t
    test-commitcloud-backup-sql2.t

    # drop internally uses rev numbers for rebase
    test-drop.t
    # clone -u doesn't support revset
    test-patch.t
    # pull -r with revset fails on remote repos
    test-pushrebase-remotenames.t
    test-treemanifest-noflat.t
    # times out without ignorerevnum=False
    test-fastlog.t
    test-rename-merge2.t
"""


def setup(testname, hgrcpath):
    # Disable mutation.record to maintain commit hashes.
    with open(hgrcpath, "a") as f:
        f.write("\n[mutation]\nrecord=False\n")
    # Support legacy revnum for incompatible tests.
    if testname in ignorerevnumincompatiblelist:
        with open(hgrcpath, "a") as f:
            f.write("\n[ui]\nignorerevnum=False\n")
