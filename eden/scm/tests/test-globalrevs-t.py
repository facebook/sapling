# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


sh % ". '$TESTDIR/hgsql/library.sh'"
sh % "initdb"
sh % "setconfig 'extensions.treemanifest=!'"

# Test operations on server repository with bad configuration fail in expected
# ways.

sh % "hg init master"
sh % "cd master"
sh % "cat" << r"""
[extensions]
globalrevs=
[globalrevs]
svnrevinteroperation=True
[hgsql]
enabled = True
""" >> ".hg/hgrc"

sh % "cat" << r"""
[globalrevs]
reponame = customname
""" >> ".hg/hgrc"

# - Expectation is to fail because hgsql extension is not enabled.

# sh % "hg log -r tip -T '{node}'" == r"""
#   abort: hgsql extension is not enabled
#   [255]"""


# - Properly configure the server with respect to hgsql extension.

sh % "configureserver . master"


# - Expectation is to fail because pushrebase extension is not enabled.

sh % "hg log -r tip -T '{node}'" == r"""
    abort: pushrebase extension is not enabled
    [255]"""


# - Enable pushrebase extension on the server.

sh % "cat" << r"""
[extensions]
pushrebase=
""" >> ".hg/hgrc"


# - Expectation is to fail because we need to configure pushrebase to only allow
# commits created through pushrebase extension.

sh % "hg log -r tip -T '{node}'" == r"""
    abort: pushrebase using incorrect configuration
    [255]"""


# - We can override the option to allow creation of commits only through
# pushrebase by setting `globalrevs.onlypushrebase` as False which will make the
# previous command succeed as we won't care about the pushrebase configuration.

sh % "hg log -r tip -T '{node}' --config 'globalrevs.onlypushrebase=False'" == "0000000000000000000000000000000000000000"


# - Configure server repository to only allow commits created through pushrebase.

sh % "cat" << r"""
[pushrebase]
blocknonpushrebase = True
""" >> ".hg/hgrc"


# - Test that the `globalrev` command fails because there is no entry in the
# database for the next available strictly increasing revision number.

sh % "hg globalrev" == r"""
    abort: no commit counters for customname in database
    [255]"""


# - Test that the `initglobalrev` command fails when run without the
# `--i-know-what-i-am-doing` flag.

sh % "hg initglobalrev 5000" == r"""
    abort: * (glob)
    [255]"""


# - Test that incorrect arguments to the `initglobalrev` command result in error.

sh % "hg initglobalrev blah --i-know-what-i-am-doing" == r"""
    abort: start must be an integer.
    [255]"""


# - Configure the next available strictly increasing revision number to be 5000.
sh % "hg initglobalrev 5000 --i-know-what-i-am-doing"
sh % "hg globalrev" == "5000"


# - Check that we can only set the next available strictly increasing revision
# number once.

cmd = sh % "hg initglobalrev 5000 --i-know-what-i-am-doing"
assert "[1]" in cmd.output, "initglobalrev twice should fail"


# - Server is configured properly now. We can create an initial commit in the
# database.

sh % "hg log -r tip -T '{node}'" == "0000000000000000000000000000000000000000"

sh % "touch a"
sh % "hg ci -Aqm a --config 'extensions.globalrevs=!'"
sh % "hg book master"


# Test that pushing to a server with the `globalrevs` extension enabled leads to
# creation of commits with strictly increasing revision numbers accessible through
# the `globalrev` template.

# - Configure client. `globalrevs` extension is enabled for making the `globalrev`
# template available to the client.

sh % "cd .."
sh % "initclient client"
sh % "cd client"
sh % "cat" << r"""
[extensions]
globalrevs=
pushrebase=
[experimental]
evolution = all
""" >> ".hg/hgrc"


# - Make commits on the client.

sh % "hg pull -q 'ssh://user@dummy/master'"
sh % "hg up -q tip"
sh % "touch b"
sh % "hg ci -Aqm b"
sh % "touch c"
sh % "hg ci -Aqm c"


# - Finally, push the commits to the server.

sh % "hg push -q 'ssh://user@dummy/master' --to master"


# - Check that the `globalrev` template on the client and server shows strictly
# increasing revision numbers for the pushed commits.

sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    @  5001 c
    |
    o  5000 b
    |
    o   a"""

sh % "cd ../master"
sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    o  5001 c
    |
    o  5000 b
    |
    @   a"""


# Test that running the `globalrev` command on the client fails.

sh % "cd ../client"
sh % "hg globalrev" == r"""
    abort: this repository is not a sql backed repository
    [255]"""

sh % "cd ../master"


# Test that failure of the transaction is handled gracefully and does not affect
# the assignment of subsequent strictly increasing revision numbers.

# - Configure the transaction to always fail before closing on the server.

sh % "cp .hg/hgrc .hg/hgrc.bak"
sh % "cat" << r"""
[hooks]
pretxnclose.error = exit 1
""" >> ".hg/hgrc"


# - Make some commits on the client.

sh % "cd ../client"
sh % "touch d"
sh % "hg ci -Aqm d"
sh % "touch e"
sh % "hg ci -Aqm e"


# - Try pushing the commits to the server. Push should fail because of the
# incorrect configuration on the server.

sh % "hg push -q 'ssh://user@dummy/master' --to master" == r"""
    abort: push failed on remote
    [255]"""


# - Fix the configuration on the server and retry. This time the pushing should
# succeed.

sh % "cd ../master"
sh % "mv .hg/hgrc.bak .hg/hgrc"

sh % "cd ../client"
sh % "hg push -q 'ssh://user@dummy/master' --to master"


# - Check that both the client and server have the expected strictly increasing
# revisions numbers.

sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    @  5003 e
    |
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    o   a"""

sh % "cd ../master"
sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    o  5003 e
    |
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    @   a"""


# Test pushing to a different head on the server.

# - Make some commits on the client to a different head (other than the current
# tip).

sh % "cd ../client"
sh % "hg up -q 'tip^'"
sh % "touch f"
sh % "hg ci -Aqm f"
sh % "touch g"
sh % "hg ci -Aqm g"


# - Push the commits to the server.

sh % "hg push -q 'ssh://user@dummy/master' --to master"


# - Check that both the client and server have the expected strictly increasing
# revisions numbers.

sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    @  5005 g
    |
    o  5004 f
    |
    | o  5003 e
    |/
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    o   a"""

sh % "cd ../master"
sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    o  5005 g
    |
    o  5004 f
    |
    | o  5003 e
    |/
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    @   a"""


# Test cherry picking commits from a branch and pushing to another branch.

# - On the client, cherry pick a commit from one branch to copy to the only other
# branch head. In particular, we are copying the commit with description `g` on
# top of commit with description `e`.

sh % "cd ../client"
sh % "hg rebase -qk -d 'desc(\"e\")' -r tip --collapse -m g1 --config 'extensions.rebase='"

# - Check that the rebase did not add `globalrev` to the commit since the commit
# did not reach the server yet.

sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    @   g1
    |
    | o  5005 g
    | |
    | o  5004 f
    | |
    o |  5003 e
    |/
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    o   a"""

# - Push the commits to the server.

sh % "hg push -q 'ssh://user@dummy/master' --to master"


# - Check that both the client and server have the expected strictly increasing
# revisions numbers.

sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    @  5006 g1
    |
    | o  5005 g
    | |
    | o  5004 f
    | |
    o |  5003 e
    |/
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    o   a"""

sh % "cd ../master"
sh % "hg log -GT '{globalrev} {desc}\\n'" == r"""
    o  5006 g1
    |
    | o  5005 g
    | |
    | o  5004 f
    | |
    o |  5003 e
    |/
    o  5002 d
    |
    o  5001 c
    |
    o  5000 b
    |
    @   a"""


# Test simultaneous pushes to different heads.

# - Configure the existing server to not work on incoming changegroup immediately.

sh % "cp .hg/hgrc .hg/hgrc.bak"
sh % "printf '[hooks]\\npre-changegroup.sleep = sleep 2\\n'" >> ".hg/hgrc"


# - Create a second server.

sh % "cd .."
sh % "initserver master2 master"

sh % "cd master2"
sh % "cat" << r"""
[extensions]
globalrevs=
pushrebase=
[pushrebase]
blocknonpushrebase=True
""" >> ".hg/hgrc"

sh % "cat" << r"""
[globalrevs]
reponame = customname
""" >> ".hg/hgrc"


# - Create a second client corresponding to the second server.

sh % "cd .."
sh % "initclient client2"

sh % "hg pull -q -R client2 'ssh://user@dummy/master2'"

sh % "cd client2"
sh % "cat" << r"""
[extensions]
globalrevs=
pushrebase=
[experimental]
evolution = all
""" >> ".hg/hgrc"


# - Make some commits on top of the tip commit on the first client.

sh % "cd ../client"
sh % "hg up -q tip"
sh % "touch h1"
sh % "hg ci -Aqm h1"
sh % "touch i"
sh % "hg ci -Aqm i"


# - Make some commits on top of the tip commit on the second client.

sh % "cd ../client2"
sh % "hg up -q tip"
sh % "touch h2"
sh % "hg ci -Aqm h2"


# - Push the commits from both the clients.

sh % "cd .."
sh % "hg push -R client -q 'ssh://user@dummy/master' --to master"
sh % "hg push -R client2 -q -f 'ssh://user@dummy/master2' --to master"


# - Introduce some bash functions to help with testing


def getglobalrev(commit):
    return int(getglobalrevstr(commit))


def getglobalrevstr(commit):
    return shlib.hg("log", "-r", commit, "-T", "{globalrev}")


shlib.getglobalrev = getglobalrevstr


def isgreaterglobalrev(left, right):
    if getglobalrev(left) > getglobalrev(right):
        return ""
    return "[1]"


shlib.isgreaterglobalrev = isgreaterglobalrev


def isnotequalglobalrev(left, right):
    if getglobalrev(left) != getglobalrev(right):
        return ""
    return "[1]"


shlib.isgreaterglobalrev = isnotequalglobalrev


def checkglobalrevs():
    if (
        isgreaterglobalrev("desc('h2')", "desc('g1')") == ""
        and isgreaterglobalrev("desc('i')", "desc('h1')") == ""
        and isgreaterglobalrev("desc('h1')", "desc('g1')") == ""
        and isnotequalglobalrev("desc('i')", "desc('h2')") == ""
        and isnotequalglobalrev("desc('h1')", "desc('h2')") == ""
    ):
        return ""
    return "[1]"


shlib.checkglobalrevs = checkglobalrevs

# - Check that both the servers have the expected strictly increasing revision
# numbers.

sh % "cd master"
sh % "checkglobalrevs"

sh % "cd ../master2"
sh % "checkglobalrevs"


# - Check that both the clients have the expected strictly increasing revisions
# numbers.

sh % "cd ../client"
sh % "isgreaterglobalrev 'desc(\"i\")' 'desc(\"h1\")'"
sh % "isgreaterglobalrev 'desc(\"h1\")' 'desc(\"g1\")'"

sh % "cd ../client2"
sh % "isgreaterglobalrev 'desc(\"h2\")' 'desc(\"g1\")'"


# - Check that the clients have the expected strictly increasing revision numbers
# after a pull.

sh % "cd ../client"
sh % "hg pull -q 'ssh://user@dummy/master'"
sh % "checkglobalrevs"

sh % "cd ../client2"
sh % "hg pull -q 'ssh://user@dummy/master2'"
sh % "checkglobalrevs"


# Test resolving commits based on the strictly increasing global revision numbers.

# - Test that incorrect lookups result in errors.

sh % "cd ../client"

sh % "hg log -r 'globalrev()'" == r"""
    hg: parse error: globalrev takes one argument
    [255]"""

sh % "hg log -r 'globalrev(1, 2)'" == r"""
    hg: parse error: globalrev takes one argument
    [255]"""

sh % "hg log -r 'globalrev(invalid_input_type)'" == r"""
    hg: parse error: the argument to globalrev() must be a number
    [255]"""

sh % "hg log -r munknown" == r"""
    abort: unknown revision 'munknown'!
    (if munknown is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""


# - Test that correct lookups work as expected.


def testlookup():
    output = shlib.hg("log", "--rev=all()", "--template={globalrev}\n").split("\n")
    output = list(int(l) for l in output if l)
    assert output, "not globalrevs"

    for globalrev in output:
        result = getglobalrev("globalrev(%s)" % globalrev)
        assert result == globalrev, (
            "globalrev revset doesn't roundtrip: globalrev(%s) == %s"
            % (globalrev, result)
        )

        result = getglobalrev("m%s" % globalrev)
        assert result == globalrev, "globalrev revset doesn't roundtrip: m%s == %s" % (
            globalrev,
            result,
        )


testlookup()

# - Test that non existent global revision numbers do not resolve to any commit in
# the repository. In particular, lets test fetching the commit corresponding to
# global revision number 4999 which should not exist as the counting starts from
# 5000 in our test cases.

sh % "hg log -r 'globalrev(4999)'"

sh % "hg log -r m4999" == r"""
    abort: unknown revision 'm4999'!
    (if m4999 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""

sh % "hg log -r m1+m2" == r"""
    abort: unknown revision 'm1'!
    (if m1 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""

sh % "hg log -r 'globalrev(-1)'"


# - Creating a bookmark with prefix `m1` should still work.

sh % "hg bookmark -r null m1"
sh % "hg log -r m1 -T '{node}\\n'" == "0000000000000000000000000000000000000000"


# - Test globalrevs extension with read only configuration on the first server.

# - Configure the first server to have read only mode for globalrevs extension.

sh % "cd ../master"
sh % "cp .hg/hgrc.bak .hg/hgrc"
sh % "cat" << r"""
[globalrevs]
readonly = True
""" >> ".hg/hgrc"


# - Queries not involving writing data to commits should still work.

testlookup()


# Test bypassing hgsql extension on the first server.

# - Configure the first server to bypass hgsql extension.

sh % "mv .hg/hgrc.bak .hg/hgrc"
sh % "cat" << r"""
[hgsql]
bypass = True
""" >> ".hg/hgrc"


# - Queries not involving the hgsql extension should still work.

testlookup()


# Test that the global revisions are only effective beyond the `startrev`
# configuration in the globalrevs extension.

# - Helper function to get the globalrev for the first globalrev based commit.


def firstvalidglobalrevcommit(startrev):
    output = shlib.hg(
        "log",
        "--rev=all()",
        "--config=globalrevs.startrev=%s" % startrev,
        "--template={globalrev}\n",
    ).split("\n")
    output = list(l for l in output if l)
    if output:
        return output[0]
    return ""


shlib.firstvalidglobalrevcommit = firstvalidglobalrevcommit

# - If the `startrev` is less than the first globalrev based commit i.e. 5000 then
# effectively all globalrevs based commits in the repository have valid global
# revision numbers.

sh % "firstvalidglobalrevcommit 4999" == "5000"


# - If the `startrev` is equal to the first globalrev based commit i.e. 5000 then
# effectively all globalrevs based commits in the repository have valid global
# revision numbers.

sh % "firstvalidglobalrevcommit 5000" == "5000"


# - If the `startrev` is greater than the first globalrev based commit i.e. 5000
# then effectively only the globalrevs based commit in the repository >=
# `startrev` have valid global revision numbers.

sh % "firstvalidglobalrevcommit 5003" == "5003"


# - If the `startrev` is greater than the last globalrev based commit i.e. 5009
# then there is no commit which has a valid global revision number in the
# repository.

sh % "firstvalidglobalrevcommit 5010"


# - Configure the repository with `startrev` as 5005.

sh % "cat" << r"""
[globalrevs]
startrev = 5005
""" >> ".hg/hgrc"


# - Test that lookup works for commits with  globalrev >= `startrev`.

sh % "getglobalrev 'globalrev(5006)'" == "5006"

sh % "getglobalrev m5005" == "5005"


# - Test that lookup fails for commits with globalrev < `startrev`.

sh % "getglobalrev 'globalrev(5003)'"

sh % "getglobalrev m5004" == r"""
    abort: unknown revision 'm5004'!
    (if m5004 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""


# - Test that the lookup works as expected when the configuration
# `globalrevs.fastlookup` is true.

sh % "cd ../client"
sh % "setconfig 'globalrevs.fastlookup=True'"

testlookup()

sh % "getglobalrev 'globalrev(4999)'"

sh % "getglobalrev 'globalrev(-1)'"

sh % "hg updateglobalrevmeta"

testlookup()

sh % "getglobalrev 'globalrev(4999)'"

sh % "getglobalrev 'globalrev(-1)'"


# Test that the `svnrev` revset and keyword works as expected.

# - The test repository is not backed by Subversion. Therefore, requesting the
# `svnrev` for a commit via the `svnrev` template keyword should resolve to the
# `globalrev` for the commit. The `globalrev` keyword template should also
# resolve the `globalrev` for the commit as expected.

sh % "hg log -G -r m5007 -T 'svnrev:{svnrev} globalrev:{globalrev}\\n'" == r"""
    o  svnrev:5007 globalrev:5007
    |
    ~"""

sh % "hg log -G -r r5007 -T 'svnrev:{svnrev} globalrev:{globalrev}\\n'" == r"""
    o  svnrev:5007 globalrev:5007
    |
    ~"""

sh % "hg log -G -r 'svnrev(5007)' -T 'svnrev:{svnrev} globalrev:{globalrev}\\n'" == r"""
    o  svnrev:5007 globalrev:5007
    |
    ~"""
