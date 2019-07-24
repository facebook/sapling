# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

sh % "enable commitextras"
sh % "setconfig 'ui.allowemptycommit=1'"

sh % "HGENCODING=utf-8"

sh % "cat" << r'''
import edenscm.mercurial.revset

baseset = edenscm.mercurial.revset.baseset

def r3232(repo, subset, x):
    """"simple revset that return [3,2,3,2]

    revisions duplicated on purpose.
    """
    if 3 not in subset:
       if 2 in subset:
           return baseset([2,2])
       return baseset()
    return baseset([3,3,2,2])

edenscm.mercurial.revset.symbols['r3232'] = r3232
''' > "testrevset.py"
sh % "cat" << r"""
[extensions]
testrevset=$TESTTMP/testrevset.py
""" >> "$HGRCPATH"


def _try(*args):
    return sh.hg("debugrevspec", "--debug", *args)


def trylist(*args):
    return sh.hg("debugrevlistspec", "--debug", *args)


def log(arg):
    return sh.hg("log", "-T", "{rev}\n", "-r", arg)


_currentbranch = None


def setbranch(branch):
    global _currentbranch
    _currentbranch = branch
    # "hg tag" reads this file. Ideally the in-repo tag feature goes way too.
    open(".hg/branch", "wb").write("%s\n" % branch)


def commit(*args):
    if _currentbranch:
        sh.hg("commit", "--extra=branch=%s" % _currentbranch, *args)
        # silent warnings about conflicted names
        sh.hg("tag", "-q", "--local", "--", _currentbranch)
    else:
        sh.hg("commit", *args)


shlib.__dict__.update(
    {
        "try": _try,
        "trylist": trylist,
        "log": log,
        "setbranch": setbranch,
        "commit": commit,
    }
)

# extension to build '_intlist()' and '_hexlist()', which is necessary because
# these predicates use '\0' as a separator:

sh % "cat" << r"""
from __future__ import absolute_import
from edenscm.mercurial import (
    node as nodemod,
    registrar,
    revset,
    revsetlang,
    smartset,
)
cmdtable = {}
command = registrar.command(cmdtable)
@command(b'debugrevlistspec',
    [('', 'optimize', None, 'print parsed tree after optimizing'),
     ('', 'bin', None, 'unhexlify arguments')])
def debugrevlistspec(ui, repo, fmt, *args, **opts):
    if opts['bin']:
        args = map(nodemod.bin, args)
    expr = revsetlang.formatspec(fmt, list(args))
    if ui.verbose:
        tree = revsetlang.parse(expr, lookup=repo.__contains__)
        ui.note(revsetlang.prettyformat(tree), "\n")
        if opts["optimize"]:
            opttree = revsetlang.optimize(revsetlang.analyze(tree))
            ui.note("* optimized:\n", revsetlang.prettyformat(opttree),
                    "\n")
    func = revset.match(ui, expr, repo)
    revs = func(repo)
    if ui.verbose:
        ui.note("* set:\n", smartset.prettyformat(revs), "\n")
    for c in revs:
        ui.write("%s\n" % c)
""" > "debugrevlistspec.py"
sh % "cat" << r"""
[extensions]
debugrevlistspec = $TESTTMP/debugrevlistspec.py
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

sh % "echo a" > "a"
sh % "setbranch a"
sh % "commit -Aqm0"

sh % "echo b" > "b"
sh % "setbranch b"
sh % "commit -Aqm1"

sh % "rm a"
sh % "setbranch a-b-c-"
sh % "commit -Aqm2 -u Bob"

sh % "hg log -r 'extra('\\''branch'\\'', '\\''a-b-c-'\\'')' --template '{rev}\\n'" == "2"
sh % "hg log -r 'extra('\\''branch'\\'')' --template '{rev}\\n'" == r"""
    0
    1
    2"""
sh % "hg log -r 'extra('\\''branch'\\'', '\\''re:a'\\'')' --template '{rev} {branch}\\n'" == r"""
    0 a
    2 a-b-c-"""

sh % "hg co 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "setbranch +a+b+c+"
sh % "commit -Aqm3"

sh % "hg co -C 2" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo bb" > "b"
sh % "setbranch -a-b-c-"
sh % "commit -Aqm4 -d 'May 12 2005'"

sh % "hg co -C 3" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "setbranch '!a/b/c/'"
sh % "commit '-Aqm5 bug'"

sh % "hg merge 4" == r"""
    1 files updated, 0 files merged, 1 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "setbranch _a_b_c_"
sh % "commit '-Aqm6 issue619'"

sh % "setbranch .a.b.c."
sh % "commit -Aqm7"

sh % "setbranch all"

sh % "hg co 4" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "setbranch '\xc3\xa9'"
sh % "commit -Aqm9"

sh % "hg tag -fr6 1.0"
sh % "hg bookmark -r6 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"

sh % "hg clone --quiet -U -r 7 . ../remote1"
sh % "hg clone --quiet -U -r 8 . ../remote2"
sh % "echo '[paths]'" >> ".hg/hgrc"
sh % "echo 'default = ../remote1'" >> ".hg/hgrc"

# trivial

sh % "try '0:1'" == r"""
    (range
      (symbol '0')
      (symbol '1'))
    * set:
    <spanset+ 0:2>
    0
    1"""
sh % "try --optimize ':'" == r"""
    (rangeall
      None)
    * optimized:
    (rangeall
      None)
    * set:
    <spanset+ 0:10>
    0
    1
    2
    3
    4
    5
    6
    7
    8
    9"""
sh % "try '3::6'" == r"""
    (dagrange
      (symbol '3')
      (symbol '6'))
    * set:
    <baseset+ [3, 5, 6]>
    3
    5
    6"""
sh % "try '0|1|2'" == r"""
    (or
      (list
        (symbol '0')
        (symbol '1')
        (symbol '2')))
    * set:
    <baseset [0, 1, 2]>
    0
    1
    2"""

# names that should work without quoting

sh % "try a" == r"""
    (symbol 'a')
    * set:
    <baseset [0]>
    0"""
sh % "try b-a" == r"""
    (minus
      (symbol 'b')
      (symbol 'a'))
    * set:
    <filteredset
      <baseset [1]>,
      <not
        <baseset [0]>>>
    1"""
sh % "try _a_b_c_" == r"""
    (symbol '_a_b_c_')
    * set:
    <baseset [6]>
    6"""
sh % "try _a_b_c_-a" == r"""
    (minus
      (symbol '_a_b_c_')
      (symbol 'a'))
    * set:
    <filteredset
      <baseset [6]>,
      <not
        <baseset [0]>>>
    6"""
sh % "try .a.b.c." == r"""
    (symbol '.a.b.c.')
    * set:
    <baseset [7]>
    7"""
sh % "try .a.b.c.-a" == r"""
    (minus
      (symbol '.a.b.c.')
      (symbol 'a'))
    * set:
    <filteredset
      <baseset [7]>,
      <not
        <baseset [0]>>>
    7"""

# names that should be caught by fallback mechanism

sh % "try -- -a-b-c-" == r"""
    (symbol '-a-b-c-')
    * set:
    <baseset [4]>
    4"""
sh % "log -a-b-c-" == "4"
sh % "try +a+b+c+" == r"""
    (symbol '+a+b+c+')
    * set:
    <baseset [3]>
    3"""
sh % "try '+a+b+c+:'" == r"""
    (rangepost
      (symbol '+a+b+c+'))
    * set:
    <spanset+ 3:10>
    3
    4
    5
    6
    7
    8
    9"""
sh % "try ':+a+b+c+'" == r"""
    (rangepre
      (symbol '+a+b+c+'))
    * set:
    <spanset+ 0:4>
    0
    1
    2
    3"""
sh % "try -- '-a-b-c-:+a+b+c+'" == r"""
    (range
      (symbol '-a-b-c-')
      (symbol '+a+b+c+'))
    * set:
    <spanset- 3:5>
    4
    3"""
sh % "log '-a-b-c-:+a+b+c+'" == r"""
    4
    3"""

sh % "try -- -a-b-c--a" == r"""
    (minus
      (minus
        (minus
          (negate
            (symbol 'a'))
          (symbol 'b'))
        (symbol 'c'))
      (negate
        (symbol 'a')))
    abort: unknown revision '-a'!
    (if -a is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "try '\xc3\xa9'" == r"""
    (symbol '\xc3\xa9')
    * set:
    <baseset [8]>
    8"""

# no quoting needed

sh % "log '::a-b-c-'" == r"""
    0
    1
    2"""

# quoting needed

sh % "try '\"-a-b-c-\"-a'" == r"""
    (minus
      (string '-a-b-c-')
      (symbol 'a'))
    * set:
    <filteredset
      <baseset [4]>,
      <not
        <baseset [0]>>>
    4"""

sh % "log '1 or 2'" == r"""
    1
    2"""
sh % "log '1|2'" == r"""
    1
    2"""
sh % "log '1 and 2'"
sh % "log '1&2'"
sh % "try '1&2|3'" == r"""
    (or
      (list
        (and
          (symbol '1')
          (symbol '2'))
        (symbol '3')))
    * set:
    <addset
      <baseset []>,
      <baseset [3]>>
    3"""
sh % "try '1|2&3'" == r"""
    (or
      (list
        (symbol '1')
        (and
          (symbol '2')
          (symbol '3'))))
    * set:
    <addset
      <baseset [1]>,
      <baseset []>>
    1"""
sh % "try '1&2&3'" == r"""
    (and
      (and
        (symbol '1')
        (symbol '2'))
      (symbol '3'))
    * set:
    <baseset []>"""
sh % "try '1|(2|3)'" == r"""
    (or
      (list
        (symbol '1')
        (group
          (or
            (list
              (symbol '2')
              (symbol '3'))))))
    * set:
    <addset
      <baseset [1]>,
      <baseset [2, 3]>>
    1
    2
    3"""
sh % "log 1.0" == "6"
sh % "log a" == "0"
sh % "log 2785f51ee" == "0"
sh % "log 'date(2005)'" == "4"
sh % "log 'date(this is a test)'" == r"""
    hg: parse error at 10: unexpected token: symbol
    (date(this is a test)
               ^ here)
    [255]"""
sh % "log 'date()'" == r"""
    hg: parse error: date requires a string
    [255]"""
sh % "log date" == r"""
    abort: unknown revision 'date'!
    (if date is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "log 'date('" == r"""
    hg: parse error at 5: not a prefix: end
    (date(
          ^ here)
    [255]"""
sh % "log 'date(\"\\xy\")'" == r"""
    hg: parse error: invalid \x escape
    [255]"""
sh % "log 'date(tip)'" == r"""
    hg: parse error: invalid date: 'tip'
    [255]"""
sh % "log '0:date'" == r"""
    abort: unknown revision 'date'!
    (if date is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "log '::\"date\"'" == r"""
    abort: unknown revision 'date'!
    (if date is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "hg book date -r 4"
sh % "log '0:date'" == r"""
    0
    1
    2
    3
    4"""
sh % "log '::date'" == r"""
    0
    1
    2
    4"""
sh % "log '::\"date\"'" == r"""
    0
    1
    2
    4"""
sh % "log 'date(2005) and 1::'" == "4"
sh % "hg book -d date"

# function name should be a symbol

sh % "log '\"date\"(2005)'" == r"""
    hg: parse error: not a symbol
    [255]"""

# keyword arguments

sh % "log 'extra(branch, value=a)'" == "0"

sh % "log 'extra(branch, a, b)'" == r"""
    hg: parse error: extra takes at most 2 positional arguments
    [255]"""
sh % "log 'extra(a, label=b)'" == r"""
    hg: parse error: extra got multiple values for keyword argument 'label'
    [255]"""
sh % "log 'extra(label=branch, default)'" == r"""
    hg: parse error: extra got an invalid argument
    [255]"""
sh % "log 'extra(branch, foo+bar=baz)'" == r"""
    hg: parse error: extra got an invalid argument
    [255]"""
sh % "log 'extra(unknown=branch)'" == r"""
    hg: parse error: extra got an unexpected keyword argument 'unknown'
    [255]"""

sh % "try 'foo=bar|baz'" == r"""
    (keyvalue
      (symbol 'foo')
      (or
        (list
          (symbol 'bar')
          (symbol 'baz'))))
    hg: parse error: can't use a key-value pair in this context
    [255]"""

#  right-hand side should be optimized recursively

sh % "try --optimize 'foo=(not public())'" == r"""
    (keyvalue
      (symbol 'foo')
      (group
        (not
          (func
            (symbol 'public')
            None))))
    * optimized:
    (keyvalue
      (symbol 'foo')
      (func
        (symbol '_notpublic')
        None))
    hg: parse error: can't use a key-value pair in this context
    [255]"""

# relation-subscript operator has the highest binding strength (as function call):

sh % "hg debugrevspec -p parsed 'tip:tip^#generations[-1]'" == r"""
    * parsed:
    (range
      (symbol 'tip')
      (relsubscript
        (parentpost
          (symbol 'tip'))
        (symbol 'generations')
        (negate
          (symbol '1'))))
    9
    8
    7
    6
    5
    4"""

sh % "hg debugrevspec -p parsed --no-show-revs 'not public()#generations[0]'" == r"""
    * parsed:
    (not
      (relsubscript
        (func
          (symbol 'public')
          None)
        (symbol 'generations')
        (symbol '0')))"""

# left-hand side of relation-subscript operator should be optimized recursively:

sh % "hg debugrevspec -p analyzed -p optimized --no-show-revs '(not public())#generations[0]'" == r"""
    * analyzed:
    (relsubscript
      (not
        (func
          (symbol 'public')
          None))
      (symbol 'generations')
      (symbol '0'))
    * optimized:
    (relsubscript
      (func
        (symbol '_notpublic')
        None)
      (symbol 'generations')
      (symbol '0'))"""

# resolution of subscript and relation-subscript ternary operators:

sh % "hg debugrevspec -p analyzed 'tip[0]'" == r"""
    * analyzed:
    (subscript
      (symbol 'tip')
      (symbol '0'))
    hg: parse error: can't use a subscript in this context
    [255]"""

sh % "hg debugrevspec -p analyzed 'tip#rel[0]'" == r"""
    * analyzed:
    (relsubscript
      (symbol 'tip')
      (symbol 'rel')
      (symbol '0'))
    hg: parse error: unknown identifier: rel
    [255]"""

sh % "hg debugrevspec -p analyzed '(tip#rel)[0]'" == r"""
    * analyzed:
    (subscript
      (relation
        (symbol 'tip')
        (symbol 'rel'))
      (symbol '0'))
    hg: parse error: can't use a subscript in this context
    [255]"""

sh % "hg debugrevspec -p analyzed 'tip#rel[0][1]'" == r"""
    * analyzed:
    (subscript
      (relsubscript
        (symbol 'tip')
        (symbol 'rel')
        (symbol '0'))
      (symbol '1'))
    hg: parse error: can't use a subscript in this context
    [255]"""

sh % "hg debugrevspec -p analyzed 'tip#rel0#rel1[1]'" == r"""
    * analyzed:
    (relsubscript
      (relation
        (symbol 'tip')
        (symbol 'rel0'))
      (symbol 'rel1')
      (symbol '1'))
    hg: parse error: unknown identifier: rel1
    [255]"""

sh % "hg debugrevspec -p analyzed 'tip#rel0[0]#rel1[1]'" == r"""
    * analyzed:
    (relsubscript
      (relsubscript
        (symbol 'tip')
        (symbol 'rel0')
        (symbol '0'))
      (symbol 'rel1')
      (symbol '1'))
    hg: parse error: unknown identifier: rel1
    [255]"""

# parse errors of relation, subscript and relation-subscript operators:

sh % "hg debugrevspec '[0]'" == r"""
    hg: parse error at 0: not a prefix: [
    ([0]
     ^ here)
    [255]"""
sh % "hg debugrevspec '.#'" == r"""
    hg: parse error at 2: not a prefix: end
    (.#
       ^ here)
    [255]"""
sh % "hg debugrevspec '#rel'" == r"""
    hg: parse error at 0: not a prefix: #
    (#rel
     ^ here)
    [255]"""
sh % "hg debugrevspec '.#rel[0'" == r"""
    hg: parse error at 7: unexpected token: end
    (.#rel[0
            ^ here)
    [255]"""
sh % "hg debugrevspec '.]'" == r"""
    hg: parse error at 1: invalid token
    (.]
      ^ here)
    [255]"""

sh % "hg debugrevspec '.#generations[a]'" == r"""
    hg: parse error: relation subscript must be an integer
    [255]"""
sh % "hg debugrevspec '.#generations[1-2]'" == r"""
    hg: parse error: relation subscript must be an integer
    [255]"""

# parsed tree at stages:

sh % "hg debugrevspec -p all '()'" == r"""
    * parsed:
    (group
      None)
    * expanded:
    (group
      None)
    * concatenated:
    (group
      None)
    * analyzed:
    None
    * optimized:
    None
    hg: parse error: missing argument
    [255]"""

sh % "hg debugrevspec --no-optimized -p all '()'" == r"""
    * parsed:
    (group
      None)
    * expanded:
    (group
      None)
    * concatenated:
    (group
      None)
    * analyzed:
    None
    hg: parse error: missing argument
    [255]"""

sh % "hg debugrevspec -p parsed -p analyzed -p optimized '(0|1)-1'" == r"""
    * parsed:
    (minus
      (group
        (or
          (list
            (symbol '0')
            (symbol '1'))))
      (symbol '1'))
    * analyzed:
    (and
      (or
        (list
          (symbol '0')
          (symbol '1')))
      (not
        (symbol '1')))
    * optimized:
    (difference
      (func
        (symbol '_list')
        (string '0\x001'))
      (symbol '1'))
    0"""

sh % "hg debugrevspec -p unknown 0" == r"""
    abort: invalid stage name: unknown
    [255]"""

sh % "hg debugrevspec -p all --optimize 0" == r"""
    abort: cannot use --optimize with --show-stage
    [255]"""

# verify optimized tree:

sh % "hg debugrevspec --verify '0|1'"

sh % "hg debugrevspec --verify -v -p analyzed -p optimized 'r3232() & 2'" == r"""
    * analyzed:
    (and
      (func
        (symbol 'r3232')
        None)
      (symbol '2'))
    * optimized:
    (andsmally
      (func
        (symbol 'r3232')
        None)
      (symbol '2'))
    * analyzed set:
    <baseset [2]>
    * optimized set:
    <baseset [2, 2]>
    --- analyzed
    +++ optimized
     2
    +2
    [1]"""

sh % "hg debugrevspec --no-optimized --verify-optimized 0" == r"""
    abort: cannot use --verify-optimized with --no-optimized
    [255]"""

# Test that symbols only get parsed as functions if there's an opening
# parenthesis.

sh % "hg book only -r 9"
sh % "log 'only(only)'" == r"""
    8
    9"""

# ':y' behaves like '0:y', but can't be rewritten as such since the revision '0'
# may be hidden (issue5385)

sh % "try -p parsed -p analyzed ':'" == r"""
    * parsed:
    (rangeall
      None)
    * analyzed:
    (rangeall
      None)
    * set:
    <spanset+ 0:10>
    0
    1
    2
    3
    4
    5
    6
    7
    8
    9"""
sh % "try -p analyzed ':1'" == r"""
    * analyzed:
    (rangepre
      (symbol '1'))
    * set:
    <spanset+ 0:2>
    0
    1"""
sh % "try -p analyzed ':(1|2)'" == r"""
    * analyzed:
    (rangepre
      (or
        (list
          (symbol '1')
          (symbol '2'))))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""
sh % "try -p analyzed ':(1&2)'" == r"""
    * analyzed:
    (rangepre
      (and
        (symbol '1')
        (symbol '2')))
    * set:
    <baseset []>"""

# infix/suffix resolution of ^ operator (issue2884):

#  x^:y means (x^):y

sh % "try '1^:2'" == r"""
    (range
      (parentpost
        (symbol '1'))
      (symbol '2'))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""

sh % "try '1^::2'" == r"""
    (dagrange
      (parentpost
        (symbol '1'))
      (symbol '2'))
    * set:
    <baseset+ [0, 1, 2]>
    0
    1
    2"""

sh % "try '9^:'" == r"""
    (rangepost
      (parentpost
        (symbol '9')))
    * set:
    <spanset+ 8:10>
    8
    9"""

#  x^:y should be resolved before omitting group operators

sh % "try '1^(:2)'" == r"""
    (parent
      (symbol '1')
      (group
        (rangepre
          (symbol '2'))))
    hg: parse error: ^ expects a number 0, 1, or 2
    [255]"""

#  x^:y should be resolved recursively

sh % "try 'sort(1^:2)'" == r"""
    (func
      (symbol 'sort')
      (range
        (parentpost
          (symbol '1'))
        (symbol '2')))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""

sh % "try '(3^:4)^:2'" == r"""
    (range
      (parentpost
        (group
          (range
            (parentpost
              (symbol '3'))
            (symbol '4'))))
      (symbol '2'))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""

sh % "try '(3^::4)^::2'" == r"""
    (dagrange
      (parentpost
        (group
          (dagrange
            (parentpost
              (symbol '3'))
            (symbol '4'))))
      (symbol '2'))
    * set:
    <baseset+ [0, 1, 2]>
    0
    1
    2"""

sh % "try '(9^:)^:'" == r"""
    (rangepost
      (parentpost
        (group
          (rangepost
            (parentpost
              (symbol '9'))))))
    * set:
    <spanset+ 4:10>
    4
    5
    6
    7
    8
    9"""

#  x^ in alias should also be resolved

sh % "try A --config 'revsetalias.A=1^:2'" == r"""
    (symbol 'A')
    * expanded:
    (range
      (parentpost
        (symbol '1'))
      (symbol '2'))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""

sh % "try 'A:2' --config 'revsetalias.A=1^'" == r"""
    (range
      (symbol 'A')
      (symbol '2'))
    * expanded:
    (range
      (parentpost
        (symbol '1'))
      (symbol '2'))
    * set:
    <spanset+ 0:3>
    0
    1
    2"""

#  but not beyond the boundary of alias expansion, because the resolution should
#  be made at the parsing stage

sh % "try '1^A' --config 'revsetalias.A=:2'" == r"""
    (parent
      (symbol '1')
      (symbol 'A'))
    * expanded:
    (parent
      (symbol '1')
      (rangepre
        (symbol '2')))
    hg: parse error: ^ expects a number 0, 1, or 2
    [255]"""

# ancestor can accept 0 or more arguments

sh % "log 'ancestor()'"
sh % "log 'ancestor(1)'" == "1"
sh % "log 'ancestor(4,5)'" == "1"
sh % "log 'ancestor(4,5) and 4'"
sh % "log 'ancestor(0,0,1,3)'" == "0"
sh % "log 'ancestor(3,1,5,3,5,1)'" == "1"
sh % "log 'ancestor(0,1,3,5)'" == "0"
sh % "log 'ancestor(1,2,3,4,5)'" == "1"

# test ancestors

sh % "hg log -G -T '{rev}\\n' --config 'experimental.graphshorten=True'" == r"""
    @  9
    o  8
    | o  7
    | o  6
    |/|
    | o  5
    o |  4
    | o  3
    o |  2
    |/
    o  1
    o  0"""

sh % "log 'ancestors(5)'" == r"""
    0
    1
    3
    5"""
sh % "log 'ancestor(ancestors(5))'" == "0"
sh % "log '::r3232()'" == r"""
    0
    1
    2
    3"""

# test ancestors with depth limit

#  (depth=0 selects the node itself)

sh % "log 'reverse(ancestors(9, depth=0))'" == "9"

#  (interleaved: '4' would be missing if heap queue were higher depth first)

sh % "log 'reverse(ancestors(8:9, depth=1))'" == r"""
    9
    8
    4"""

#  (interleaved: '2' would be missing if heap queue were higher depth first)

sh % "log 'reverse(ancestors(7+8, depth=2))'" == r"""
    8
    7
    6
    5
    4
    2"""

#  (walk example above by separate queries)

sh % "log 'reverse(ancestors(8, depth=2)) + reverse(ancestors(7, depth=2))'" == r"""
    8
    4
    2
    7
    6
    5"""

#  (walk 2nd and 3rd ancestors)

sh % "log 'reverse(ancestors(7, depth=3, startdepth=2))'" == r"""
    5
    4
    3
    2"""

#  (interleaved: '4' would be missing if higher-depth ancestors weren't scanned)

sh % "log 'reverse(ancestors(7+8, depth=2, startdepth=2))'" == r"""
    5
    4
    2"""

#  (note that 'ancestors(x, depth=y, startdepth=z)' does not identical to
#  'ancestors(x, depth=y) - ancestors(x, depth=z-1)' because a node may have
#  multiple depths)

sh % "log 'reverse(ancestors(7+8, depth=2) - ancestors(7+8, depth=1))'" == r"""
    5
    2"""

# test bad arguments passed to ancestors()

sh % "log 'ancestors(., depth=-1)'" == r"""
    hg: parse error: negative depth
    [255]"""
sh % "log 'ancestors(., depth=foo)'" == r"""
    hg: parse error: ancestors expects an integer depth
    [255]"""

# test descendants

sh % "hg log -G -T '{rev}\\n' --config 'experimental.graphshorten=True'" == r"""
    @  9
    o  8
    | o  7
    | o  6
    |/|
    | o  5
    o |  4
    | o  3
    o |  2
    |/
    o  1
    o  0"""

#  (null is ultimate root and has optimized path)

sh % "log 'null:4 & descendants(null)'" == r"""
    -1
    0
    1
    2
    3
    4"""

#  (including merge)

sh % "log ':8 & descendants(2)'" == r"""
    2
    4
    6
    7
    8"""

#  (multiple roots)

sh % "log ':8 & descendants(2+5)'" == r"""
    2
    4
    5
    6
    7
    8"""

# test descendants with depth limit

#  (depth=0 selects the node itself)

sh % "log 'descendants(0, depth=0)'" == "0"
sh % "log 'null: & descendants(null, depth=0)'" == "-1"

#  (p2 = null should be ignored)

sh % "log 'null: & descendants(null, depth=2)'" == r"""
    -1
    0
    1"""

#  (multiple paths: depth(6) = (2, 3))

sh % "log 'descendants(1+3, depth=2)'" == r"""
    1
    2
    3
    4
    5
    6"""

#  (multiple paths: depth(5) = (1, 2), depth(6) = (2, 3))

sh % "log 'descendants(3+1, depth=2, startdepth=2)'" == r"""
    4
    5
    6"""

#  (multiple depths: depth(6) = (0, 2, 4), search for depth=2)

sh % "log 'descendants(0+3+6, depth=3, startdepth=1)'" == r"""
    1
    2
    3
    4
    5
    6
    7"""

#  (multiple depths: depth(6) = (0, 4), no match)

sh % "log 'descendants(0+6, depth=3, startdepth=1)'" == r"""
    1
    2
    3
    4
    5
    7"""

# test ancestors/descendants relation subscript:

sh % "log 'tip#generations[0]'" == "9"
sh % "log '.#generations[-1]'" == "8"
sh % "log '.#g[(-1)]'" == "8"

sh % "hg debugrevspec -p parsed 'roots(:)#g[2]'" == r"""
    * parsed:
    (relsubscript
      (func
        (symbol 'roots')
        (rangeall
          None))
      (symbol 'g')
      (symbol '2'))
    2
    3"""

# test author

sh % "log 'author(bob)'" == "2"
sh % "log 'author(\"re:bob|test\")'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    8
    9"""
sh % "log 'author(r\"re:\\S\")'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    8
    9"""
sh % "log 'children(ancestor(4,5))'" == r"""
    2
    3"""

sh % "log 'children(4)'" == r"""
    6
    8"""
sh % "log 'children(null)'" == "0"

sh % "log 'closed()'"
sh % "log 'contains(a)'" == r"""
    0
    1
    3
    5"""
sh % "log 'contains(\"../repo/a\")'" == r"""
    0
    1
    3
    5"""
sh % "log 'desc(B)'" == "5"
sh % "hg log -r 'desc(r\"re:S?u\")' --template '{rev} {desc|firstline}\\n'" == r"""
    5 5 bug
    6 6 issue619"""
sh % "log 'descendants(2 or 3)'" == r"""
    2
    3
    4
    5
    6
    7
    8
    9"""
sh % "log 'file(\"b*\")'" == r"""
    1
    4"""
sh % "log 'filelog(\"b\")'" == r"""
    1
    4"""
sh % "log 'filelog(\"../repo/b\")'" == r"""
    1
    4"""
sh % "log 'follow()'" == r"""
    0
    1
    2
    4
    8
    9"""
sh % "log 'grep(\"issue\\d+\")'" == "6"
sh % "try 'grep(\"(\")'" == r"""
    (func
      (symbol 'grep')
      (string '('))
    hg: parse error: invalid match pattern: unbalanced parenthesis
    [255]"""
sh % "try 'grep(\"\\bissue\\d+\")'" == r"""
    (func
      (symbol 'grep')
      (string '\x08issue\\d+'))
    * set:
    <filteredset
      <fullreposet+ 0:10>,
      <grep '\x08issue\\d+'>>"""
sh % "try 'grep(r\"\\bissue\\d+\")'" == r"""
    (func
      (symbol 'grep')
      (string '\\bissue\\d+'))
    * set:
    <filteredset
      <fullreposet+ 0:10>,
      <grep '\\bissue\\d+'>>
    6"""
sh % "try 'grep(r\"\\\")'" == r"""
    hg: parse error at 7: unterminated string
    (grep(r"\")
            ^ here)
    [255]"""
sh % "log 'head()'" == r"""
    7
    9"""
sh % "log 'heads(6::)'" == "7"
sh % "log 'keyword(issue)'" == "6"
sh % "log 'keyword(\"test a\")'"

# Test first (=limit) and last

sh % "log 'limit(head(), 1)'" == "7"
sh % "log 'limit(author(\"re:bob|test\"), 3, 5)'" == r"""
    5
    6
    7"""
sh % "log 'limit(author(\"re:bob|test\"), offset=6)'" == "6"
sh % "log 'limit(author(\"re:bob|test\"), offset=10)'"
sh % "log 'limit(all(), 1, -1)'" == r"""
    hg: parse error: negative offset
    [255]"""
sh % "log 'limit(all(), -1)'" == r"""
    hg: parse error: negative number to select
    [255]"""
sh % "log 'limit(all(), 0)'"

sh % "log 'last(all(), -1)'" == r"""
    hg: parse error: negative number to select
    [255]"""
sh % "log 'last(all(), 0)'"
sh % "log 'last(all(), 1)'" == "9"
sh % "log 'last(all(), 2)'" == r"""
    8
    9"""

# Test smartset.slice() by first/last()

#  (using unoptimized set, filteredset as example)

sh % "hg debugrevspec --no-show-revs -s '0:7 & all()'" == r"""
    * set:
    <filteredset
      <spanset+ 0:8>,
      <spanset+ 0:10>>"""
sh % "log 'limit(0:7 & all(), 3, 4)'" == r"""
    4
    5
    6"""
sh % "log 'limit(7:0 & all(), 3, 4)'" == r"""
    3
    2
    1"""
sh % "log 'last(0:7 & all(), 2)'" == r"""
    6
    7"""

#  (using baseset)

sh % "hg debugrevspec --no-show-revs -s 0+1+2+3+4+5+6+7" == r"""
    * set:
    <baseset [0, 1, 2, 3, 4, 5, 6, 7]>"""
sh % "hg debugrevspec --no-show-revs -s '0::7'" == r"""
    * set:
    <baseset+ [0, 1, 2, 3, 4, 5, 6, 7]>"""
sh % "log 'limit(0+1+2+3+4+5+6+7, 3, 4)'" == r"""
    4
    5
    6"""
sh % "log 'limit(sort(0::7, rev), 3, 4)'" == r"""
    4
    5
    6"""
sh % "log 'limit(sort(0::7, -rev), 3, 4)'" == r"""
    3
    2
    1"""
sh % "log 'last(sort(0::7, rev), 2)'" == r"""
    6
    7"""
sh % "hg debugrevspec -s 'limit(sort(0::7, rev), 3, 6)'" == r"""
    * set:
    <baseset+ [6, 7]>
    6
    7"""
sh % "hg debugrevspec -s 'limit(sort(0::7, rev), 3, 9)'" == r"""
    * set:
    <baseset+ []>"""
sh % "hg debugrevspec -s 'limit(sort(0::7, -rev), 3, 6)'" == r"""
    * set:
    <baseset- [0, 1]>
    1
    0"""
sh % "hg debugrevspec -s 'limit(sort(0::7, -rev), 3, 9)'" == r"""
    * set:
    <baseset- []>"""
sh % "hg debugrevspec -s 'limit(0::7, 0)'" == r"""
    * set:
    <baseset+ []>"""

#  (using spanset)

sh % "hg debugrevspec --no-show-revs -s '0:7'" == r"""
    * set:
    <spanset+ 0:8>"""
sh % "log 'limit(0:7, 3, 4)'" == r"""
    4
    5
    6"""
sh % "log 'limit(7:0, 3, 4)'" == r"""
    3
    2
    1"""
sh % "log 'limit(0:7, 3, 6)'" == r"""
    6
    7"""
sh % "log 'limit(7:0, 3, 6)'" == r"""
    1
    0"""
sh % "log 'last(0:7, 2)'" == r"""
    6
    7"""
sh % "hg debugrevspec -s 'limit(0:7, 3, 6)'" == r"""
    * set:
    <spanset+ 6:8>
    6
    7"""
sh % "hg debugrevspec -s 'limit(0:7, 3, 9)'" == r"""
    * set:
    <spanset+ 8:8>"""
sh % "hg debugrevspec -s 'limit(7:0, 3, 6)'" == r"""
    * set:
    <spanset- 0:2>
    1
    0"""
sh % "hg debugrevspec -s 'limit(7:0, 3, 9)'" == r"""
    * set:
    <spanset- 0:0>"""
sh % "hg debugrevspec -s 'limit(0:7, 0)'" == r"""
    * set:
    <spanset+ 0:0>"""

# Test order of first/last revisions

sh % "hg debugrevspec -s 'first(4:0, 3) & 3:'" == r"""
    * set:
    <filteredset
      <spanset- 2:5>,
      <spanset+ 3:10>>
    4
    3"""

sh % "hg debugrevspec -s '3: & first(4:0, 3)'" == r"""
    * set:
    <filteredset
      <spanset+ 3:10>,
      <spanset- 2:5>>
    3
    4"""

sh % "hg debugrevspec -s 'last(4:0, 3) & :1'" == r"""
    * set:
    <filteredset
      <spanset- 0:3>,
      <spanset+ 0:2>>
    1
    0"""

sh % "hg debugrevspec -s ':1 & last(4:0, 3)'" == r"""
    * set:
    <filteredset
      <spanset+ 0:2>,
      <spanset+ 0:3>>
    0
    1"""

# Test scmutil.revsingle() should return the last revision

sh % "hg debugrevspec -s 'last(0::)'" == r"""
    * set:
    <baseset slice=0:1
      <generatorset->>
    9"""
sh % "hg identify -r '0::' --num" == "9"

# Test matching

sh % "log 'matching(6)'" == "6"
sh % "log 'matching(6:7, \"phase parents user date branch summary files description\")'" == r"""
    6
    7"""

# Testing min and max

# max: simple

sh % "log 'max(contains(a))'" == "5"

# max: simple on unordered set)

sh % "log 'max((4+0+2+5+7) and contains(a))'" == "5"

# max: no result

sh % "log 'max(contains(stringthatdoesnotappearanywhere))'"

# max: no result on unordered set

sh % "log 'max((4+0+2+5+7) and contains(stringthatdoesnotappearanywhere))'"

# min: simple

sh % "log 'min(contains(a))'" == "0"

# min: simple on unordered set

sh % "log 'min((4+0+2+5+7) and contains(a))'" == "0"

# min: empty

sh % "log 'min(contains(stringthatdoesnotappearanywhere))'"

# min: empty on unordered set

sh % "log 'min((4+0+2+5+7) and contains(stringthatdoesnotappearanywhere))'"


sh % "log 'merge()'" == "6"
sh % "log 'branchpoint()'" == r"""
    1
    4"""
sh % "log 'modifies(b)'" == "4"
sh % "log 'modifies(\"path:b\")'" == "4"
sh % "log 'modifies(\"*\")'" == r"""
    4
    6"""
sh % "log 'modifies(\"set:modified()\")'" == "4"
sh % "log 'id(5)'" == "2"
sh % "log 'only(9)'" == r"""
    8
    9"""
sh % "log 'only(8)'" == "8"
sh % "log 'only(9, 5)'" == r"""
    2
    4
    8
    9"""
sh % "log 'only(7 + 9, 5 + 2)'" == r"""
    4
    6
    7
    8
    9"""

# Test empty set input
sh % "log 'only(p2())'"
sh % "log 'only(p1(), p2())'" == r"""
    0
    1
    2
    4
    8
    9"""

# Test '%' operator

sh % "log '9%'" == r"""
    8
    9"""
sh % "log '9%5'" == r"""
    2
    4
    8
    9"""
sh % "log '(7 + 9)%(5 + 2)'" == r"""
    4
    6
    7
    8
    9"""

# Test operand of '%' is optimized recursively (issue4670)

sh % "try --optimize '8:9-8%'" == r"""
    (onlypost
      (minus
        (range
          (symbol '8')
          (symbol '9'))
        (symbol '8')))
    * optimized:
    (func
      (symbol 'only')
      (difference
        (range
          (symbol '8')
          (symbol '9'))
        (symbol '8')))
    * set:
    <baseset+ [8, 9]>
    8
    9"""
sh % "try --optimize '(9)%(5)'" == r"""
    (only
      (group
        (symbol '9'))
      (group
        (symbol '5')))
    * optimized:
    (func
      (symbol 'only')
      (list
        (symbol '9')
        (symbol '5')))
    * set:
    <baseset+ [2, 4, 8, 9]>
    2
    4
    8
    9"""

# Test the order of operations

sh % "log '7 + 9%5 + 2'" == r"""
    7
    2
    4
    8
    9"""

# Test explicit numeric revision
sh % "log 'rev(-2)'"
sh % "log 'rev(-1)'" == "-1"
sh % "log 'rev(0)'" == "0"
sh % "log 'rev(9)'" == "9"
sh % "log 'rev(10)'"
sh % "log 'rev(tip)'" == r"""
    hg: parse error: rev expects a number
    [255]"""

# Test hexadecimal revision
sh % "log 'id(2)'" == r"""
    abort: 00changelog.i@2: ambiguous identifier!
    [255]"""
sh % "log 'id(23268)'" == "4"
sh % "log 'id(2785f51eece)'" == "0"
sh % "log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532c)'" == "8"
sh % "log 'id(d5d0dcbdc4a)'"
sh % "log 'id(d5d0dcbdc4w)'"
sh % "log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532d)'"
sh % "log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532q)'"
sh % "log 'id(1.0)'"
sh % "log 'id(xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx)'"

# Test null revision
sh % "log '(null)'" == "-1"
sh % "log '(null:0)'" == r"""
    -1
    0"""
sh % "log '(0:null)'" == r"""
    0
    -1"""
sh % "log 'null::0'" == r"""
    -1
    0"""
sh % "log 'null:tip - 0:'" == "-1"
sh % "log 'null: and null::'" | "head -1" == "-1"
sh % "log 'null: or 0:'" | "head -2" == r"""
    -1
    0"""
sh % "log 'ancestors(null)'" == "-1"
sh % "log 'reverse(null:)'" | "tail -2" == r"""
    0
    -1"""
sh % "log 'first(null:)'" == "-1"
sh % "log 'min(null:)'"
# BROKEN: should be '-1'
sh % "log 'tip:null and all()'" | "tail -2" == r"""
    1
    0"""

# Test working-directory revision
sh % "hg debugrevspec 'wdir()'" == "2147483647"
sh % "hg debugrevspec 'wdir()^'" == "9"
sh % "hg up 7" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg debugrevspec 'wdir()^'" == "7"
sh % "hg debugrevspec 'wdir()^0'" == "2147483647"
sh % "hg debugrevspec 'wdir()~3'" == "5"
sh % "hg debugrevspec 'ancestors(wdir())'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    2147483647"""
sh % "hg debugrevspec 'wdir()~0'" == "2147483647"
sh % "hg debugrevspec 'p1(wdir())'" == "7"
sh % "hg debugrevspec 'p2(wdir())'"
sh % "hg debugrevspec 'parents(wdir())'" == "7"
sh % "hg debugrevspec 'wdir()^1'" == "7"
sh % "hg debugrevspec 'wdir()^2'"
sh % "hg debugrevspec 'wdir()^3'" == r"""
    hg: parse error: ^ expects a number 0, 1, or 2
    [255]"""

# DAG ranges with wdir()
sh % "hg debugrevspec 'wdir()::1'"
sh % "hg debugrevspec 'wdir()::wdir()'" == "2147483647"
sh % "hg debugrevspec 'wdir()::(1+wdir())'" == "2147483647"
sh % "hg debugrevspec '6::wdir()'" == r"""
    6
    7
    2147483647"""
sh % "hg debugrevspec '5::(wdir()+7)'" == r"""
    5
    6
    7
    2147483647"""
sh % "hg debugrevspec '(1+wdir())::(2+wdir())'" == r"""
    1
    2
    3
    4
    5
    6
    7
    2147483647"""

# For tests consistency
sh % "hg up 9" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg debugrevspec 'tip or wdir()'" == r"""
    9
    2147483647"""
sh % "hg debugrevspec '0:tip and wdir()'"
sh % "log '0:wdir()'" | "tail -3" == r"""
    8
    9
    2147483647"""
sh % "log 'wdir():0'" | "head -3" == r"""
    2147483647
    9
    8"""
sh % "log 'wdir():wdir()'" == "2147483647"
sh % "log '(all() + wdir()) & min(. + wdir())'" == "9"
sh % "log '(all() + wdir()) & max(. + wdir())'" == "2147483647"
sh % "log 'first(wdir() + .)'" == "2147483647"
sh % "log 'last(. + wdir())'" == "2147483647"

# Test working-directory integer revision and node id
# (BUG: '0:wdir()' is still needed to populate wdir revision)

sh % "hg debugrevspec '0:wdir() & 2147483647'" == "2147483647"
sh % "hg debugrevspec '0:wdir() & rev(2147483647)'" == "2147483647"
sh % "hg debugrevspec '0:wdir() & ffffffffffffffffffffffffffffffffffffffff'" == "2147483647"
sh % "hg debugrevspec '0:wdir() & ffffffffffff'" == "2147483647"
sh % "hg debugrevspec '0:wdir() & id(ffffffffffffffffffffffffffffffffffffffff)'" == "2147483647"
sh % "hg debugrevspec '0:wdir() & id(ffffffffffff)'" == "2147483647"

sh % "cd .."

# Test short 'ff...' hash collision
# (BUG: '0:wdir()' is still needed to populate wdir revision)

sh % "hg init wdir-hashcollision"
sh % "cd wdir-hashcollision"
sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> ".hg/hgrc"
sh % "echo 0" > "a"
sh % "hg ci -qAm 0"

for i in [2463, 2961, 6726, 78127]:
    sh.hg("up", "-q", "0")
    open("a", "wb").write("%s\n" % i)
    sh.hg("ci", "-qm", "%s" % i)
sh % "hg up -q null"
sh % "hg log -r '0:wdir()' -T '{rev}:{node} {shortest(node, 3)}\\n'" == r"""
    0:b4e73ffab476aa0ee32ed81ca51e07169844bc6a b4e
    1:fffbae3886c8fbb2114296380d276fd37715d571 fffba
    2:fffb6093b00943f91034b9bdad069402c834e572 fffb6
    3:fff48a9b9de34a4d64120c29548214c67980ade3 fff4
    4:ffff85cff0ff78504fcdc3c0bc10de0c65379249 ffff8
    2147483647:ffffffffffffffffffffffffffffffffffffffff fffff"""
sh % "hg debugobsolete fffbae3886c8fbb2114296380d276fd37715d571" == "obsoleted 1 changesets"

sh % "hg debugrevspec '0:wdir() & fff'" == r"""
    abort: 00changelog.i@fff: ambiguous identifier!
    [255]"""
sh % "hg debugrevspec '0:wdir() & ffff'" == r"""
    abort: 00changelog.i@ffff: ambiguous identifier!
    [255]"""
sh % "hg debugrevspec '0:wdir() & fffb'" == r"""
    abort: 00changelog.i@fffb: ambiguous identifier!
    [255]"""
# BROKEN should be '2' (node lookup uses unfiltered repo since dc25ed84bee8)
sh % "hg debugrevspec '0:wdir() & id(fffb)'" == "2"
sh % "hg debugrevspec '0:wdir() & ffff8'" == "4"
sh % "hg debugrevspec '0:wdir() & fffff'" == "2147483647"

sh % "cd .."

sh % "cd repo"

sh % "log 'outgoing()'" == r"""
    8
    9"""
sh % "log 'outgoing(\"../remote1\")'" == r"""
    8
    9"""
sh % "log 'outgoing(\"../remote2\")'" == r"""
    3
    5
    6
    7
    9"""
sh % "log 'p1(merge())'" == "5"
sh % "log 'p2(merge())'" == "4"
sh % "log 'parents(merge())'" == r"""
    4
    5"""
sh % "log 'p1(branchpoint())'" == r"""
    0
    2"""
sh % "log 'p2(branchpoint())'"
sh % "log 'parents(branchpoint())'" == r"""
    0
    2"""
sh % "log 'removes(a)'" == r"""
    2
    6"""
sh % "log 'roots(all())'" == "0"
sh % "log 'reverse(2 or 3 or 4 or 5)'" == r"""
    5
    4
    3
    2"""
sh % "log 'reverse(all())'" == r"""
    9
    8
    7
    6
    5
    4
    3
    2
    1
    0"""
sh % "log 'reverse(all()) & filelog(b)'" == r"""
    4
    1"""
sh % "log 'rev(5)'" == "5"
sh % "log 'sort(limit(reverse(all()), 3))'" == r"""
    7
    8
    9"""
sh % "log 'sort(2 or 3 or 4 or 5, date)'" == r"""
    2
    3
    5
    4"""
sh % "log 'tagged()'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    8"""
sh % "log 'tag()'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    8"""
sh % "log 'tag(1.0)'" == "6"
sh % "log 'tag(tip)'" == "9"

# Test order of revisions in compound expression
# ----------------------------------------------

# The general rule is that only the outermost (= leftmost) predicate can
# enforce its ordering requirement. The other predicates should take the
# ordering defined by it.

#  'A & B' should follow the order of 'A':

sh % "log '2:0 & 0::2'" == r"""
    2
    1
    0"""

#  'head()' combines sets in right order:

sh % "log '9:7 & head()'" == r"""
    9
    7"""

#  'x:y' takes ordering parameter into account:

sh % "try -p optimized '3:0 & 0:3 & not 2:1'" == r"""
    * optimized:
    (difference
      (and
        (range
          (symbol '3')
          (symbol '0'))
        (range
          (symbol '0')
          (symbol '3')))
      (range
        (symbol '2')
        (symbol '1')))
    * set:
    <filteredset
      <filteredset
        <spanset- 0:4>,
        <spanset+ 0:4>>,
      <not
        <spanset+ 1:3>>>
    3
    0"""

#  'a + b', which is optimized to '_list(a b)', should take the ordering of
#  the left expression:

sh % "try --optimize '2:0 & (0 + 1 + 2)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (group
        (or
          (list
            (symbol '0')
            (symbol '1')
            (symbol '2')))))
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_list')
        (string '0\x001\x002')))
    * set:
    <filteredset
      <spanset- 0:3>,
      <baseset [0, 1, 2]>>
    2
    1
    0"""

#  'A + B' should take the ordering of the left expression:

sh % "try --optimize '2:0 & (0:1 + 2)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (group
        (or
          (list
            (range
              (symbol '0')
              (symbol '1'))
            (symbol '2')))))
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (or
        (list
          (range
            (symbol '0')
            (symbol '1'))
          (symbol '2'))))
    * set:
    <filteredset
      <spanset- 0:3>,
      <addset
        <spanset+ 0:2>,
        <baseset [2]>>>
    2
    1
    0"""

#  '_intlist(a b)' should behave like 'a + b':

sh % "trylist --optimize '2:0 & %ld' 0 1 2" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_intlist')
        (string '0\x001\x002')))
    * optimized:
    (andsmally
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_intlist')
        (string '0\x001\x002')))
    * set:
    <filteredset
      <spanset- 0:3>,
      <baseset+ [0, 1, 2]>>
    2
    1
    0"""

sh % "trylist --optimize '%ld & 2:0' 0 2 1" == r"""
    (and
      (func
        (symbol '_intlist')
        (string '0\x002\x001'))
      (range
        (symbol '2')
        (symbol '0')))
    * optimized:
    (and
      (func
        (symbol '_intlist')
        (string '0\x002\x001'))
      (range
        (symbol '2')
        (symbol '0')))
    * set:
    <filteredset
      <baseset [0, 2, 1]>,
      <spanset- 0:3>>
    0
    2
    1"""

#  '_hexlist(a b)' should behave like 'a + b':

args = sh.hg("log", "-T", "{node} ", "-r0:2")
sh % (
    "trylist --optimize --bin '2:0 & %%ln' %s" % args
) == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_hexlist')
        (string '*'))) (glob)
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_hexlist')
        (string '*'))) (glob)
    * set:
    <filteredset
      <spanset- 0:3>,
      <baseset [0, 1, 2]>>
    2
    1
    0"""

args = sh.hg("log", "-T", "{node} ", "-r0+2+1")
sh % (
    "trylist --optimize --bin '%%ln & 2:0' %s" % args
) == r"""
    (and
      (func
        (symbol '_hexlist')
        (string '*')) (glob)
      (range
        (symbol '2')
        (symbol '0')))
    * optimized:
    (andsmally
      (func
        (symbol '_hexlist')
        (string '*')) (glob)
      (range
        (symbol '2')
        (symbol '0')))
    * set:
    <baseset [0, 2, 1]>
    0
    2
    1"""

#  '_list' should not go through the slow follow-order path if order doesn't
#  matter:

sh % "try -p optimized '2:0 & not (0 + 1)'" == r"""
    * optimized:
    (difference
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol '_list')
        (string '0\x001')))
    * set:
    <filteredset
      <spanset- 0:3>,
      <not
        <baseset [0, 1]>>>
    2"""

sh % "try -p optimized '2:0 & not (0:2 & (0 + 1))'" == r"""
    * optimized:
    (difference
      (range
        (symbol '2')
        (symbol '0'))
      (and
        (range
          (symbol '0')
          (symbol '2'))
        (func
          (symbol '_list')
          (string '0\x001'))))
    * set:
    <filteredset
      <spanset- 0:3>,
      <not
        <baseset [0, 1]>>>
    2"""

#  because 'present()' does nothing other than suppressing an error, the
#  ordering requirement should be forwarded to the nested expression

sh % "try -p optimized 'present(2 + 0 + 1)'" == r"""
    * optimized:
    (func
      (symbol 'present')
      (func
        (symbol '_list')
        (string '2\x000\x001')))
    * set:
    <baseset [2, 0, 1]>
    2
    0
    1"""

sh % "try --optimize '2:0 & present(0 + 1 + 2)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol 'present')
        (or
          (list
            (symbol '0')
            (symbol '1')
            (symbol '2')))))
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol 'present')
        (func
          (symbol '_list')
          (string '0\x001\x002'))))
    * set:
    <filteredset
      <spanset- 0:3>,
      <baseset [0, 1, 2]>>
    2
    1
    0"""

#  'reverse()' should take effect only if it is the outermost expression:

sh % "try --optimize '0:2 & reverse(all())'" == r"""
    (and
      (range
        (symbol '0')
        (symbol '2'))
      (func
        (symbol 'reverse')
        (func
          (symbol 'all')
          None)))
    * optimized:
    (and
      (range
        (symbol '0')
        (symbol '2'))
      (func
        (symbol 'reverse')
        (func
          (symbol 'all')
          None)))
    * set:
    <filteredset
      <spanset+ 0:3>,
      <spanset+ 0:10>>
    0
    1
    2"""

#  'sort()' should take effect only if it is the outermost expression:

sh % "try --optimize '0:2 & sort(all(), -rev)'" == r"""
    (and
      (range
        (symbol '0')
        (symbol '2'))
      (func
        (symbol 'sort')
        (list
          (func
            (symbol 'all')
            None)
          (negate
            (symbol 'rev')))))
    * optimized:
    (and
      (range
        (symbol '0')
        (symbol '2'))
      (func
        (symbol 'sort')
        (list
          (func
            (symbol 'all')
            None)
          (string '-rev'))))
    * set:
    <filteredset
      <spanset+ 0:3>,
      <spanset+ 0:10>>
    0
    1
    2"""

#  invalid argument passed to noop sort():

sh % "log '0:2 & sort()'" == r"""
    hg: parse error: sort requires one or two arguments
    [255]"""
sh % "log '0:2 & sort(all(), -invalid)'" == r"""
    hg: parse error: unknown sort key '-invalid'
    [255]"""

#  for 'A & f(B)', 'B' should not be affected by the order of 'A':

sh % "try --optimize '2:0 & first(1 + 0 + 2)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol 'first')
        (or
          (list
            (symbol '1')
            (symbol '0')
            (symbol '2')))))
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol 'first')
        (func
          (symbol '_list')
          (string '1\x000\x002'))))
    * set:
    <filteredset
      <baseset [1]>,
      <spanset- 0:3>>
    1"""

sh % "try --optimize '2:0 & not last(0 + 2 + 1)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (not
        (func
          (symbol 'last')
          (or
            (list
              (symbol '0')
              (symbol '2')
              (symbol '1'))))))
    * optimized:
    (difference
      (range
        (symbol '2')
        (symbol '0'))
      (func
        (symbol 'last')
        (func
          (symbol '_list')
          (string '0\x002\x001'))))
    * set:
    <filteredset
      <spanset- 0:3>,
      <not
        <baseset [1]>>>
    2
    0"""

#  for 'A & (op)(B)', 'B' should not be affected by the order of 'A':

sh % "try --optimize '2:0 & (1 + 0 + 2):(0 + 2 + 1)'" == r"""
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (range
        (group
          (or
            (list
              (symbol '1')
              (symbol '0')
              (symbol '2'))))
        (group
          (or
            (list
              (symbol '0')
              (symbol '2')
              (symbol '1'))))))
    * optimized:
    (and
      (range
        (symbol '2')
        (symbol '0'))
      (range
        (func
          (symbol '_list')
          (string '1\x000\x002'))
        (func
          (symbol '_list')
          (string '0\x002\x001'))))
    * set:
    <baseset [1]>
    1"""

#  'A & B' can be rewritten as 'flipand(B, A)' by weight.

sh % "try --optimize 'contains(\"glob:*\") & (2 + 0 + 1)'" == r"""
    (and
      (func
        (symbol 'contains')
        (string 'glob:*'))
      (group
        (or
          (list
            (symbol '2')
            (symbol '0')
            (symbol '1')))))
    * optimized:
    (andsmally
      (func
        (symbol 'contains')
        (string 'glob:*'))
      (func
        (symbol '_list')
        (string '2\x000\x001')))
    * set:
    <filteredset
      <baseset+ [0, 1, 2]>,
      <contains 'glob:*'>>
    0
    1
    2"""

#  and in this example, 'A & B' is rewritten as 'B & A', but 'A' overrides
#  the order appropriately:

sh % "try --optimize 'reverse(contains(\"glob:*\")) & (0 + 2 + 1)'" == r"""
    (and
      (func
        (symbol 'reverse')
        (func
          (symbol 'contains')
          (string 'glob:*')))
      (group
        (or
          (list
            (symbol '0')
            (symbol '2')
            (symbol '1')))))
    * optimized:
    (andsmally
      (func
        (symbol 'reverse')
        (func
          (symbol 'contains')
          (string 'glob:*')))
      (func
        (symbol '_list')
        (string '0\x002\x001')))
    * set:
    <filteredset
      <baseset- [0, 1, 2]>,
      <contains 'glob:*'>>
    2
    1
    0"""

# test sort revset
# --------------------------------------------

# test when adding two unordered revsets

sh % "log 'sort(keyword(issue) or modifies(b))'" == r"""
    4
    6"""

# test when sorting a reversed collection in the same way it is

sh % "log 'sort(reverse(all()), -rev)'" == r"""
    9
    8
    7
    6
    5
    4
    3
    2
    1
    0"""

# test when sorting a reversed collection

sh % "log 'sort(reverse(all()), rev)'" == r"""
    0
    1
    2
    3
    4
    5
    6
    7
    8
    9"""


# test sorting two sorted collections in different orders

sh % "log 'sort(outgoing() or reverse(removes(a)), rev)'" == r"""
    2
    6
    8
    9"""

# test sorting two sorted collections in different orders backwards

sh % "log 'sort(outgoing() or reverse(removes(a)), -rev)'" == r"""
    9
    8
    6
    2"""

# test empty sort key which is noop

sh % "log 'sort(0 + 2 + 1, \"\")'" == r"""
    0
    2
    1"""

# test invalid sort keys

sh % "log 'sort(all(), -invalid)'" == r"""
    hg: parse error: unknown sort key '-invalid'
    [255]"""

sh % "cd .."

# test sorting by multiple keys including variable-length strings

sh % "hg init sorting"
sh % "cd sorting"
sh % "cat" << r"""
[ui]
logtemplate = '{rev} {branch|p5}{desc|p5}{author|p5}{date|hgdate}\n'
[templatealias]
p5(s) = pad(s, 5)
""" >> ".hg/hgrc"
sh % "setbranch b12"
sh % "commit -m m111 -u u112 -d '111 10800'"
sh % "setbranch b11"
sh % "commit -m m12 -u u111 -d '112 7200'"
sh % "setbranch b111"
sh % "commit -m m11 -u u12 -d '111 3600'"
sh % "setbranch b112"
sh % "commit -m m111 -u u11 -d '120 0'"

#  compare revisions (has fast path):

sh % "hg log -r 'sort(all(), rev)'" == r"""
    0 b12  m111 u112 111 10800
    1 b11  m12  u111 112 7200
    2 b111 m11  u12  111 3600
    3 b112 m111 u11  120 0"""

sh % "hg log -r 'sort(all(), -rev)'" == r"""
    3 b112 m111 u11  120 0
    2 b111 m11  u12  111 3600
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800"""

#  compare variable-length strings (issue5218):

sh % "hg log -r 'sort(all(), branch)'" == r"""
    1 b11  m12  u111 112 7200
    2 b111 m11  u12  111 3600
    3 b112 m111 u11  120 0
    0 b12  m111 u112 111 10800"""

sh % "hg log -r 'sort(all(), -branch)'" == r"""
    0 b12  m111 u112 111 10800
    3 b112 m111 u11  120 0
    2 b111 m11  u12  111 3600
    1 b11  m12  u111 112 7200"""

sh % "hg log -r 'sort(all(), desc)'" == r"""
    2 b111 m11  u12  111 3600
    0 b12  m111 u112 111 10800
    3 b112 m111 u11  120 0
    1 b11  m12  u111 112 7200"""

sh % "hg log -r 'sort(all(), -desc)'" == r"""
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800
    3 b112 m111 u11  120 0
    2 b111 m11  u12  111 3600"""

sh % "hg log -r 'sort(all(), user)'" == r"""
    3 b112 m111 u11  120 0
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

sh % "hg log -r 'sort(all(), -user)'" == r"""
    2 b111 m11  u12  111 3600
    0 b12  m111 u112 111 10800
    1 b11  m12  u111 112 7200
    3 b112 m111 u11  120 0"""

#  compare dates (tz offset should have no effect):

sh % "hg log -r 'sort(all(), date)'" == r"""
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600
    1 b11  m12  u111 112 7200
    3 b112 m111 u11  120 0"""

sh % "hg log -r 'sort(all(), -date)'" == r"""
    3 b112 m111 u11  120 0
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

#  be aware that 'sort(x, -k)' is not exactly the same as 'reverse(sort(x, k))'
#  because '-k' reverses the comparison, not the list itself:

sh % "hg log -r 'sort(0 + 2, date)'" == r"""
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

sh % "hg log -r 'sort(0 + 2, -date)'" == r"""
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

sh % "hg log -r 'reverse(sort(0 + 2, date))'" == r"""
    2 b111 m11  u12  111 3600
    0 b12  m111 u112 111 10800"""

#  sort by multiple keys:

sh % "hg log -r 'sort(all(), \"branch -rev\")'" == r"""
    1 b11  m12  u111 112 7200
    2 b111 m11  u12  111 3600
    3 b112 m111 u11  120 0
    0 b12  m111 u112 111 10800"""

sh % "hg log -r 'sort(all(), \"-desc -date\")'" == r"""
    1 b11  m12  u111 112 7200
    3 b112 m111 u11  120 0
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

sh % "hg log -r 'sort(all(), \"user -branch date rev\")'" == r"""
    3 b112 m111 u11  120 0
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800
    2 b111 m11  u12  111 3600"""

#  toposort prioritises graph branches

sh % "hg up 2" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "touch a"
sh % "hg addremove" == "adding a"
sh % "hg ci -m t1 -u tu -d '130 0'"
sh % "echo a" >> "a"
sh % "hg ci -m t2 -u tu -d '130 0'"
sh % "hg book book1"
sh % "hg up 4" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark book1)"""
sh % "touch a"
sh % "hg addremove"
sh % "hg ci -m t3 -u tu -d '130 0'"

sh % "hg log -r 'sort(all(), topo)'" == r"""
    6 b111 t3   tu   130 0
    5 b111 t2   tu   130 0
    4 b111 t1   tu   130 0
    3 b112 m111 u11  120 0
    2 b111 m11  u12  111 3600
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800"""

sh % "hg log -r 'sort(all(), -topo)'" == r"""
    0 b12  m111 u112 111 10800
    1 b11  m12  u111 112 7200
    2 b111 m11  u12  111 3600
    3 b112 m111 u11  120 0
    4 b111 t1   tu   130 0
    5 b111 t2   tu   130 0
    6 b111 t3   tu   130 0"""

sh % "hg log -r 'sort(all(), topo, topo.firstbranch=book1)'" == r"""
    5 b111 t2   tu   130 0
    6 b111 t3   tu   130 0
    4 b111 t1   tu   130 0
    3 b112 m111 u11  120 0
    2 b111 m11  u12  111 3600
    1 b11  m12  u111 112 7200
    0 b12  m111 u112 111 10800"""

# topographical sorting can't be combined with other sort keys, and you can't
# use the topo.firstbranch option when topo sort is not active:

sh % "hg log -r 'sort(all(), \"topo user\")'" == r"""
    hg: parse error: topo sort order cannot be combined with other sort keys
    [255]"""

sh % "hg log -r 'sort(all(), user, topo.firstbranch=book1)'" == r"""
    hg: parse error: topo.firstbranch can only be used when using the topo sort key
    [255]"""

# topo.firstbranch should accept any kind of expressions:

sh % "hg log -r 'sort(0, topo, topo.firstbranch=(book1))'" == "0 b12  m111 u112 111 10800"

sh % "cd .."
sh % "cd repo"

# test multiline revset with errors

sh % "hg log -r '\n. +\n.^ +'" == r"""
    hg: parse error at 9: not a prefix: end
    ( . + .^ +
              ^ here)
    [255]"""
