# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

mutationblacklist = """
    test-commitcloud-backup-all.t
    test-commitcloud-backup-compression.t
    test-commitcloud-backup-logging.t
    test-commitcloud-backup.t
    test-commitcloud-sync-bookmarks.t
    test-commitcloud-sync-oscillation.t
    test-commitcloud-sync.t
    test-debugstrip.t
    test-fb-hgext-hiddenerror.t
    test-fb-hgext-pushrebase.t
    test-fb-hgext-snapshot-show.t
    test-fb-hgext-treemanifest-infinitepush.t
    test-fb-hgext-treemanifest-treeonly-linknodes.t
    test-hggit-incoming.t
    test-infinitepush-bundlestore.t
    test-infinitepush-forwardfillerqueue.t
    test-infinitepush-replaybookmarksqueue-ignore-backup.t
    test-infinitepush-replaybookmarksqueue-multiple-updates.t
    test-infinitepush-replaybookmarksqueue-one-bookmark.t
    test-inherit-mode.t
    test-mutation-fromobsmarkers.t
    test-rebase-copy-relations.t
    test-rebase-dest.t
    test-revset2.t
    test-globalrevs-t.py
    test-obsmarker-template-t.py
""".split()

narrowheadsincompatiblelist = """
    test-bookmarks.t
    test-debugstrip.t
    test-directaccess-revset.t
    test-eol-clone.t
    test-generaldelta.t
    test-globalopts.t
    test-hgext-perfsuite.t
    test-hggit-bookmark-workflow.t
    test-hggit-push-r.t
    test-histedit-mutation.t
    test-hook.t
    test-incoming-outgoing.t
    test-infinitepush-bundlestore.t
    test-infinitepush-remotefilelog.t
    test-inherit-mode.t
    test-lfs.t
    test-log-wireproto-requests.t
    test-mutation.t
    test-pull-r.t
    test-push.t
    test-pushrebase-merge-changed-file-list.t
    test-rebase-abort.t
    test-rebase-missing-cwd.t
    test-rebase-mutation.t
    test-remotenames-fastheaddiscovery-hidden-commits.t
    test-remotenames-push.t
    test-revset-outgoing.t
    test-revset2.t
    test-setdiscovery.t
    test-share.t
    test-ssh.t
    test-uncommit.t
    test-url-rev.t
    test-visibility.t

    test-absorb-phase-t.py
    test-bookmark-strip-t.py
    test-command-template-t.py
    test-fb-hgext-remotefilelog-commit-repack-t.py
    test-fb-hgext-reset-t.py
    test-fb-hgext-smartlog-remotenames-t.py
    test-fb-hgext-tweakdefaults-pullrebaseffwd-t.py
    test-graft-t.py
    test-log-t.py
    test-pull-update-t.py
    test-rebase-check-restore-t.py
    test-remotenames-strip-t.py
    test-globalrevs-t.py
    test-hgsql-local-commands-t.py
    test-revset-t.py
    test-shelve-t.py
"""


def setup(testname, hgrcpath):
    # Disable mutation.record to maintain commit hashes.
    with open(hgrcpath, "a") as f:
        f.write("\n[mutation]\nrecord=False\n")
    # Disable mutation and re-enable obsstore on unsupported tests.
    if testname in mutationblacklist:
        with open(hgrcpath, "a") as f:
            f.write("\n[mutation]\nenabled=False\nproxy-obsstore=False\n")
    # Disable narrow-heads if incompatible.
    if testname in narrowheadsincompatiblelist:
        with open(hgrcpath, "a") as f:
            f.write("\n[experimental]\nnarrow-heads=False\n")
