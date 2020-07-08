# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

mutationblacklist = """
    test-commitcloud-backup-all.t
    test-commitcloud-sync-oscillation.t
    test-fb-hgext-hiddenerror.t
    test-fb-hgext-snapshot-show.t
    test-fb-hgext-treemanifest-infinitepush.t
    test-fb-hgext-treemanifest-treeonly-linknodes.t
    test-hggit-incoming.t
    test-infinitepush-forwardfillerqueue.t
    test-infinitepush-replaybookmarksqueue-ignore-backup.t
    test-infinitepush-replaybookmarksqueue-multiple-updates.t
    test-infinitepush-replaybookmarksqueue-one-bookmark.t
    test-inherit-mode.t
    test-mutation-fromobsmarkers.t
    test-rebase-dest.t
    test-revset2.t
    test-obsmarker-template-t.py
""".split()

narrowheadsincompatiblelist = """
    test-bookmarks.t
    test-directaccess-revset.t
    test-eol-clone.t
    test-hgext-perfsuite.t
    test-lfs.t
    test-push.t
    test-revset2.t

    test-hgsql-local-commands-t.py
    test-revset-t.py
"""

transactionincompatiblelist = """
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
    # Disable head-based-commit-transaction if incomaptible.
    if testname in transactionincompatiblelist:
        with open(hgrcpath, "a") as f:
            f.write("\n[experimental]\nhead-based-commit-transaction=False\n")
