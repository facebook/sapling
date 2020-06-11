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
    test-common-commands-fb.t
    test-debugstrip.t
    test-fb-hgext-hiddenerror.t
    test-fb-hgext-pushrebase.t
    test-fb-hgext-snapshot-show.t
    test-fb-hgext-treemanifest-infinitepush.t
    test-fb-hgext-treemanifest-treeonly-linknodes.t
    test-fb-hgext-undo.t
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
    test-rebase-obsolete.t
    test-revset2.t
    test-globalrevs-t.py
    test-obsmarker-template-t.py
""".split()


def setup(testname, hgrcpath):
    # Disable mutation.record to maintain commit hashes.
    with open(hgrcpath, "a") as f:
        f.write("\n[mutation]\nrecord=False\n")
    # Disable mutation and re-enable obsstore on unsupported tests.
    if testname in mutationblacklist:
        with open(hgrcpath, "a") as f:
            f.write("\n[mutation]\nenabled=False\nproxy-obsstore=False\n")
