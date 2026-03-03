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
    test-sparse.t
    test-template-revf64.t
    test-url-rev.t

    # drop internally uses rev numbers for rebase
    test-drop.t
    # coupled with rev numbers
    test-debugbuilddag.t
    # clone -u doesn't support revset
    test-patch.t
    test-pull-pull-corruption.t
    # pull -r with revset fails on remote repos
    test-pushrebase-remotenames.t
    test-perftweaks-remotenames.t
    test-treemanifest-noflat.t
    # times out without ignorerevnum=False
    test-fastlog.t
    test-rename-merge2.t
    # annotate -n output heavily relies on rev numbers
    test-annotate.t
    # many rev number references with ambiguous commit messages (msg 0..31)
    test-bisect.t
    # 135+ revision number references throughout
    test-command-template.t
    # heavily uses rev numbers in export ranges and %r/%R output format
    test-export.t
    # times out without ignorerevnum=False
    test-debugstrip.t
    # output changed significantly with ignorerevnum
    test-commit-interactive.t
    # times out without ignorerevnum=False
    test-tweakdefaults.t
    # many revision number references throughout (in fileset queries and -r arguments)
    test-fileset.t
    # 29+ revision number references across multiple commits
    test-git-export.t
    # 2742 lines with 29 revision references and 18 {rev} templates
    test-glog.t
    # 1181 lines with 33+ revision references
    test-graft.t
    # uses negative revision numbers in histedit commands
    test-histedit-no-change.t
    # 43 revision number references throughout
    test-log.t
    # 42 revision number references throughout
    test-merge-tools.t
    # times out at 30s timeout limit
    test-rebase-scenario-global.t
    test-remotenames-tracking.t
    # intentionally tests rev number deprecation warnings; uses rev numbers by design
    test-revnum-deprecate.t
    # many rev number references across a large test file
    test-revset2.t
    # very complex, 2784 lines with many rev number references
    test-revset.t
    # complex unshelve state management
    test-shelve.t
"""


def setup(testname, hgrcpath):
    # Disable mutation.record to maintain commit hashes.
    with open(hgrcpath, "a") as f:
        f.write("\n[mutation]\nrecord=False\n")
    # Support legacy revnum for incompatible tests.
    if testname in ignorerevnumincompatiblelist:
        with open(hgrcpath, "a") as f:
            f.write("\n[ui]\nignorerevnum=False\n")
