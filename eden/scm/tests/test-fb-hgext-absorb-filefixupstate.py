from __future__ import absolute_import

import itertools

from edenscm.hgext import absorb


class simplefctx(object):
    def __init__(self, content):
        self.content = content

    def data(self):
        return self.content


def insertreturns(x):
    # insert "\n"s after each single char
    if isinstance(x, str):
        return "".join(ch + "\n" for ch in x)
    else:
        return map(insertreturns, x)


def removereturns(x):
    # the revert of "insertreturns"
    if isinstance(x, str):
        return x.replace("\n", "")
    else:
        return map(removereturns, x)


def assertlistequal(lhs, rhs, decorator=lambda x: x):
    if lhs != rhs:
        raise RuntimeError(
            "mismatch:\n actual:   %r\n expected: %r"
            % tuple(map(decorator, [lhs, rhs]))
        )


def testfilefixup(oldcontents, workingcopy, expectedcontents, fixups=None):
    """([str], str, [str], [(rev, a1, a2, b1, b2)]?) -> None

    workingcopy is a string, of which every character denotes a single line.

    oldcontents, expectedcontents are lists of strings, every character of
    every string denots a single line.

    if fixups is not None, it's the expected fixups list and will be checked.
    """
    expectedcontents = insertreturns(expectedcontents)
    oldcontents = insertreturns(oldcontents)
    workingcopy = insertreturns(workingcopy)
    state = absorb.filefixupstate(map(simplefctx, oldcontents), "path")
    state.diffwith(simplefctx(workingcopy))
    if fixups is not None:
        assertlistequal(state.fixups, fixups)
    state.apply()
    assertlistequal(state.finalcontents, expectedcontents, removereturns)


def buildcontents(linesrevs):
    # linesrevs: [(linecontent : str, revs : [int])]
    revs = set(itertools.chain(*[revs for line, revs in linesrevs]))
    return [""] + ["".join([l for l, rs in linesrevs if r in rs]) for r in sorted(revs)]


# input case 0: one single commit
case0 = ["", "11"]

# replace a single chunk
testfilefixup(case0, "", ["", ""])
testfilefixup(case0, "2", ["", "2"])
testfilefixup(case0, "22", ["", "22"])
testfilefixup(case0, "222", ["", "222"])

# input case 1: 3 lines, each commit adds one line
case1 = buildcontents([("1", [1, 2, 3]), ("2", [2, 3]), ("3", [3])])

# 1:1 line mapping
testfilefixup(case1, "123", case1)
testfilefixup(case1, "12c", ["", "1", "12", "12c"])
testfilefixup(case1, "1b3", ["", "1", "1b", "1b3"])
testfilefixup(case1, "1bc", ["", "1", "1b", "1bc"])
testfilefixup(case1, "a23", ["", "a", "a2", "a23"])
testfilefixup(case1, "a2c", ["", "a", "a2", "a2c"])
testfilefixup(case1, "ab3", ["", "a", "ab", "ab3"])
testfilefixup(case1, "abc", ["", "a", "ab", "abc"])

# non 1:1 edits
testfilefixup(case1, "abcd", case1)
testfilefixup(case1, "ab", case1)

# deletion
testfilefixup(case1, "", ["", "", "", ""])
testfilefixup(case1, "1", ["", "1", "1", "1"])
testfilefixup(case1, "2", ["", "", "2", "2"])
testfilefixup(case1, "3", ["", "", "", "3"])
testfilefixup(case1, "13", ["", "1", "1", "13"])

# replaces
testfilefixup(case1, "1bb3", ["", "1", "1bb", "1bb3"])

# (confusing) replaces
testfilefixup(case1, "1bbb", case1)
testfilefixup(case1, "bbbb", case1)
testfilefixup(case1, "bbb3", case1)
testfilefixup(case1, "1b", case1)
testfilefixup(case1, "bb", case1)
testfilefixup(case1, "b3", case1)

# insertions at the beginning and the end
testfilefixup(case1, "123c", ["", "1", "12", "123c"])
testfilefixup(case1, "a123", ["", "a1", "a12", "a123"])

# (confusing) insertions
testfilefixup(case1, "1a23", case1)
testfilefixup(case1, "12b3", case1)

# input case 2: delete in the middle
case2 = buildcontents([("11", [1, 2]), ("22", [1]), ("33", [1, 2])])

# deletion (optimize code should make it 2 chunks)
testfilefixup(case2, "", ["", "22", ""], fixups=[(4, 0, 2, 0, 0), (4, 2, 4, 0, 0)])

# 1:1 line mapping
testfilefixup(case2, "aaaa", ["", "aa22aa", "aaaa"])

# non 1:1 edits
# note: unlike case0, the chunk is not "continuous" and no edit allowed
testfilefixup(case2, "aaa", case2)

# input case 3: rev 3 reverts rev 2
case3 = buildcontents([("1", [1, 2, 3]), ("2", [2]), ("3", [1, 2, 3])])

# 1:1 line mapping
testfilefixup(case3, "13", case3)
testfilefixup(case3, "1b", ["", "1b", "12b", "1b"])
testfilefixup(case3, "a3", ["", "a3", "a23", "a3"])
testfilefixup(case3, "ab", ["", "ab", "a2b", "ab"])

# non 1:1 edits
testfilefixup(case3, "a", case3)
testfilefixup(case3, "abc", case3)

# deletion
testfilefixup(case3, "", ["", "", "2", ""])

# insertion
testfilefixup(case3, "a13c", ["", "a13c", "a123c", "a13c"])

# input case 4: a slightly complex case
case4 = buildcontents(
    [
        ("1", [1, 2, 3]),
        ("2", [2, 3]),
        ("3", [1, 2]),
        ("4", [1, 3]),
        ("5", [3]),
        ("6", [2, 3]),
        ("7", [2]),
        ("8", [2, 3]),
        ("9", [3]),
    ]
)

testfilefixup(case4, "1245689", case4)
testfilefixup(case4, "1a2456bbb", case4)
testfilefixup(case4, "1abc5689", case4)
testfilefixup(case4, "1ab5689", ["", "134", "1a3678", "1ab5689"])
testfilefixup(case4, "aa2bcd8ee", ["", "aa34", "aa23d78", "aa2bcd8ee"])
testfilefixup(case4, "aa2bcdd8ee", ["", "aa34", "aa23678", "aa24568ee"])
testfilefixup(case4, "aaaaaa", case4)
testfilefixup(case4, "aa258b", ["", "aa34", "aa2378", "aa258b"])
testfilefixup(case4, "25bb", ["", "34", "23678", "25689"])
testfilefixup(case4, "27", ["", "34", "23678", "245689"])
testfilefixup(case4, "28", ["", "34", "2378", "28"])
testfilefixup(case4, "", ["", "34", "37", ""])

# input case 5: replace a small chunk which is near a deleted line
case5 = buildcontents([("12", [1, 2]), ("3", [1]), ("4", [1, 2])])

testfilefixup(case5, "1cd4", ["", "1cd34", "1cd4"])

# input case 6: base "changeset" is immutable
case6 = ["1357", "0125678"]

testfilefixup(case6, "0125678", case6)
testfilefixup(case6, "0a25678", case6)
testfilefixup(case6, "0a256b8", case6)
testfilefixup(case6, "abcdefg", ["1357", "a1c5e7g"])
testfilefixup(case6, "abcdef", case6)
testfilefixup(case6, "", ["1357", "157"])
testfilefixup(case6, "0123456789", ["1357", "0123456789"])

# input case 7: change an empty file
case7 = [""]

testfilefixup(case7, "1", case7)
