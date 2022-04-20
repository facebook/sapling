# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This test was written when we migrated from using C++ Manifests to Rust
# Manifests and wanted to verify the values of the hashes.

from __future__ import absolute_import

import os

from testutil.autofix import eq
from testutil.dott import feature, sh, testtmp  # noqa: F401


def listcommitandmanifesthashes(rev):
    # returns dictionary from descrition to commit node and manifest node
    # { commit_name: (commit_hash, manifest_hash)}
    template = "{desc} {node|short} {manifest}\n"
    args = ["log", "-T", template, "-r", os.path.expandvars(rev)]
    return list(tuple(line.split()) for line in sh.hgexcept(*args).splitlines())


sh % "setconfig experimental.allowfilepeer=True"
sh % '. "$TESTDIR/library.sh"'

sh % "configure dummyssh"
sh % "enable treemanifest remotenames remotefilelog pushrebase"

# Check manifest behavior with empty commit
sh % "hginit emptycommit"
sh % "cd emptycommit"
(
    sh % "drawdag"
    << r""" # drawdag.defaultfiles=false
A
"""
)
eq(
    listcommitandmanifesthashes("$A::"),
    [("A", "7b3f3d5e5faf", "0000000000000000000000000000000000000000")],
)

# Check hash and manifest values in a local repository
sh % "hginit $TESTTMP/localcommitsandmerge"
sh % "cd $TESTTMP/localcommitsandmerge"

# A - add
# B - modify
# C, D - add + modify
# E - merge with conflict and divergence
# F - just checking that merge doesn't mess repo by performing a modify
(
    sh % "drawdag"
    << r""" # drawdag.defaultfiles=false
F   # F/y/c=f  # crash with rustmanifest if y/c=c
|
E    # E/y/d=(removed)
|\   # E/x/a=d
C |  # C/y/c=c
| |  # C/x/a=c
| D  # D/y/d=d
|/   # D/x/a=d
B  # B/x/b=b
|
A  # A/x/a=a
"""
)
eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "8080f180998f", "47968cf0bfa76dd552b0c468487e0b2e58dd067a"),
        ("B", "f3631cd323b7", "2e67f334fe3b408e0657bd93b6b0799d8e4bffbf"),
        ("C", "ab6f17cbfcbc", "9f7dac017ac942faf4c03e81b078194f95a4e042"),
        ("D", "d55de8a18953", "e6e729a4a441b3c48a20a19e6696a33428e8824b"),
        ("E", "02d26f311e24", "c618b8195031a0c6874a557ee7445f6567af4dd7"),
        ("F", "c431bfe62c4c", "c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46"),
    ],
)
sh % "hg files -r $F" == r"""
    x/a
    x/b
    y/c"""
sh % "hg cat -r $F x/a x/b y/c" == "dbf"


# Check that the same graph will be constructed from by pushing commits
# to a server doing pushrebase
sh % "hginit $TESTTMP/serverpushrebasemerge"
sh % "cd $TESTTMP/serverpushrebasemerge"
(
    sh % "cat"
    << r"""
[extensions]
pushrebase=
treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
[remotefilelog]
server=True
[treemanifest]
server=True
"""
    >> ".hg/hgrc"
)
sh % "hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/tempclient -q" == ""
sh % "cd $TESTTMP/tempclient"
(
    sh % "drawdag"
    << r""" # drawdag.defaultfiles=false
A  # A/x/a=a
"""
)
sh % "hg bookmark master -r $A"
eq(
    listcommitandmanifesthashes("$A::"),
    [("A", "8080f180998f", "47968cf0bfa76dd552b0c468487e0b2e58dd067a")],
)
sh % "hg push -r $A --to master --create" == r"""
pushing rev * to destination ssh://user@dummy/serverpushrebasemerge bookmark master (glob)
searching for changes
exporting bookmark master
remote: pushing 1 changeset:
remote:     *  A (glob)
"""

sh % "hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/clientpushrebasemerge -q" == r"""
    fetching tree '' 47968cf0bfa76dd552b0c468487e0b2e58dd067a
    1 trees fetched over 0.00s
    fetching tree 'x' 4f20beec050d22de4f11003f4cdadd266b59be20
    1 trees fetched over 0.00s"""
sh % "cd $TESTTMP/clientpushrebasemerge"
(
    sh % "cat"
    << r"""
[treemanifest]
sendtrees=True
treeonly=True
"""
    >> ".hg/hgrc"
)
sh % "drawdag" << r""" # drawdag.defaultfiles=false
F   # F/y/c=f  # crash with rustmanifest if y/c=c
|
E    # E/y/d=(removed)
|\   # E/x/a=d
C |  # C/y/c=c
| |  # C/x/a=c
| D  # D/y/d=d
|/   # D/x/a=d
B  # B/x/b=b
|
desc(A)
""" == ""
eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "8080f180998f", "47968cf0bfa76dd552b0c468487e0b2e58dd067a"),
        ("B", "f3631cd323b7", "2e67f334fe3b408e0657bd93b6b0799d8e4bffbf"),
        ("C", "ab6f17cbfcbc", "9f7dac017ac942faf4c03e81b078194f95a4e042"),
        ("D", "d55de8a18953", "e6e729a4a441b3c48a20a19e6696a33428e8824b"),
        ("E", "02d26f311e24", "c618b8195031a0c6874a557ee7445f6567af4dd7"),
        ("F", "c431bfe62c4c", "c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46"),
    ],
)
sh % "hg push --to=master -r $F" == r"""
    pushing rev c431bfe62c4c to destination ssh://user@dummy/serverpushrebasemerge bookmark master
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    updating bookmark master
    remote: pushing 5 changesets:
    remote:     *  B (glob)
    remote:     *  C (glob)
    remote:     *  D (glob)
    remote:     *  E (glob)
    remote:     *  F (glob)
    remote: 5 new changesets from the server will be downloaded"""

sh % "hg files -r master" == r"""
    x/a
    x/b
    y/c"""

# Check that a secondary client will pull a consistent view of the repository
sh % "hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/pullingclient -q" == r"""
    fetching tree '' c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46
    1 trees fetched over 0.00s
    2 trees fetched over 0.00s"""
sh % "cd $TESTTMP/pullingclient"
eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "8080f180998f", "47968cf0bfa76dd552b0c468487e0b2e58dd067a"),
        ("B", "f3631cd323b7", "2e67f334fe3b408e0657bd93b6b0799d8e4bffbf"),
        ("C", "ab6f17cbfcbc", "9f7dac017ac942faf4c03e81b078194f95a4e042"),
        ("D", "d55de8a18953", "e6e729a4a441b3c48a20a19e6696a33428e8824b"),
        ("E", "ce93848c2534", "c618b8195031a0c6874a557ee7445f6567af4dd7"),
        ("F", "2ce21aadf6a7", "c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46"),
    ],
)

# Check pushrebase with a branch
sh % "cd $TESTTMP/clientpushrebasemerge"
# F is master and we will branch from E
sh % "drawdag" << r""" # drawdag.defaultfiles=false
J   # J/x/a=i
|\  # J/y/d=h
| I # I/x/a=i
| |
H | # H/y/d=h
|/
G   # G/y/d=g
|
desc(E)
""" == ""
sh % "hg push --to=master -r $J" == r"""
    pushing rev * to destination ssh://user@dummy/serverpushrebasemerge bookmark master (glob)
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    updating bookmark master
    remote: pushing 4 changesets:
    remote:     *  G (glob)
    remote:     *  H (glob)
    remote:     *  I (glob)
    remote:     *  J (glob)
    remote: 4 new changesets from the server will be downloaded"""

# Check server after pushrebasing the branch whose parent is E
sh % "cd $TESTTMP/serverpushrebasemerge"
sh % "hg log -G -T '{desc} {bookmarks}'" == r"""
    o    J master
    ├─╮
    │ o  I
    │ │
    o │  H
    ├─╯
    o  G
    │
    o  F
    │
    o    E
    ├─╮
    │ o  D
    │ │
    o │  C
    ├─╯
    o  B
    │
    o  A"""
eq(
    listcommitandmanifesthashes("desc(F)::"),
    [
        ("F", "2ce21aadf6a7", "c8a3f0d6d065d07e6ee7cee3edf15712a7d15d46"),
        ("G", "f90743172206", "5d26e08806c5cdc3e7f3fba1d7fcf50cd224960e"),
        ("H", "3c5d22b367fc", "41a7c2a088eb3a436987339e5e73f08afa7da8e8"),
        ("I", "ae1644484ec9", "b151de3f04de862dfdbaa23c68a297f225951044"),
        ("J", "ec18dc54c59a", "ae0f3f86d8bf6dfb032cfc903794783ca8752437"),
    ],
)
