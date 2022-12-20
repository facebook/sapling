# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""single place to control features used by tests

This file provides an alternative way to tweak "legacy" configs for
test compatibility. Eventually ideally this file does not exist.
But for now it's easier than changing configs at the top of individual
tests.
"""

ignorerevnumincompatiblelist = """
    test-alias.t
    test-amend-hide.t
    test-amend-rebase.t
    test-amend-restack.t
    test-annotate.t
    test-backwards-remove.t
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
    test-commitcloud-switch-workspace.t
    test-commit-interactive.t
    test-confused-revert.t
    test-context-metadata.t
    test-contrib-perf.t
    test-debugbuilddag.t
    test-debugcheckcasecollisions.t
    test-debugindexdot-t.py
    test-debugmetalog.t
    test-debugrename.t
    test-debugstrip.t
    test-diff-change.t
    test-diffdir.t
    test-diff-issue2761.t
    test-diff-only-files-in-revs.t
    test-dirstate-race.t
    test-empty-group-t.py
    test-eol-clone.t
    test-eol-hook.t
    test-eol-update.t
    test-export.t
    test-fb-ext-copytrace-errormsg.t
    test-fb-ext-drop.t
    test-fb-ext-fastannotate-corrupt.t
    test-fb-ext-fastannotate-perfhack.t
    test-fb-ext-fastannotate-protocol.t
    test-fb-ext-fastannotate-renames.t
    test-fb-ext-fastannotate.t
    test-fb-ext-fastlog.t
    test-fb-ext-fbhistedit-rebase-interactive.t
    test-fb-ext-morestatus.t
    test-fb-ext-phrevset.t
    test-fb-ext-pushrebase-remotenames.t
    test-fb-ext-remotefilelog-prefetch.t
    test-fb-ext-remotefilelog-sparse.t
    test-fb-ext-remotefilelog-wireproto.t
    test-fb-ext-reset-remotenames.t
    test-fb-ext-reset.t
    test-fb-ext-smartlog-inhibit.t
    test-fb-ext-smartlog-remotenames.t
    test-fb-ext-treemanifest-noflat.t
    test-fb-ext-treemanifest-prefetch.t
    test-fb-ext-treemanifest.t
    test-fb-ext-tweakdefaults-ordering.t
    test-fb-ext-tweakdefaults-pullrebaseremotenames.t
    test-fb-ext-tweakdefaults-remotenames.t
    test-fb-ext-tweakdefaults.t
    test-fb-ext-tweakdefaults-update.t
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
    test-import-unknown.t
    test-issue1438.t
    test-issue612.t
    test-issue660.t
    test-locate.t
    test-log.t
    test-manifest-merging.t
    test-manifest.t
    test-merge10-t.py
    test-merge1.t
    test-merge4.t
    test-merge5.t
    test-merge9.t
    test-merge-ancestor-mergestate.t
    test-merge-commit.t
    test-merge-revert2.t
    test-merge-revert.t
    test-merge-tools.t
    test-mv-cp-st-diff.t
    test-pager.t
    test-parse-date.t
    test-patch.t
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
    test-remotenames-strip.t
    test-remotenames-tracking.t
    test-remotenames-update.t
    test-rename-merge2.t
    test-revert.t
    test-revert-unknown.t
    test-revnum-deprecate.t
    test-revset2.t
    test-revset.t
    test-shelve.t
    test-sparse.t
    test-sparse-verbose-json.t
    test-ssh-clone-r.t
    test-status-rev.t
    test-template-revf64.t
    test-update-empty.t
    test-url-rev.t
    test-visibility-reset.t
    test-hgsql-sqlrefill.t
    test-hgsql-strip.t
    test-hgsql-treemanifest.t
    test-casecollision-merge.t
    test-casefolding.t
    test-commitcloud-backup-sql2.t
"""


def setup(testname, hgrcpath):
    # Disable mutation.record to maintain commit hashes.
    with open(hgrcpath, "a") as f:
        f.write("\n[mutation]\nrecord=False\n")
    # Support legacy revnum for incompatible tests.
    if testname in ignorerevnumincompatiblelist:
        with open(hgrcpath, "a") as f:
            f.write("\n[ui]\nignorerevnum=False\n")
