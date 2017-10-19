  $ HGENCODING=utf-8
  $ export HGENCODING
  $ cat > testrevset.py << EOF
  > import mercurial.revset
  > 
  > baseset = mercurial.revset.baseset
  > 
  > def r3232(repo, subset, x):
  >     """"simple revset that return [3,2,3,2]
  > 
  >     revisions duplicated on purpose.
  >     """
  >     if 3 not in subset:
  >        if 2 in subset:
  >            return baseset([2,2])
  >        return baseset()
  >     return baseset([3,3,2,2])
  > 
  > mercurial.revset.symbols['r3232'] = r3232
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > drawdag=$TESTDIR/drawdag.py
  > testrevset=$TESTTMP/testrevset.py
  > EOF

  $ try() {
  >   hg debugrevspec --debug "$@"
  > }

  $ log() {
  >   hg log --template '{rev}\n' -r "$1"
  > }

extension to build '_intlist()' and '_hexlist()', which is necessary because
these predicates use '\0' as a separator:

  $ cat <<EOF > debugrevlistspec.py
  > from __future__ import absolute_import
  > from mercurial import (
  >     node as nodemod,
  >     registrar,
  >     revset,
  >     revsetlang,
  >     smartset,
  > )
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'debugrevlistspec',
  >     [('', 'optimize', None, 'print parsed tree after optimizing'),
  >      ('', 'bin', None, 'unhexlify arguments')])
  > def debugrevlistspec(ui, repo, fmt, *args, **opts):
  >     if opts['bin']:
  >         args = map(nodemod.bin, args)
  >     expr = revsetlang.formatspec(fmt, list(args))
  >     if ui.verbose:
  >         tree = revsetlang.parse(expr, lookup=repo.__contains__)
  >         ui.note(revsetlang.prettyformat(tree), "\n")
  >         if opts["optimize"]:
  >             opttree = revsetlang.optimize(revsetlang.analyze(tree))
  >             ui.note("* optimized:\n", revsetlang.prettyformat(opttree),
  >                     "\n")
  >     func = revset.match(ui, expr, repo)
  >     revs = func(repo)
  >     if ui.verbose:
  >         ui.note("* set:\n", smartset.prettyformat(revs), "\n")
  >     for c in revs:
  >         ui.write("%s\n" % c)
  > EOF
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > debugrevlistspec = $TESTTMP/debugrevlistspec.py
  > EOF
  $ trylist() {
  >   hg debugrevlistspec --debug "$@"
  > }

  $ hg init repo
  $ cd repo

  $ echo a > a
  $ hg branch a
  marked working directory as branch a
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm0

  $ echo b > b
  $ hg branch b
  marked working directory as branch b
  $ hg ci -Aqm1

  $ rm a
  $ hg branch a-b-c-
  marked working directory as branch a-b-c-
  $ hg ci -Aqm2 -u Bob

  $ hg log -r "extra('branch', 'a-b-c-')" --template '{rev}\n'
  2
  $ hg log -r "extra('branch')" --template '{rev}\n'
  0
  1
  2
  $ hg log -r "extra('branch', 're:a')" --template '{rev} {branch}\n'
  0 a
  2 a-b-c-

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch +a+b+c+
  marked working directory as branch +a+b+c+
  $ hg ci -Aqm3

  $ hg co 2  # interleave
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bb > b
  $ hg branch -- -a-b-c-
  marked working directory as branch -a-b-c-
  $ hg ci -Aqm4 -d "May 12 2005"

  $ hg co 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch !a/b/c/
  marked working directory as branch !a/b/c/
  $ hg ci -Aqm"5 bug"

  $ hg merge 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg branch _a_b_c_
  marked working directory as branch _a_b_c_
  $ hg ci -Aqm"6 issue619"

  $ hg branch .a.b.c.
  marked working directory as branch .a.b.c.
  $ hg ci -Aqm7

  $ hg branch all
  marked working directory as branch all

  $ hg co 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch é
  marked working directory as branch \xc3\xa9 (esc)
  $ hg ci -Aqm9

  $ hg tag -r6 1.0
  $ hg bookmark -r6 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

  $ hg clone --quiet -U -r 7 . ../remote1
  $ hg clone --quiet -U -r 8 . ../remote2
  $ echo "[paths]" >> .hg/hgrc
  $ echo "default = ../remote1" >> .hg/hgrc

trivial

  $ try 0:1
  (range
    (symbol '0')
    (symbol '1'))
  * set:
  <spanset+ 0:2>
  0
  1
  $ try --optimize :
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
  9
  $ try 3::6
  (dagrange
    (symbol '3')
    (symbol '6'))
  * set:
  <baseset+ [3, 5, 6]>
  3
  5
  6
  $ try '0|1|2'
  (or
    (list
      (symbol '0')
      (symbol '1')
      (symbol '2')))
  * set:
  <baseset [0, 1, 2]>
  0
  1
  2

names that should work without quoting

  $ try a
  (symbol 'a')
  * set:
  <baseset [0]>
  0
  $ try b-a
  (minus
    (symbol 'b')
    (symbol 'a'))
  * set:
  <filteredset
    <baseset [1]>,
    <not
      <baseset [0]>>>
  1
  $ try _a_b_c_
  (symbol '_a_b_c_')
  * set:
  <baseset [6]>
  6
  $ try _a_b_c_-a
  (minus
    (symbol '_a_b_c_')
    (symbol 'a'))
  * set:
  <filteredset
    <baseset [6]>,
    <not
      <baseset [0]>>>
  6
  $ try .a.b.c.
  (symbol '.a.b.c.')
  * set:
  <baseset [7]>
  7
  $ try .a.b.c.-a
  (minus
    (symbol '.a.b.c.')
    (symbol 'a'))
  * set:
  <filteredset
    <baseset [7]>,
    <not
      <baseset [0]>>>
  7

names that should be caught by fallback mechanism

  $ try -- '-a-b-c-'
  (symbol '-a-b-c-')
  * set:
  <baseset [4]>
  4
  $ log -a-b-c-
  4
  $ try '+a+b+c+'
  (symbol '+a+b+c+')
  * set:
  <baseset [3]>
  3
  $ try '+a+b+c+:'
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
  9
  $ try ':+a+b+c+'
  (rangepre
    (symbol '+a+b+c+'))
  * set:
  <spanset+ 0:4>
  0
  1
  2
  3
  $ try -- '-a-b-c-:+a+b+c+'
  (range
    (symbol '-a-b-c-')
    (symbol '+a+b+c+'))
  * set:
  <spanset- 3:5>
  4
  3
  $ log '-a-b-c-:+a+b+c+'
  4
  3

  $ try -- -a-b-c--a # complains
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
  [255]
  $ try é
  (symbol '\xc3\xa9')
  * set:
  <baseset [9]>
  9

no quoting needed

  $ log ::a-b-c-
  0
  1
  2

quoting needed

  $ try '"-a-b-c-"-a'
  (minus
    (string '-a-b-c-')
    (symbol 'a'))
  * set:
  <filteredset
    <baseset [4]>,
    <not
      <baseset [0]>>>
  4

  $ log '1 or 2'
  1
  2
  $ log '1|2'
  1
  2
  $ log '1 and 2'
  $ log '1&2'
  $ try '1&2|3' # precedence - and is higher
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
  3
  $ try '1|2&3'
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
  1
  $ try '1&2&3' # associativity
  (and
    (and
      (symbol '1')
      (symbol '2'))
    (symbol '3'))
  * set:
  <baseset []>
  $ try '1|(2|3)'
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
  3
  $ log '1.0' # tag
  6
  $ log 'a' # branch
  0
  $ log '2785f51ee'
  0
  $ log 'date(2005)'
  4
  $ log 'date(this is a test)'
  hg: parse error at 10: unexpected token: symbol
  [255]
  $ log 'date()'
  hg: parse error: date requires a string
  [255]
  $ log 'date'
  abort: unknown revision 'date'!
  [255]
  $ log 'date('
  hg: parse error at 5: not a prefix: end
  [255]
  $ log 'date("\xy")'
  hg: parse error: invalid \x escape
  [255]
  $ log 'date(tip)'
  hg: parse error: invalid date: 'tip'
  [255]
  $ log '0:date'
  abort: unknown revision 'date'!
  [255]
  $ log '::"date"'
  abort: unknown revision 'date'!
  [255]
  $ hg book date -r 4
  $ log '0:date'
  0
  1
  2
  3
  4
  $ log '::date'
  0
  1
  2
  4
  $ log '::"date"'
  0
  1
  2
  4
  $ log 'date(2005) and 1::'
  4
  $ hg book -d date

function name should be a symbol

  $ log '"date"(2005)'
  hg: parse error: not a symbol
  [255]

keyword arguments

  $ log 'extra(branch, value=a)'
  0

  $ log 'extra(branch, a, b)'
  hg: parse error: extra takes at most 2 positional arguments
  [255]
  $ log 'extra(a, label=b)'
  hg: parse error: extra got multiple values for keyword argument 'label'
  [255]
  $ log 'extra(label=branch, default)'
  hg: parse error: extra got an invalid argument
  [255]
  $ log 'extra(branch, foo+bar=baz)'
  hg: parse error: extra got an invalid argument
  [255]
  $ log 'extra(unknown=branch)'
  hg: parse error: extra got an unexpected keyword argument 'unknown'
  [255]

  $ try 'foo=bar|baz'
  (keyvalue
    (symbol 'foo')
    (or
      (list
        (symbol 'bar')
        (symbol 'baz'))))
  hg: parse error: can't use a key-value pair in this context
  [255]

 right-hand side should be optimized recursively

  $ try --optimize 'foo=(not public())'
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
  [255]

relation-subscript operator has the highest binding strength (as function call):

  $ hg debugrevspec -p parsed 'tip:tip^#generations[-1]'
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
  4

  $ hg debugrevspec -p parsed --no-show-revs 'not public()#generations[0]'
  * parsed:
  (not
    (relsubscript
      (func
        (symbol 'public')
        None)
      (symbol 'generations')
      (symbol '0')))

left-hand side of relation-subscript operator should be optimized recursively:

  $ hg debugrevspec -p analyzed -p optimized --no-show-revs \
  > '(not public())#generations[0]'
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
    (symbol '0'))

resolution of subscript and relation-subscript ternary operators:

  $ hg debugrevspec -p analyzed 'tip[0]'
  * analyzed:
  (subscript
    (symbol 'tip')
    (symbol '0'))
  hg: parse error: can't use a subscript in this context
  [255]

  $ hg debugrevspec -p analyzed 'tip#rel[0]'
  * analyzed:
  (relsubscript
    (symbol 'tip')
    (symbol 'rel')
    (symbol '0'))
  hg: parse error: unknown identifier: rel
  [255]

  $ hg debugrevspec -p analyzed '(tip#rel)[0]'
  * analyzed:
  (subscript
    (relation
      (symbol 'tip')
      (symbol 'rel'))
    (symbol '0'))
  hg: parse error: can't use a subscript in this context
  [255]

  $ hg debugrevspec -p analyzed 'tip#rel[0][1]'
  * analyzed:
  (subscript
    (relsubscript
      (symbol 'tip')
      (symbol 'rel')
      (symbol '0'))
    (symbol '1'))
  hg: parse error: can't use a subscript in this context
  [255]

  $ hg debugrevspec -p analyzed 'tip#rel0#rel1[1]'
  * analyzed:
  (relsubscript
    (relation
      (symbol 'tip')
      (symbol 'rel0'))
    (symbol 'rel1')
    (symbol '1'))
  hg: parse error: unknown identifier: rel1
  [255]

  $ hg debugrevspec -p analyzed 'tip#rel0[0]#rel1[1]'
  * analyzed:
  (relsubscript
    (relsubscript
      (symbol 'tip')
      (symbol 'rel0')
      (symbol '0'))
    (symbol 'rel1')
    (symbol '1'))
  hg: parse error: unknown identifier: rel1
  [255]

parse errors of relation, subscript and relation-subscript operators:

  $ hg debugrevspec '[0]'
  hg: parse error at 0: not a prefix: [
  [255]
  $ hg debugrevspec '.#'
  hg: parse error at 2: not a prefix: end
  [255]
  $ hg debugrevspec '#rel'
  hg: parse error at 0: not a prefix: #
  [255]
  $ hg debugrevspec '.#rel[0'
  hg: parse error at 7: unexpected token: end
  [255]
  $ hg debugrevspec '.]'
  hg: parse error at 1: invalid token
  [255]

  $ hg debugrevspec '.#generations[a]'
  hg: parse error: relation subscript must be an integer
  [255]
  $ hg debugrevspec '.#generations[1-2]'
  hg: parse error: relation subscript must be an integer
  [255]

parsed tree at stages:

  $ hg debugrevspec -p all '()'
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
  [255]

  $ hg debugrevspec --no-optimized -p all '()'
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
  [255]

  $ hg debugrevspec -p parsed -p analyzed -p optimized '(0|1)-1'
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
  0

  $ hg debugrevspec -p unknown '0'
  abort: invalid stage name: unknown
  [255]

  $ hg debugrevspec -p all --optimize '0'
  abort: cannot use --optimize with --show-stage
  [255]

verify optimized tree:

  $ hg debugrevspec --verify '0|1'

  $ hg debugrevspec --verify -v -p analyzed -p optimized 'r3232() & 2'
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
  [1]

  $ hg debugrevspec --no-optimized --verify-optimized '0'
  abort: cannot use --verify-optimized with --no-optimized
  [255]

Test that symbols only get parsed as functions if there's an opening
parenthesis.

  $ hg book only -r 9
  $ log 'only(only)'   # Outer "only" is a function, inner "only" is the bookmark
  8
  9

':y' behaves like '0:y', but can't be rewritten as such since the revision '0'
may be hidden (issue5385)

  $ try -p parsed -p analyzed ':'
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
  9
  $ try -p analyzed ':1'
  * analyzed:
  (rangepre
    (symbol '1'))
  * set:
  <spanset+ 0:2>
  0
  1
  $ try -p analyzed ':(1|2)'
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
  2
  $ try -p analyzed ':(1&2)'
  * analyzed:
  (rangepre
    (and
      (symbol '1')
      (symbol '2')))
  * set:
  <baseset []>

infix/suffix resolution of ^ operator (issue2884):

 x^:y means (x^):y

  $ try '1^:2'
  (range
    (parentpost
      (symbol '1'))
    (symbol '2'))
  * set:
  <spanset+ 0:3>
  0
  1
  2

  $ try '1^::2'
  (dagrange
    (parentpost
      (symbol '1'))
    (symbol '2'))
  * set:
  <baseset+ [0, 1, 2]>
  0
  1
  2

  $ try '9^:'
  (rangepost
    (parentpost
      (symbol '9')))
  * set:
  <spanset+ 8:10>
  8
  9

 x^:y should be resolved before omitting group operators

  $ try '1^(:2)'
  (parent
    (symbol '1')
    (group
      (rangepre
        (symbol '2'))))
  hg: parse error: ^ expects a number 0, 1, or 2
  [255]

 x^:y should be resolved recursively

  $ try 'sort(1^:2)'
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
  2

  $ try '(3^:4)^:2'
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
  2

  $ try '(3^::4)^::2'
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
  2

  $ try '(9^:)^:'
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
  9

 x^ in alias should also be resolved

  $ try 'A' --config 'revsetalias.A=1^:2'
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
  2

  $ try 'A:2' --config 'revsetalias.A=1^'
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
  2

 but not beyond the boundary of alias expansion, because the resolution should
 be made at the parsing stage

  $ try '1^A' --config 'revsetalias.A=:2'
  (parent
    (symbol '1')
    (symbol 'A'))
  * expanded:
  (parent
    (symbol '1')
    (rangepre
      (symbol '2')))
  hg: parse error: ^ expects a number 0, 1, or 2
  [255]

ancestor can accept 0 or more arguments

  $ log 'ancestor()'
  $ log 'ancestor(1)'
  1
  $ log 'ancestor(4,5)'
  1
  $ log 'ancestor(4,5) and 4'
  $ log 'ancestor(0,0,1,3)'
  0
  $ log 'ancestor(3,1,5,3,5,1)'
  1
  $ log 'ancestor(0,1,3,5)'
  0
  $ log 'ancestor(1,2,3,4,5)'
  1

test ancestors

  $ hg log -G -T '{rev}\n' --config experimental.graphshorten=True
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
  o  0

  $ log 'ancestors(5)'
  0
  1
  3
  5
  $ log 'ancestor(ancestors(5))'
  0
  $ log '::r3232()'
  0
  1
  2
  3

test ancestors with depth limit

 (depth=0 selects the node itself)

  $ log 'reverse(ancestors(9, depth=0))'
  9

 (interleaved: '4' would be missing if heap queue were higher depth first)

  $ log 'reverse(ancestors(8:9, depth=1))'
  9
  8
  4

 (interleaved: '2' would be missing if heap queue were higher depth first)

  $ log 'reverse(ancestors(7+8, depth=2))'
  8
  7
  6
  5
  4
  2

 (walk example above by separate queries)

  $ log 'reverse(ancestors(8, depth=2)) + reverse(ancestors(7, depth=2))'
  8
  4
  2
  7
  6
  5

 (walk 2nd and 3rd ancestors)

  $ log 'reverse(ancestors(7, depth=3, startdepth=2))'
  5
  4
  3
  2

 (interleaved: '4' would be missing if higher-depth ancestors weren't scanned)

  $ log 'reverse(ancestors(7+8, depth=2, startdepth=2))'
  5
  4
  2

 (note that 'ancestors(x, depth=y, startdepth=z)' does not identical to
 'ancestors(x, depth=y) - ancestors(x, depth=z-1)' because a node may have
 multiple depths)

  $ log 'reverse(ancestors(7+8, depth=2) - ancestors(7+8, depth=1))'
  5
  2

test bad arguments passed to ancestors()

  $ log 'ancestors(., depth=-1)'
  hg: parse error: negative depth
  [255]
  $ log 'ancestors(., depth=foo)'
  hg: parse error: ancestors expects an integer depth
  [255]

test descendants

  $ hg log -G -T '{rev}\n' --config experimental.graphshorten=True
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
  o  0

 (null is ultimate root and has optimized path)

  $ log 'null:4 & descendants(null)'
  -1
  0
  1
  2
  3
  4

 (including merge)

  $ log ':8 & descendants(2)'
  2
  4
  6
  7
  8

 (multiple roots)

  $ log ':8 & descendants(2+5)'
  2
  4
  5
  6
  7
  8

test descendants with depth limit

 (depth=0 selects the node itself)

  $ log 'descendants(0, depth=0)'
  0
  $ log 'null: & descendants(null, depth=0)'
  -1

 (p2 = null should be ignored)

  $ log 'null: & descendants(null, depth=2)'
  -1
  0
  1

 (multiple paths: depth(6) = (2, 3))

  $ log 'descendants(1+3, depth=2)'
  1
  2
  3
  4
  5
  6

 (multiple paths: depth(5) = (1, 2), depth(6) = (2, 3))

  $ log 'descendants(3+1, depth=2, startdepth=2)'
  4
  5
  6

 (multiple depths: depth(6) = (0, 2, 4), search for depth=2)

  $ log 'descendants(0+3+6, depth=3, startdepth=1)'
  1
  2
  3
  4
  5
  6
  7

 (multiple depths: depth(6) = (0, 4), no match)

  $ log 'descendants(0+6, depth=3, startdepth=1)'
  1
  2
  3
  4
  5
  7

test ancestors/descendants relation subscript:

  $ log 'tip#generations[0]'
  9
  $ log '.#generations[-1]'
  8
  $ log '.#g[(-1)]'
  8

  $ hg debugrevspec -p parsed 'roots(:)#g[2]'
  * parsed:
  (relsubscript
    (func
      (symbol 'roots')
      (rangeall
        None))
    (symbol 'g')
    (symbol '2'))
  2
  3

test author

  $ log 'author(bob)'
  2
  $ log 'author("re:bob|test")'
  0
  1
  2
  3
  4
  5
  6
  7
  8
  9
  $ log 'author(r"re:\S")'
  0
  1
  2
  3
  4
  5
  6
  7
  8
  9
  $ log 'branch(é)'
  8
  9
  $ log 'branch(a)'
  0
  $ hg log -r 'branch("re:a")' --template '{rev} {branch}\n'
  0 a
  2 a-b-c-
  3 +a+b+c+
  4 -a-b-c-
  5 !a/b/c/
  6 _a_b_c_
  7 .a.b.c.
  $ log 'children(ancestor(4,5))'
  2
  3

  $ log 'children(4)'
  6
  8
  $ log 'children(null)'
  0

  $ log 'closed()'
  $ log 'contains(a)'
  0
  1
  3
  5
  $ log 'contains("../repo/a")'
  0
  1
  3
  5
  $ log 'desc(B)'
  5
  $ hg log -r 'desc(r"re:S?u")' --template "{rev} {desc|firstline}\n"
  5 5 bug
  6 6 issue619
  $ log 'descendants(2 or 3)'
  2
  3
  4
  5
  6
  7
  8
  9
  $ log 'file("b*")'
  1
  4
  $ log 'filelog("b")'
  1
  4
  $ log 'filelog("../repo/b")'
  1
  4
  $ log 'follow()'
  0
  1
  2
  4
  8
  9
  $ log 'grep("issue\d+")'
  6
  $ try 'grep("(")' # invalid regular expression
  (func
    (symbol 'grep')
    (string '('))
  hg: parse error: invalid match pattern: unbalanced parenthesis
  [255]
  $ try 'grep("\bissue\d+")'
  (func
    (symbol 'grep')
    (string '\x08issue\\d+'))
  * set:
  <filteredset
    <fullreposet+ 0:10>,
    <grep '\x08issue\\d+'>>
  $ try 'grep(r"\bissue\d+")'
  (func
    (symbol 'grep')
    (string '\\bissue\\d+'))
  * set:
  <filteredset
    <fullreposet+ 0:10>,
    <grep '\\bissue\\d+'>>
  6
  $ try 'grep(r"\")'
  hg: parse error at 7: unterminated string
  [255]
  $ log 'head()'
  0
  1
  2
  3
  4
  5
  6
  7
  9
  $ log 'heads(6::)'
  7
  $ log 'keyword(issue)'
  6
  $ log 'keyword("test a")'

Test first (=limit) and last

  $ log 'limit(head(), 1)'
  0
  $ log 'limit(author("re:bob|test"), 3, 5)'
  5
  6
  7
  $ log 'limit(author("re:bob|test"), offset=6)'
  6
  $ log 'limit(author("re:bob|test"), offset=10)'
  $ log 'limit(all(), 1, -1)'
  hg: parse error: negative offset
  [255]
  $ log 'limit(all(), -1)'
  hg: parse error: negative number to select
  [255]
  $ log 'limit(all(), 0)'

  $ log 'last(all(), -1)'
  hg: parse error: negative number to select
  [255]
  $ log 'last(all(), 0)'
  $ log 'last(all(), 1)'
  9
  $ log 'last(all(), 2)'
  8
  9

Test smartset.slice() by first/last()

 (using unoptimized set, filteredset as example)

  $ hg debugrevspec --no-show-revs -s '0:7 & branch("re:")'
  * set:
  <filteredset
    <spanset+ 0:8>,
    <branch 're:'>>
  $ log 'limit(0:7 & branch("re:"), 3, 4)'
  4
  5
  6
  $ log 'limit(7:0 & branch("re:"), 3, 4)'
  3
  2
  1
  $ log 'last(0:7 & branch("re:"), 2)'
  6
  7

 (using baseset)

  $ hg debugrevspec --no-show-revs -s 0+1+2+3+4+5+6+7
  * set:
  <baseset [0, 1, 2, 3, 4, 5, 6, 7]>
  $ hg debugrevspec --no-show-revs -s 0::7
  * set:
  <baseset+ [0, 1, 2, 3, 4, 5, 6, 7]>
  $ log 'limit(0+1+2+3+4+5+6+7, 3, 4)'
  4
  5
  6
  $ log 'limit(sort(0::7, rev), 3, 4)'
  4
  5
  6
  $ log 'limit(sort(0::7, -rev), 3, 4)'
  3
  2
  1
  $ log 'last(sort(0::7, rev), 2)'
  6
  7
  $ hg debugrevspec -s 'limit(sort(0::7, rev), 3, 6)'
  * set:
  <baseset+ [6, 7]>
  6
  7
  $ hg debugrevspec -s 'limit(sort(0::7, rev), 3, 9)'
  * set:
  <baseset+ []>
  $ hg debugrevspec -s 'limit(sort(0::7, -rev), 3, 6)'
  * set:
  <baseset- [0, 1]>
  1
  0
  $ hg debugrevspec -s 'limit(sort(0::7, -rev), 3, 9)'
  * set:
  <baseset- []>
  $ hg debugrevspec -s 'limit(0::7, 0)'
  * set:
  <baseset+ []>

 (using spanset)

  $ hg debugrevspec --no-show-revs -s 0:7
  * set:
  <spanset+ 0:8>
  $ log 'limit(0:7, 3, 4)'
  4
  5
  6
  $ log 'limit(7:0, 3, 4)'
  3
  2
  1
  $ log 'limit(0:7, 3, 6)'
  6
  7
  $ log 'limit(7:0, 3, 6)'
  1
  0
  $ log 'last(0:7, 2)'
  6
  7
  $ hg debugrevspec -s 'limit(0:7, 3, 6)'
  * set:
  <spanset+ 6:8>
  6
  7
  $ hg debugrevspec -s 'limit(0:7, 3, 9)'
  * set:
  <spanset+ 8:8>
  $ hg debugrevspec -s 'limit(7:0, 3, 6)'
  * set:
  <spanset- 0:2>
  1
  0
  $ hg debugrevspec -s 'limit(7:0, 3, 9)'
  * set:
  <spanset- 0:0>
  $ hg debugrevspec -s 'limit(0:7, 0)'
  * set:
  <spanset+ 0:0>

Test order of first/last revisions

  $ hg debugrevspec -s 'first(4:0, 3) & 3:'
  * set:
  <filteredset
    <spanset- 2:5>,
    <spanset+ 3:10>>
  4
  3

  $ hg debugrevspec -s '3: & first(4:0, 3)'
  * set:
  <filteredset
    <spanset+ 3:10>,
    <spanset- 2:5>>
  3
  4

  $ hg debugrevspec -s 'last(4:0, 3) & :1'
  * set:
  <filteredset
    <spanset- 0:3>,
    <spanset+ 0:2>>
  1
  0

  $ hg debugrevspec -s ':1 & last(4:0, 3)'
  * set:
  <filteredset
    <spanset+ 0:2>,
    <spanset+ 0:3>>
  0
  1

Test scmutil.revsingle() should return the last revision

  $ hg debugrevspec -s 'last(0::)'
  * set:
  <baseset slice=0:1
    <generatorset->>
  9
  $ hg identify -r '0::' --num
  9

Test matching

  $ log 'matching(6)'
  6
  $ log 'matching(6:7, "phase parents user date branch summary files description substate")'
  6
  7

Testing min and max

max: simple

  $ log 'max(contains(a))'
  5

max: simple on unordered set)

  $ log 'max((4+0+2+5+7) and contains(a))'
  5

max: no result

  $ log 'max(contains(stringthatdoesnotappearanywhere))'

max: no result on unordered set

  $ log 'max((4+0+2+5+7) and contains(stringthatdoesnotappearanywhere))'

min: simple

  $ log 'min(contains(a))'
  0

min: simple on unordered set

  $ log 'min((4+0+2+5+7) and contains(a))'
  0

min: empty

  $ log 'min(contains(stringthatdoesnotappearanywhere))'

min: empty on unordered set

  $ log 'min((4+0+2+5+7) and contains(stringthatdoesnotappearanywhere))'


  $ log 'merge()'
  6
  $ log 'branchpoint()'
  1
  4
  $ log 'modifies(b)'
  4
  $ log 'modifies("path:b")'
  4
  $ log 'modifies("*")'
  4
  6
  $ log 'modifies("set:modified()")'
  4
  $ log 'id(5)'
  2
  $ log 'only(9)'
  8
  9
  $ log 'only(8)'
  8
  $ log 'only(9, 5)'
  2
  4
  8
  9
  $ log 'only(7 + 9, 5 + 2)'
  4
  6
  7
  8
  9

Test empty set input
  $ log 'only(p2())'
  $ log 'only(p1(), p2())'
  0
  1
  2
  4
  8
  9

Test '%' operator

  $ log '9%'
  8
  9
  $ log '9%5'
  2
  4
  8
  9
  $ log '(7 + 9)%(5 + 2)'
  4
  6
  7
  8
  9

Test operand of '%' is optimized recursively (issue4670)

  $ try --optimize '8:9-8%'
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
  9
  $ try --optimize '(9)%(5)'
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
  9

Test the order of operations

  $ log '7 + 9%5 + 2'
  7
  2
  4
  8
  9

Test explicit numeric revision
  $ log 'rev(-2)'
  $ log 'rev(-1)'
  -1
  $ log 'rev(0)'
  0
  $ log 'rev(9)'
  9
  $ log 'rev(10)'
  $ log 'rev(tip)'
  hg: parse error: rev expects a number
  [255]

Test hexadecimal revision
  $ log 'id(2)'
  abort: 00changelog.i@2: ambiguous identifier!
  [255]
  $ log 'id(23268)'
  4
  $ log 'id(2785f51eece)'
  0
  $ log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532c)'
  8
  $ log 'id(d5d0dcbdc4a)'
  $ log 'id(d5d0dcbdc4w)'
  $ log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532d)'
  $ log 'id(d5d0dcbdc4d9ff5dbb2d336f32f0bb561c1a532q)'
  $ log 'id(1.0)'
  $ log 'id(xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx)'

Test null revision
  $ log '(null)'
  -1
  $ log '(null:0)'
  -1
  0
  $ log '(0:null)'
  0
  -1
  $ log 'null::0'
  -1
  0
  $ log 'null:tip - 0:'
  -1
  $ log 'null: and null::' | head -1
  -1
  $ log 'null: or 0:' | head -2
  -1
  0
  $ log 'ancestors(null)'
  -1
  $ log 'reverse(null:)' | tail -2
  0
  -1
  $ log 'first(null:)'
  -1
  $ log 'min(null:)'
BROKEN: should be '-1'
  $ log 'tip:null and all()' | tail -2
  1
  0

Test working-directory revision
  $ hg debugrevspec 'wdir()'
  2147483647
  $ hg debugrevspec 'wdir()^'
  9
  $ hg up 7
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugrevspec 'wdir()^'
  7
  $ hg debugrevspec 'wdir()^0'
  2147483647
  $ hg debugrevspec 'wdir()~3'
  5
  $ hg debugrevspec 'ancestors(wdir())'
  0
  1
  2
  3
  4
  5
  6
  7
  2147483647
  $ hg debugrevspec 'wdir()~0'
  2147483647
  $ hg debugrevspec 'p1(wdir())'
  7
  $ hg debugrevspec 'p2(wdir())'
  $ hg debugrevspec 'parents(wdir())'
  7
  $ hg debugrevspec 'wdir()^1'
  7
  $ hg debugrevspec 'wdir()^2'
  $ hg debugrevspec 'wdir()^3'
  hg: parse error: ^ expects a number 0, 1, or 2
  [255]
For tests consistency
  $ hg up 9
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugrevspec 'tip or wdir()'
  9
  2147483647
  $ hg debugrevspec '0:tip and wdir()'
  $ log '0:wdir()' | tail -3
  8
  9
  2147483647
  $ log 'wdir():0' | head -3
  2147483647
  9
  8
  $ log 'wdir():wdir()'
  2147483647
  $ log '(all() + wdir()) & min(. + wdir())'
  9
  $ log '(all() + wdir()) & max(. + wdir())'
  2147483647
  $ log 'first(wdir() + .)'
  2147483647
  $ log 'last(. + wdir())'
  2147483647

Test working-directory integer revision and node id
(BUG: '0:wdir()' is still needed to populate wdir revision)

  $ hg debugrevspec '0:wdir() & 2147483647'
  2147483647
  $ hg debugrevspec '0:wdir() & rev(2147483647)'
  2147483647
  $ hg debugrevspec '0:wdir() & ffffffffffffffffffffffffffffffffffffffff'
  2147483647
  $ hg debugrevspec '0:wdir() & ffffffffffff'
  2147483647
  $ hg debugrevspec '0:wdir() & id(ffffffffffffffffffffffffffffffffffffffff)'
  2147483647
  $ hg debugrevspec '0:wdir() & id(ffffffffffff)'
  2147483647

  $ cd ..

Test short 'ff...' hash collision
(BUG: '0:wdir()' is still needed to populate wdir revision)

  $ hg init wdir-hashcollision
  $ cd wdir-hashcollision
  $ cat <<EOF >> .hg/hgrc
  > [experimental]
  > evolution.createmarkers=True
  > EOF
  $ echo 0 > a
  $ hg ci -qAm 0
  $ for i in 2463 2961 6726 78127; do
  >   hg up -q 0
  >   echo $i > a
  >   hg ci -qm $i
  > done
  $ hg up -q null
  $ hg log -r '0:wdir()' -T '{rev}:{node} {shortest(node, 3)}\n'
  0:b4e73ffab476aa0ee32ed81ca51e07169844bc6a b4e
  1:fffbae3886c8fbb2114296380d276fd37715d571 fffba
  2:fffb6093b00943f91034b9bdad069402c834e572 fffb6
  3:fff48a9b9de34a4d64120c29548214c67980ade3 fff4
  4:ffff85cff0ff78504fcdc3c0bc10de0c65379249 ffff8
  2147483647:ffffffffffffffffffffffffffffffffffffffff fffff
  $ hg debugobsolete fffbae3886c8fbb2114296380d276fd37715d571
  obsoleted 1 changesets

  $ hg debugrevspec '0:wdir() & fff'
  abort: 00changelog.i@fff: ambiguous identifier!
  [255]
  $ hg debugrevspec '0:wdir() & ffff'
  abort: 00changelog.i@ffff: ambiguous identifier!
  [255]
  $ hg debugrevspec '0:wdir() & fffb'
  abort: 00changelog.i@fffb: ambiguous identifier!
  [255]
BROKEN should be '2' (node lookup uses unfiltered repo since dc25ed84bee8)
  $ hg debugrevspec '0:wdir() & id(fffb)'
  2
  $ hg debugrevspec '0:wdir() & ffff8'
  4
  $ hg debugrevspec '0:wdir() & fffff'
  2147483647

  $ cd ..

Test branch() with wdir()

  $ cd repo

  $ log '0:wdir() & branch("literal:é")'
  8
  9
  2147483647
  $ log '0:wdir() & branch("re:é")'
  8
  9
  2147483647
  $ log '0:wdir() & branch("re:^a")'
  0
  2
  $ log '0:wdir() & branch(8)'
  8
  9
  2147483647

branch(wdir()) returns all revisions belonging to the working branch. The wdir
itself isn't returned unless it is explicitly populated.

  $ log 'branch(wdir())'
  8
  9
  $ log '0:wdir() & branch(wdir())'
  8
  9
  2147483647

  $ log 'outgoing()'
  8
  9
  $ log 'outgoing("../remote1")'
  8
  9
  $ log 'outgoing("../remote2")'
  3
  5
  6
  7
  9
  $ log 'p1(merge())'
  5
  $ log 'p2(merge())'
  4
  $ log 'parents(merge())'
  4
  5
  $ log 'p1(branchpoint())'
  0
  2
  $ log 'p2(branchpoint())'
  $ log 'parents(branchpoint())'
  0
  2
  $ log 'removes(a)'
  2
  6
  $ log 'roots(all())'
  0
  $ log 'reverse(2 or 3 or 4 or 5)'
  5
  4
  3
  2
  $ log 'reverse(all())'
  9
  8
  7
  6
  5
  4
  3
  2
  1
  0
  $ log 'reverse(all()) & filelog(b)'
  4
  1
  $ log 'rev(5)'
  5
  $ log 'sort(limit(reverse(all()), 3))'
  7
  8
  9
  $ log 'sort(2 or 3 or 4 or 5, date)'
  2
  3
  5
  4
  $ log 'tagged()'
  6
  $ log 'tag()'
  6
  $ log 'tag(1.0)'
  6
  $ log 'tag(tip)'
  9

Test order of revisions in compound expression
----------------------------------------------

The general rule is that only the outermost (= leftmost) predicate can
enforce its ordering requirement. The other predicates should take the
ordering defined by it.

 'A & B' should follow the order of 'A':

  $ log '2:0 & 0::2'
  2
  1
  0

 'head()' combines sets in right order:

  $ log '2:0 & head()'
  2
  1
  0

 'x:y' takes ordering parameter into account:

  $ try -p optimized '3:0 & 0:3 & not 2:1'
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
  0

 'a + b', which is optimized to '_list(a b)', should take the ordering of
 the left expression:

  $ try --optimize '2:0 & (0 + 1 + 2)'
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
  0

 'A + B' should take the ordering of the left expression:

  $ try --optimize '2:0 & (0:1 + 2)'
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
  0

 '_intlist(a b)' should behave like 'a + b':

  $ trylist --optimize '2:0 & %ld' 0 1 2
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
  0

  $ trylist --optimize '%ld & 2:0' 0 2 1
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
  1

 '_hexlist(a b)' should behave like 'a + b':

  $ trylist --optimize --bin '2:0 & %ln' `hg log -T '{node} ' -r0:2`
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
  0

  $ trylist --optimize --bin '%ln & 2:0' `hg log -T '{node} ' -r0+2+1`
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
  1

 '_list' should not go through the slow follow-order path if order doesn't
 matter:

  $ try -p optimized '2:0 & not (0 + 1)'
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
  2

  $ try -p optimized '2:0 & not (0:2 & (0 + 1))'
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
  2

 because 'present()' does nothing other than suppressing an error, the
 ordering requirement should be forwarded to the nested expression

  $ try -p optimized 'present(2 + 0 + 1)'
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
  1

  $ try --optimize '2:0 & present(0 + 1 + 2)'
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
  0

 'reverse()' should take effect only if it is the outermost expression:

  $ try --optimize '0:2 & reverse(all())'
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
  2

 'sort()' should take effect only if it is the outermost expression:

  $ try --optimize '0:2 & sort(all(), -rev)'
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
  2

 invalid argument passed to noop sort():

  $ log '0:2 & sort()'
  hg: parse error: sort requires one or two arguments
  [255]
  $ log '0:2 & sort(all(), -invalid)'
  hg: parse error: unknown sort key '-invalid'
  [255]

 for 'A & f(B)', 'B' should not be affected by the order of 'A':

  $ try --optimize '2:0 & first(1 + 0 + 2)'
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
  1

  $ try --optimize '2:0 & not last(0 + 2 + 1)'
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
  0

 for 'A & (op)(B)', 'B' should not be affected by the order of 'A':

  $ try --optimize '2:0 & (1 + 0 + 2):(0 + 2 + 1)'
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
  <filteredset
    <spanset- 0:3>,
    <baseset [1]>>
  1

 'A & B' can be rewritten as 'flipand(B, A)' by weight.

  $ try --optimize 'contains("glob:*") & (2 + 0 + 1)'
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
  2

 and in this example, 'A & B' is rewritten as 'B & A', but 'A' overrides
 the order appropriately:

  $ try --optimize 'reverse(contains("glob:*")) & (0 + 2 + 1)'
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
  0

test sort revset
--------------------------------------------

test when adding two unordered revsets

  $ log 'sort(keyword(issue) or modifies(b))'
  4
  6

test when sorting a reversed collection in the same way it is

  $ log 'sort(reverse(all()), -rev)'
  9
  8
  7
  6
  5
  4
  3
  2
  1
  0

test when sorting a reversed collection

  $ log 'sort(reverse(all()), rev)'
  0
  1
  2
  3
  4
  5
  6
  7
  8
  9


test sorting two sorted collections in different orders

  $ log 'sort(outgoing() or reverse(removes(a)), rev)'
  2
  6
  8
  9

test sorting two sorted collections in different orders backwards

  $ log 'sort(outgoing() or reverse(removes(a)), -rev)'
  9
  8
  6
  2

test empty sort key which is noop

  $ log 'sort(0 + 2 + 1, "")'
  0
  2
  1

test invalid sort keys

  $ log 'sort(all(), -invalid)'
  hg: parse error: unknown sort key '-invalid'
  [255]

  $ cd ..

test sorting by multiple keys including variable-length strings

  $ hg init sorting
  $ cd sorting
  $ cat <<EOF >> .hg/hgrc
  > [ui]
  > logtemplate = '{rev} {branch|p5}{desc|p5}{author|p5}{date|hgdate}\n'
  > [templatealias]
  > p5(s) = pad(s, 5)
  > EOF
  $ hg branch -qf b12
  $ hg ci -m m111 -u u112 -d '111 10800'
  $ hg branch -qf b11
  $ hg ci -m m12 -u u111 -d '112 7200'
  $ hg branch -qf b111
  $ hg ci -m m11 -u u12 -d '111 3600'
  $ hg branch -qf b112
  $ hg ci -m m111 -u u11 -d '120 0'
  $ hg branch -qf b111
  $ hg ci -m m112 -u u111 -d '110 14400'
  created new head

 compare revisions (has fast path):

  $ hg log -r 'sort(all(), rev)'
  0 b12  m111 u112 111 10800
  1 b11  m12  u111 112 7200
  2 b111 m11  u12  111 3600
  3 b112 m111 u11  120 0
  4 b111 m112 u111 110 14400

  $ hg log -r 'sort(all(), -rev)'
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0
  2 b111 m11  u12  111 3600
  1 b11  m12  u111 112 7200
  0 b12  m111 u112 111 10800

 compare variable-length strings (issue5218):

  $ hg log -r 'sort(all(), branch)'
  1 b11  m12  u111 112 7200
  2 b111 m11  u12  111 3600
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0
  0 b12  m111 u112 111 10800

  $ hg log -r 'sort(all(), -branch)'
  0 b12  m111 u112 111 10800
  3 b112 m111 u11  120 0
  2 b111 m11  u12  111 3600
  4 b111 m112 u111 110 14400
  1 b11  m12  u111 112 7200

  $ hg log -r 'sort(all(), desc)'
  2 b111 m11  u12  111 3600
  0 b12  m111 u112 111 10800
  3 b112 m111 u11  120 0
  4 b111 m112 u111 110 14400
  1 b11  m12  u111 112 7200

  $ hg log -r 'sort(all(), -desc)'
  1 b11  m12  u111 112 7200
  4 b111 m112 u111 110 14400
  0 b12  m111 u112 111 10800
  3 b112 m111 u11  120 0
  2 b111 m11  u12  111 3600

  $ hg log -r 'sort(all(), user)'
  3 b112 m111 u11  120 0
  1 b11  m12  u111 112 7200
  4 b111 m112 u111 110 14400
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600

  $ hg log -r 'sort(all(), -user)'
  2 b111 m11  u12  111 3600
  0 b12  m111 u112 111 10800
  1 b11  m12  u111 112 7200
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0

 compare dates (tz offset should have no effect):

  $ hg log -r 'sort(all(), date)'
  4 b111 m112 u111 110 14400
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600
  1 b11  m12  u111 112 7200
  3 b112 m111 u11  120 0

  $ hg log -r 'sort(all(), -date)'
  3 b112 m111 u11  120 0
  1 b11  m12  u111 112 7200
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600
  4 b111 m112 u111 110 14400

 be aware that 'sort(x, -k)' is not exactly the same as 'reverse(sort(x, k))'
 because '-k' reverses the comparison, not the list itself:

  $ hg log -r 'sort(0 + 2, date)'
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600

  $ hg log -r 'sort(0 + 2, -date)'
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600

  $ hg log -r 'reverse(sort(0 + 2, date))'
  2 b111 m11  u12  111 3600
  0 b12  m111 u112 111 10800

 sort by multiple keys:

  $ hg log -r 'sort(all(), "branch -rev")'
  1 b11  m12  u111 112 7200
  4 b111 m112 u111 110 14400
  2 b111 m11  u12  111 3600
  3 b112 m111 u11  120 0
  0 b12  m111 u112 111 10800

  $ hg log -r 'sort(all(), "-desc -date")'
  1 b11  m12  u111 112 7200
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600

  $ hg log -r 'sort(all(), "user -branch date rev")'
  3 b112 m111 u11  120 0
  4 b111 m112 u111 110 14400
  1 b11  m12  u111 112 7200
  0 b12  m111 u112 111 10800
  2 b111 m11  u12  111 3600

 toposort prioritises graph branches

  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch a
  $ hg addremove
  adding a
  $ hg ci -m 't1' -u 'tu' -d '130 0'
  created new head
  $ echo 'a' >> a
  $ hg ci -m 't2' -u 'tu' -d '130 0'
  $ hg book book1
  $ hg up 4
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark book1)
  $ touch a
  $ hg addremove
  adding a
  $ hg ci -m 't3' -u 'tu' -d '130 0'

  $ hg log -r 'sort(all(), topo)'
  7 b111 t3   tu   130 0
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0
  6 b111 t2   tu   130 0
  5 b111 t1   tu   130 0
  2 b111 m11  u12  111 3600
  1 b11  m12  u111 112 7200
  0 b12  m111 u112 111 10800

  $ hg log -r 'sort(all(), -topo)'
  0 b12  m111 u112 111 10800
  1 b11  m12  u111 112 7200
  2 b111 m11  u12  111 3600
  5 b111 t1   tu   130 0
  6 b111 t2   tu   130 0
  3 b112 m111 u11  120 0
  4 b111 m112 u111 110 14400
  7 b111 t3   tu   130 0

  $ hg log -r 'sort(all(), topo, topo.firstbranch=book1)'
  6 b111 t2   tu   130 0
  5 b111 t1   tu   130 0
  7 b111 t3   tu   130 0
  4 b111 m112 u111 110 14400
  3 b112 m111 u11  120 0
  2 b111 m11  u12  111 3600
  1 b11  m12  u111 112 7200
  0 b12  m111 u112 111 10800

topographical sorting can't be combined with other sort keys, and you can't
use the topo.firstbranch option when topo sort is not active:

  $ hg log -r 'sort(all(), "topo user")'
  hg: parse error: topo sort order cannot be combined with other sort keys
  [255]

  $ hg log -r 'sort(all(), user, topo.firstbranch=book1)'
  hg: parse error: topo.firstbranch can only be used when using the topo sort key
  [255]

topo.firstbranch should accept any kind of expressions:

  $ hg log -r 'sort(0, topo, topo.firstbranch=(book1))'
  0 b12  m111 u112 111 10800

  $ cd ..
  $ cd repo
