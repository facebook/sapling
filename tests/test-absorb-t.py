# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


sh % "setconfig 'experimental.evolution='"
sh % "enable absorb"

sh % "cat" << r"""
from edenscm.mercurial import commands, registrar
cmdtable = {}
command = registrar.command(cmdtable)
@command('amend', [], '')
def amend(ui, repo, *pats, **opts):
    return 3
""" >> "$TESTTMP/dummyamend.py"
sh % "cat" << r"""
[extensions]
amend=$TESTTMP/dummyamend.py
[absorb]
amendflag = correlated
""" >> "$HGRCPATH"


def sedi(pattern, *paths):
    # pattern looks like 's/foo/bar/'
    _s, a, b = pattern.split("/")[:3]
    for path in paths:
        content = open(path, "rb").read().replace(a, b)
        open(path, "wb").write(content)


shlib.sedi = sedi

sh % "newrepo"

# Do not crash with empty repo:

sh % "hg absorb" == r"""
    abort: no changeset to change
    [255]"""

# Make some commits:

for i in range(1, 6):
    open("a", "ab").write("%s\n" % i)
    sh % ("hg commit -A a -q -m 'commit %s'" % i)

# Change a few lines:

sh % "cat" << r"""
1a
2b
3
4d
5e
""" > "a"

# Preview absorb changes:

sh % "hg absorb --dry-run" == r"""
    showing changes for a
            @@ -0,2 +0,2 @@
    4ec16f8 -1
    5c5f952 -2
    4ec16f8 +1a
    5c5f952 +2b
            @@ -3,2 +3,2 @@
    ad8b8b7 -4
    4f55fa6 -5
    ad8b8b7 +4d
    4f55fa6 +5e

    4 changesets affected
    4f55fa6 commit 5
    ad8b8b7 commit 4
    5c5f952 commit 2
    4ec16f8 commit 1"""

# Run absorb:

sh % "hg absorb --apply-changes" == r"""
    showing changes for a
            @@ -0,2 +0,2 @@
    4ec16f8 -1
    5c5f952 -2
    4ec16f8 +1a
    5c5f952 +2b
            @@ -3,2 +3,2 @@
    ad8b8b7 -4
    4f55fa6 -5
    ad8b8b7 +4d
    4f55fa6 +5e

    4 changesets affected
    4f55fa6 commit 5
    ad8b8b7 commit 4
    5c5f952 commit 2
    4ec16f8 commit 1
    saved backup bundle to * (glob)
    2 of 2 chunks applied"""
sh % "hg annotate a" == r"""
    0: 1a
    1: 2b
    2: 3
    3: 4d
    4: 5e"""

# Delete a few lines and related commits will be removed if they will be empty:

sh % "cat" << r"""
2b
4d
""" > "a"
sh % "echo y" | "hg absorb --config 'ui.interactive=1'" == r"""
    showing changes for a
            @@ -0,1 +0,0 @@
    f548282 -1a
            @@ -2,1 +1,0 @@
    ff5d556 -3
            @@ -4,1 +2,0 @@
    84e5416 -5e

    3 changesets affected
    84e5416 commit 5
    ff5d556 commit 3
    f548282 commit 1
    apply changes (yn)?  y
    saved backup bundle to * (glob)
    3 of 3 chunks applied"""
sh % "hg annotate a" == r"""
    1: 2b
    2: 4d"""
sh % "hg log -T '{rev} {desc}\\n' -Gp" == r"""
    @  2 commit 4
    |  diff -r 1cae118c7ed8 -r 58a62bade1c6 a
    |  --- a/a	Thu Jan 01 00:00:00 1970 +0000
    |  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    |  @@ -1,1 +1,2 @@
    |   2b
    |  +4d
    |
    o  1 commit 2
    |  diff -r 84add69aeac0 -r 1cae118c7ed8 a
    |  --- a/a	Thu Jan 01 00:00:00 1970 +0000
    |  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    |  @@ -0,0 +1,1 @@
    |  +2b
    |
    o  0 commit 1"""

# Non 1:1 map changes will be ignored:

sh % "echo 1" > "a"
sh % "hg absorb" == r"""
    showing changes for a
            @@ -0,2 +0,1 @@
            -2b
            -4d
            +1
    nothing to absorb
    [1]"""

# Insertaions:

sh % "cat" << r"""
insert before 2b
2b
4d
insert aftert 4d
""" > "a"
sh % "hg absorb -aq"
sh % "hg status"
sh % "hg annotate a" == r"""
    1: insert before 2b
    1: 2b
    2: 4d
    2: insert aftert 4d"""

# Bookmarks are moved:

sh % "hg bookmark -r 1 b1"
sh % "hg bookmark -r 2 b2"
sh % "hg bookmark ba"
sh % "hg bookmarks" == r"""
       b1                        1:b35060a57a50
       b2                        2:946e4bc87915
     * ba                        2:946e4bc87915"""
sh % "sedi s/insert/INSERT/ a"
sh % "hg absorb -aq"
sh % "hg status"
sh % "hg bookmarks" == r"""
       b1                        1:a4183e9b3d31
       b2                        2:c9b20c925790
     * ba                        2:c9b20c925790"""

# Non-mofified files are ignored:

sh % "touch b"
sh % "hg commit -A b -m b"
sh % "touch c"
sh % "hg add c"
sh % "hg rm b"
sh % "hg absorb" == r"""
    nothing to absorb
    [1]"""
sh % "sedi s/INSERT/Insert/ a"
sh % "hg absorb -a" == r"""
    showing changes for a
            @@ -0,1 +0,1 @@
    a4183e9 -INSERT before 2b
    a4183e9 +Insert before 2b
            @@ -3,1 +3,1 @@
    c9b20c9 -INSERT aftert 4d
    c9b20c9 +Insert aftert 4d

    2 changesets affected
    c9b20c9 commit 4
    a4183e9 commit 2
    saved backup bundle to * (glob)
    2 of 2 chunks applied"""
sh % "hg status" == r"""
    A c
    R b"""

# Public commits will not be changed:

sh % "hg phase -p 1"
sh % "sedi s/Insert/insert/ a"
sh % "hg absorb -n" == r"""
    showing changes for a
            @@ -0,1 +0,1 @@
            -Insert before 2b
            +insert before 2b
            @@ -3,1 +3,1 @@
    85b4e0e -Insert aftert 4d
    85b4e0e +insert aftert 4d

    1 changeset affected
    85b4e0e commit 4"""
sh % "hg absorb -a" == r"""
    showing changes for a
            @@ -0,1 +0,1 @@
            -Insert before 2b
            +insert before 2b
            @@ -3,1 +3,1 @@
    85b4e0e -Insert aftert 4d
    85b4e0e +insert aftert 4d

    1 changeset affected
    85b4e0e commit 4
    saved backup bundle to * (glob)
    1 of 2 chunks applied"""
sh % "hg diff -U 0" == r"""
    diff -r 1c8eadede62a a
    --- a/a	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	* (glob)
    @@ -1,1 +1,1 @@
    -Insert before 2b
    +insert before 2b"""
sh % "hg annotate a" == r"""
    1: Insert before 2b
    1: 2b
    2: 4d
    2: insert aftert 4d"""

# Make working copy clean:

sh % "hg revert -q -C a b"
sh % "hg forget c"
sh % "rm c"
sh % "hg status"

# Merge commit will not be changed:

sh % "echo 1" > "m1"
sh % "hg commit -A m1 -m m1"
sh % "hg bookmark -q -i m1"
sh % "hg update -q '.^'"
sh % "echo 2" > "m2"
sh % "hg commit -q -A m2 -m m2"
sh % "hg merge -q m1"
sh % "hg commit -m merge"
sh % "hg bookmark -d m1"
sh % "hg log -G -T '{rev} {desc} {phase}\\n'" == r"""
    @    6 merge draft
    |\
    | o  5 m2 draft
    | |
    o |  4 m1 draft
    |/
    o  3 b draft
    |
    o  2 commit 4 draft
    |
    o  1 commit 2 public
    |
    o  0 commit 1 public"""
sh % "echo 2" >> "m1"
sh % "echo 2" >> "m2"
sh % "hg absorb -a" == r"""
    abort: no changeset to change
    [255]"""
sh % "hg revert -q -C m1 m2"

# Use a new repo:

sh % "newrepo"

# Make some commits to multiple files:

for f in ["a", "b"]:
    for i in [1, 2]:
        open(f, "ab").write("%s line %s\n" % (f, i))
        sh.hg("commit", "-A", f, "-m", "commit %s %s" % (f, i), "-q")

# Use pattern to select files to be fixed up:

sh % "sedi s/line/Line/ a b"
sh % "hg status" == r"""
    M a
    M b"""
sh % "hg absorb -a a" == r"""
    showing changes for a
            @@ -0,2 +0,2 @@
    6905bbb -a line 1
    4472dd5 -a line 2
    6905bbb +a Line 1
    4472dd5 +a Line 2

    2 changesets affected
    4472dd5 commit a 2
    6905bbb commit a 1
    saved backup bundle to * (glob)
    1 of 1 chunk applied"""
sh % "hg status" == "M b"
sh % "hg absorb -a --exclude b" == r"""
    nothing to absorb
    [1]"""
sh % "hg absorb -a b" == r"""
    showing changes for b
            @@ -0,2 +0,2 @@
    2517e37 -b line 1
    61782db -b line 2
    2517e37 +b Line 1
    61782db +b Line 2

    2 changesets affected
    61782db commit b 2
    2517e37 commit b 1
    saved backup bundle to * (glob)
    1 of 1 chunk applied"""
sh % "hg status"
sh % "cat a b" == r"""
    a Line 1
    a Line 2
    b Line 1
    b Line 2"""

# Test config option absorb.maxstacksize:

sh % "sedi s/Line/line/ a b"
sh % "hg log -T '{rev}:{node} {desc}\\n'" == r"""
    3:712d16a8f445834e36145408eabc1d29df05ec09 commit b 2
    2:74cfa6294160149d60adbf7582b99ce37a4597ec commit b 1
    1:28f10dcf96158f84985358a2e5d5b3505ca69c22 commit a 2
    0:f9a81da8dc53380ed91902e5b82c1b36255a4bd0 commit a 1"""
sh % "hg --config 'absorb.maxstacksize=1' absorb -n" == r"""
    absorb: only the recent 1 changesets will be analysed
    showing changes for a
            @@ -0,2 +0,2 @@
            -a Line 1
            -a Line 2
            +a line 1
            +a line 2
    showing changes for b
            @@ -0,2 +0,2 @@
            -b Line 1
    712d16a -b Line 2
            +b line 1
    712d16a +b line 2

    1 changeset affected
    712d16a commit b 2"""

# Test obsolete markers creation:

sh % "cat" << r"""
[experimental]
evolution=createmarkers
""" >> "$HGRCPATH"

sh % "hg --config 'absorb.maxstacksize=3' sf -a" == r"""
    absorb: only the recent 3 changesets will be analysed
    showing changes for a
            @@ -0,2 +0,2 @@
            -a Line 1
    28f10dc -a Line 2
            +a line 1
    28f10dc +a line 2
    showing changes for b
            @@ -0,2 +0,2 @@
    74cfa62 -b Line 1
    712d16a -b Line 2
    74cfa62 +b line 1
    712d16a +b line 2

    3 changesets affected
    712d16a commit b 2
    74cfa62 commit b 1
    28f10dc commit a 2
    2 of 2 chunks applied"""
sh % "hg log -T '{rev}:{node|short} {desc} {get(extras, \"absorb_source\")}\\n'" == r"""
    6:cbc0c676ae8f commit b 2  (trailing space)
    5:071dee819ad0 commit b 1  (trailing space)
    4:4faf555e5598 commit a 2  (trailing space)
    0:f9a81da8dc53 commit a 1"""
sh % "hg absorb -a" == r"""
    showing changes for a
            @@ -0,1 +0,1 @@
    f9a81da -a Line 1
    f9a81da +a line 1

    1 changeset affected
    f9a81da commit a 1
    1 of 1 chunk applied"""
sh % "hg log -T '{rev}:{node|short} {desc} {get(extras, \"absorb_source\")}\\n'" == r"""
    10:a478955a9e03 commit b 2  (trailing space)
    9:7380d5e6fab8 commit b 1  (trailing space)
    8:4472dd5179eb commit a 2  (trailing space)
    7:6905bbb02e4e commit a 1"""

# Test config option absorb.amendflags and running as a sub command of amend:

sh % "hg amend -h" == r"""
    hg amend

    (no help text available)

    Options:

      --correlated incorporate corrections into stack. see 'hg help absorb' for
                   details

    (some details hidden, use --verbose to show complete help)"""

open("c", "wb").write(bytearray([0, 1, 2, 10]))

sh % "hg commit -A c -m 'c is a binary file'"
sh % "echo c" >> "c"

sh % "cat b" == r"""
    b line 1
    b line 2
"""
sh % "cat" << "b line 1\nINS\nb line 2\n" > "b"

sh % "echo END" >> "b"
sh % "hg rm a"
sh % "echo y" | "hg amend --correlated --config 'ui.interactive=1'" == r"""
    showing changes for b
            @@ -1,0 +1,1 @@
            +INS
            @@ -2,0 +3,1 @@
    a478955 +END

    1 changeset affected
    a478955 commit b 2
    apply changes (yn)?  y
    1 of 2 chunks applied

    # changes not applied and left in working directory:
    # M b : 1 modified chunks were ignored
    # M c : unsupported file type (ex. binary or link)
    # R a : removed files were ignored"""

# Executable files:

sh % "cat" << r"""
[diff]
git=True
""" >> "$HGRCPATH"

if feature.check(["execbit"]):
    sh % "newrepo"
    sh % "echo" > "foo.py"
    sh % "chmod +x foo.py"
    sh % "hg add foo.py"
    sh % "hg commit -mfoo"

    sh % "echo bla" > "foo.py"
    sh % "hg absorb --dry-run" == r"""
        showing changes for foo.py
                @@ -0,1 +0,1 @@
        99b4ae7 -
        99b4ae7 +bla

        1 changeset affected
        99b4ae7 foo"""
    sh % "hg absorb --apply-changes" == r"""
        showing changes for foo.py
                @@ -0,1 +0,1 @@
        99b4ae7 -
        99b4ae7 +bla

        1 changeset affected
        99b4ae7 foo
        1 of 1 chunk applied"""
    sh % "hg diff -c ." == r"""
        diff --git a/foo.py b/foo.py
        new file mode 100755
        --- /dev/null
        +++ b/foo.py
        @@ -0,0 +1,1 @@
        +bla"""
    sh % "hg diff"

# Remove lines may delete changesets:

sh % "newrepo"
sh % "cat" << r"""
1
2
""" > "a"
sh % "hg commit -m a12 -A a"
sh % "cat" << r"""
1
2
""" > "b"
sh % "hg commit -m b12 -A b"
sh % "echo 3" >> "b"
sh % "hg commit -m b3"
sh % "echo 4" >> "b"
sh % "hg commit -m b4"
sh % "echo 1" > "b"
sh % "echo 3" >> "a"
sh % "hg absorb -n" == r"""
    showing changes for a
            @@ -2,0 +2,1 @@
    bfafb49 +3
    showing changes for b
            @@ -1,3 +1,0 @@
    1154859 -2
    30970db -3
    a393a58 -4

    4 changesets affected
    a393a58 b4
    30970db b3
    1154859 b12
    bfafb49 a12"""
sh % "hg absorb -av" | "grep became" == r"""
    bfafb49242db: 1 file(s) changed, became 259b86984766
    115485984805: 2 file(s) changed, became bd7f2557c265
    30970dbf7b40: became empty and was dropped
    a393a58b9a85: became empty and was dropped"""
sh % "hg log -T '{rev} {desc}\\n' -Gp" == r"""
    @  5 b12
    |  diff --git a/b b/b
    |  new file mode 100644
    |  --- /dev/null
    |  +++ b/b
    |  @@ -0,0 +1,1 @@
    |  +1
    |
    o  4 a12
       diff --git a/a b/a
       new file mode 100644
       --- /dev/null
       +++ b/a
       @@ -0,0 +1,3 @@
       +1
       +2
       +3"""
# Only with commit deletion:

sh % "newrepo"
sh % "touch a"
sh % "hg ci -m 'empty a' -A a"
sh % "echo 1" >> "a"
sh % "hg ci -m 'append to a'"
sh % "rm a"
sh % "touch a"
sh % "'HGPLAIN=1' hg absorb" == r"""
    showing changes for a
            @@ -0,1 +0,0 @@
    d235271 -1

    1 changeset affected
    d235271 append to a
    apply changes (yn)?  y
    1 of 1 chunk applied"""
