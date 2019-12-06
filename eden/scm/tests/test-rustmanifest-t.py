# Copyright (c) Facebook, Inc. and its affiliates.
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


sh % '. "$TESTDIR/library.sh"'

sh % "cat" << r"""
[extensions]
treemanifest=
remotenames=
remotefilelog=
pushrebase=
[ui]
ssh = python "$TESTDIR/dummyssh"
""" >> "$HGRCPATH"

# Check manifest behavior with empty commit
sh % "hginit emptycommit"
sh % "cd emptycommit"
sh % "drawdag" << r""" # drawdag.defaultfiles=false
A
"""
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
A  # A/x/a=a
"""
eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "bd99ff0a074c", "7607ba5a97e3117540bbb7525093678eb26e374f"),
        ("B", "329658f81fe4", "02e01983feb89482571eb285cdf95791d5d5004c"),
        ("C", "8f7b309be719", "7630040f028fe48237216dd272521b72cbc9fdd4"),
        ("D", "4739f43fec6e", "241b1b1a0c626f74c431fea9a19f8d41babf6d66"),
        ("E", "25624926a6f6", "536621fb22888a57188bdb3fb7524956e9eea571"),
        ("F", "7d6ade338bd7", "f597a49b2fb7de2f6ccc8daea22210cc762f463f"),
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
sh % "cat" << r"""
[extensions]
pushrebase=
[remotefilelog]
server=True
[treemanifest]
server=True
""" >> ".hg/hgrc"
sh % "drawdag" << r""" # drawdag.defaultfiles=false
A  # A/x/a=a
"""
sh % "hg bookmark master -r $A"
eq(
    listcommitandmanifesthashes("$A::"),
    [("A", "bd99ff0a074c", "7607ba5a97e3117540bbb7525093678eb26e374f")],
)

sh % "hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/clientpushrebasemerge -q" == r"""
    fetching tree '' 7607ba5a97e3117540bbb7525093678eb26e374f
    2 trees fetched over 0.00s"""
sh % "cd $TESTTMP/clientpushrebasemerge"
sh % "cat" << r"""
[treemanifest]
sendtrees=True
treeonly=True
""" >> ".hg/hgrc"
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
        ("A", "bd99ff0a074c", "7607ba5a97e3117540bbb7525093678eb26e374f"),
        ("B", "329658f81fe4", "02e01983feb89482571eb285cdf95791d5d5004c"),
        ("C", "8f7b309be719", "7630040f028fe48237216dd272521b72cbc9fdd4"),
        ("D", "4739f43fec6e", "241b1b1a0c626f74c431fea9a19f8d41babf6d66"),
        ("E", "25624926a6f6", "536621fb22888a57188bdb3fb7524956e9eea571"),
        ("F", "7d6ade338bd7", "f597a49b2fb7de2f6ccc8daea22210cc762f463f"),
    ],
)
sh % "hg push --to=master -r $F" == r"""
    pushing rev 7d6ade338bd7 to destination ssh://user@dummy/serverpushrebasemerge bookmark master
    searching for changes
    remote: pushing 5 changesets:
    remote:     329658f81fe4  B
    remote:     8f7b309be719  C
    remote:     4739f43fec6e  D
    remote:     25624926a6f6  E
    remote:     7d6ade338bd7  F
    remote: 5 new changesets from the server will be downloaded
    adding changesets
    adding manifests
    adding file changes
    added 2 changesets with 0 changes to 4 files
    2 new obsolescence markers
    updating bookmark master
    obsoleted 2 changesets"""

sh % "hg files -r master" == r"""
    x/a
    x/b
    y/c"""

# Check how data on server looks after pushrebase
sh % "cd $TESTTMP/serverpushrebasemerge"
# Check that the shape of the graph looks the same on the server as it did on
# the client
sh % "hg log -G -T {desc}" << r"""
""" == r"""
    o  F
    |
    o    E
    |\
    | o  D
    | |
    o |  C
    |/
    o  B
    |
    o  A"""

eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "bd99ff0a074c", "7607ba5a97e3117540bbb7525093678eb26e374f"),
        ("B", "329658f81fe4", "02e01983feb89482571eb285cdf95791d5d5004c"),
        ("C", "8f7b309be719", "7630040f028fe48237216dd272521b72cbc9fdd4"),
        ("D", "4739f43fec6e", "241b1b1a0c626f74c431fea9a19f8d41babf6d66"),
        ("E", "a932a3c05d51", "536621fb22888a57188bdb3fb7524956e9eea571"),
        ("F", "38d281aaf22d", "f597a49b2fb7de2f6ccc8daea22210cc762f463f"),
    ],
)
sh % "hg files -r master" == r"""
    x/a
    x/b
    y/c"""
sh % "hg cat -r master x/a x/b y/c" == "dbf"

# Check that a secondary client will pull a consistent view of the repository
sh % "hg clone 'ssh://user@dummy/serverpushrebasemerge' $TESTTMP/pullingclient -q" == r"""
    fetching tree '' f597a49b2fb7de2f6ccc8daea22210cc762f463f, based on 7607ba5a97e3117540bbb7525093678eb26e374f, found via 38d281aaf22d
    3 trees fetched over 0.00s"""
sh % "cd $TESTTMP/pullingclient"
eq(
    listcommitandmanifesthashes("$A::"),
    [
        ("A", "bd99ff0a074c", "7607ba5a97e3117540bbb7525093678eb26e374f"),
        ("B", "329658f81fe4", "02e01983feb89482571eb285cdf95791d5d5004c"),
        ("C", "8f7b309be719", "7630040f028fe48237216dd272521b72cbc9fdd4"),
        ("D", "4739f43fec6e", "241b1b1a0c626f74c431fea9a19f8d41babf6d66"),
        ("E", "a932a3c05d51", "536621fb22888a57188bdb3fb7524956e9eea571"),
        ("F", "38d281aaf22d", "f597a49b2fb7de2f6ccc8daea22210cc762f463f"),
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
    pushing rev d82619755335 to destination ssh://user@dummy/serverpushrebasemerge bookmark master
    searching for changes
    remote: pushing 4 changesets:
    remote:     e06c993545b9  G
    remote:     02c1e8fb563f  H
    remote:     1017689b7b56  I
    remote:     d82619755335  J
    remote: 4 new changesets from the server will be downloaded
    adding changesets
    adding manifests
    adding file changes
    added 4 changesets with 0 changes to 2 files
    4 new obsolescence markers
    updating bookmark master
    obsoleted 4 changesets"""

# Check server after pushrebasing the branch whose parent is E
sh % "cd $TESTTMP/serverpushrebasemerge"
sh % "hg log -G -T '{desc} {bookmarks}'" == r"""
    o    J master
    |\
    | o  I
    | |
    o |  H
    |/
    o  G
    |
    o  F
    |
    o    E
    |\
    | o  D
    | |
    o |  C
    |/
    o  B
    |
    o  A"""
eq(
    listcommitandmanifesthashes("38d281aaf22d::"),
    [
        ("F", "38d281aaf22d", "f597a49b2fb7de2f6ccc8daea22210cc762f463f"),
        ("G", "497c401919aa", "ca20659db942b8ea320f881b49e68e21e45864f1"),
        ("H", "3bd0ec8704f8", "ae9b2352116ef794826314dc2ee7409936e1c371"),
        ("I", "006310844558", "d3600921b015507d29c92e9f7ffa7c78c42e67db"),
        ("J", "3a19854876d1", "24ab094f699834d0b27ccc5b4c59066b3dd06438"),
    ],
)

sh % "hg files -r master" == r"""
    x/a
    x/b
    y/c
    y/d"""
sh % "hg cat -r master x/a x/b y/c y/d" == "ibfh"
