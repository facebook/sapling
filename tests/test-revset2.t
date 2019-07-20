  $ setconfig extensions.treemanifest=!
  $ . helpers-usechg.sh

  $ enable commitextras
  $ setconfig ui.allowemptycommit=1

  $ HGENCODING=utf-8
  $ export HGENCODING
  $ cat > testrevset.py << EOF
  > import edenscm.mercurial.revset
  > 
  > baseset = edenscm.mercurial.revset.baseset
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
  > edenscm.mercurial.revset.symbols['r3232'] = r3232
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > testrevset=$TESTTMP/testrevset.py
  > EOF

  $ try() {
  >   hg debugrevspec --debug "$@"
  > }

  $ log() {
  >   hg log --template '{rev}\n' -r "$1"
  > }

  $ setbranch() {
  >   BRANCH="$1"
  >   # "hg tag" reads this file. Ideally the in-repo tag feature goes way too.
  >   echo "$1" > .hg/branch
  > }

  $ commit() {
  >   if [ -n "$BRANCH" ]; then
  >     hg commit --extra "branch=$BRANCH" "$@"
  >     # silent warnings about conflicted names
  >     hg tag -q --local --remove -- "$BRANCH" 2>/dev/null
  >     hg tag -q --local -- "$BRANCH" 2>/dev/null
  >   else
  >     hg commit "$@"
  >   fi
  > }

extension to build '_intlist()' and '_hexlist()', which is necessary because
these predicates use '\0' as a separator:

  $ cat <<EOF > debugrevlistspec.py
  > from __future__ import absolute_import
  > from edenscm.mercurial import (
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
  $ setbranch a
  $ commit -Aqm0

  $ echo b > b
  $ setbranch b
  $ commit -Aqm1

  $ rm a
  $ setbranch a-b-c-
  $ commit -Aqm2 -u Bob

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
  $ setbranch +a+b+c+
  $ commit -Aqm3

  $ hg co -C 2  # interleave
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bb > b
  $ setbranch -a-b-c-
  $ commit -Aqm4 -d "May 12 2005"

  $ hg co -C 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ setbranch !a/b/c/
  $ commit -Aqm"5 bug"

  $ hg merge 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ setbranch _a_b_c_
  $ commit -Aqm"6 issue619"

  $ setbranch .a.b.c.
  $ commit -Aqm7

  $ setbranch all

  $ hg co 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ setbranch é
  $ commit -Aqm9

  $ hg tag -fr6 1.0
  $ hg bookmark -r6 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

  $ hg clone --quiet -U -r 7 . ../remote1
  $ hg clone --quiet -U -r 8 . ../remote2
  $ echo "[paths]" >> .hg/hgrc
  $ echo "default = ../remote1" >> .hg/hgrc

test subtracting something from an addset

  $ log '(outgoing() or removes(a)) - removes(a)'
  8
  9

test intersecting something with an addset

  $ log 'parents(outgoing() or removes(a))'
  1
  4
  5
  8

test that `or` operation combines elements in the right order:

  $ log '3:4 or 2:5'
  3
  4
  2
  5
  $ log '3:4 or 5:2'
  3
  4
  5
  2
  $ log 'sort(3:4 or 2:5)'
  2
  3
  4
  5
  $ log 'sort(3:4 or 5:2)'
  2
  3
  4
  5

test that more than one `-r`s are combined in the right order and deduplicated:

  $ hg log -T '{rev}\n' -r 3 -r 3 -r 4 -r 5:2 -r 'ancestors(4)'
  3
  4
  5
  2
  0
  1

test that `or` operation skips duplicated revisions from right-hand side

  $ try 'reverse(1::5) or ancestors(4)'
  (or
    (list
      (func
        (symbol 'reverse')
        (dagrange
          (symbol '1')
          (symbol '5')))
      (func
        (symbol 'ancestors')
        (symbol '4'))))
  * set:
  <addset
    <baseset- [1, 3, 5]>,
    <generatorset+>>
  5
  3
  1
  0
  2
  4
  $ try 'sort(ancestors(4) or reverse(1::5))'
  (func
    (symbol 'sort')
    (or
      (list
        (func
          (symbol 'ancestors')
          (symbol '4'))
        (func
          (symbol 'reverse')
          (dagrange
            (symbol '1')
            (symbol '5'))))))
  * set:
  <addset+
    <generatorset+>,
    <baseset- [1, 3, 5]>>
  0
  1
  2
  3
  4
  5

test optimization of trivial `or` operation

  $ try --optimize '0|(1)|"2"|-2|tip|null'
  (or
    (list
      (symbol '0')
      (group
        (symbol '1'))
      (string '2')
      (negate
        (symbol '2'))
      (symbol 'tip')
      (symbol 'null')))
  * optimized:
  (func
    (symbol '_list')
    (string '0\x001\x002\x00-2\x00tip\x00null'))
  * set:
  <baseset [0, 1, 2, 8, 9, -1]>
  0
  1
  2
  8
  9
  -1

  $ try --optimize '0|1|2:3'
  (or
    (list
      (symbol '0')
      (symbol '1')
      (range
        (symbol '2')
        (symbol '3'))))
  * optimized:
  (or
    (list
      (func
        (symbol '_list')
        (string '0\x001'))
      (range
        (symbol '2')
        (symbol '3'))))
  * set:
  <addset
    <baseset [0, 1]>,
    <spanset+ 2:4>>
  0
  1
  2
  3

  $ try --optimize '0:1|2|3:4|5|6'
  (or
    (list
      (range
        (symbol '0')
        (symbol '1'))
      (symbol '2')
      (range
        (symbol '3')
        (symbol '4'))
      (symbol '5')
      (symbol '6')))
  * optimized:
  (or
    (list
      (range
        (symbol '0')
        (symbol '1'))
      (symbol '2')
      (range
        (symbol '3')
        (symbol '4'))
      (func
        (symbol '_list')
        (string '5\x006'))))
  * set:
  <addset
    <addset
      <spanset+ 0:2>,
      <baseset [2]>>,
    <addset
      <spanset+ 3:5>,
      <baseset [5, 6]>>>
  0
  1
  2
  3
  4
  5
  6

unoptimized `or` looks like this

  $ try --no-optimized -p analyzed '0|1|2|3|4'
  * analyzed:
  (or
    (list
      (symbol '0')
      (symbol '1')
      (symbol '2')
      (symbol '3')
      (symbol '4')))
  * set:
  <addset
    <addset
      <baseset [0]>,
      <baseset [1]>>,
    <addset
      <baseset [2]>,
      <addset
        <baseset [3]>,
        <baseset [4]>>>>
  0
  1
  2
  3
  4

test that `_list` should be narrowed by provided `subset`

  $ log '0:2 and (null|1|2|3)'
  1
  2

test that `_list` should remove duplicates

  $ log '0|1|2|1|2|-1|tip'
  0
  1
  2
  9

test unknown revision in `_list`

  $ log '0|unknown'
  abort: unknown revision 'unknown'!
  (if unknown is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

test integer range in `_list`

  $ log '-1|-10'
  9
  0

  $ log '-10|-11'
  abort: unknown revision '-11'!
  (if -11 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ log '9|10'
  abort: unknown revision '10'!
  (if 10 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

test '0000' != '0' in `_list`

  $ log '0|0000'
  0
  -1

test ',' in `_list`
  $ log '0,1'
  hg: parse error: can't use a list in this context
  (see hg help "revsets.x or y")
  [255]
  $ try '0,1,2'
  (list
    (symbol '0')
    (symbol '1')
    (symbol '2'))
  hg: parse error: can't use a list in this context
  (see hg help "revsets.x or y")
  [255]

test that chained `or` operations make balanced addsets

  $ try '0:1|1:2|2:3|3:4|4:5'
  (or
    (list
      (range
        (symbol '0')
        (symbol '1'))
      (range
        (symbol '1')
        (symbol '2'))
      (range
        (symbol '2')
        (symbol '3'))
      (range
        (symbol '3')
        (symbol '4'))
      (range
        (symbol '4')
        (symbol '5'))))
  * set:
  <addset
    <addset
      <spanset+ 0:2>,
      <spanset+ 1:3>>,
    <addset
      <spanset+ 2:4>,
      <addset
        <spanset+ 3:5>,
        <spanset+ 4:6>>>>
  0
  1
  2
  3
  4
  5

no crash by empty group "()" while optimizing `or` operations

  $ try --optimize '0|()'
  (or
    (list
      (symbol '0')
      (group
        None)))
  * optimized:
  (or
    (list
      (symbol '0')
      None))
  hg: parse error: missing argument
  [255]

test that chained `or` operations never eat up stack (issue4624)
(uses `0:1` instead of `0` to avoid future optimization of trivial revisions)

  $ hg log -T '{rev}\n' -r `$PYTHON -c "print '+'.join(['0:1'] * 500)"`
  0
  1

test that repeated `-r` options never eat up stack (issue4565)
(uses `-r 0::1` to avoid possible optimization at old-style parser)

  $ hg log -T '{rev}\n' `$PYTHON -c "for i in xrange(500): print '-r 0::1 ',"`
  0
  1

check that conversion to only works
  $ try --optimize '::3 - ::1'
  (minus
    (dagrangepre
      (symbol '3'))
    (dagrangepre
      (symbol '1')))
  * optimized:
  (func
    (symbol 'only')
    (list
      (symbol '3')
      (symbol '1')))
  * set:
  <baseset+ [3]>
  3
  $ try --optimize 'ancestors(1) - ancestors(3)'
  (minus
    (func
      (symbol 'ancestors')
      (symbol '1'))
    (func
      (symbol 'ancestors')
      (symbol '3')))
  * optimized:
  (func
    (symbol 'only')
    (list
      (symbol '1')
      (symbol '3')))
  * set:
  <baseset+ []>
  $ try --optimize 'not ::2 and ::6'
  (and
    (not
      (dagrangepre
        (symbol '2')))
    (dagrangepre
      (symbol '6')))
  * optimized:
  (func
    (symbol 'only')
    (list
      (symbol '6')
      (symbol '2')))
  * set:
  <baseset+ [3, 4, 5, 6]>
  3
  4
  5
  6
  $ try --optimize 'ancestors(6) and not ancestors(4)'
  (and
    (func
      (symbol 'ancestors')
      (symbol '6'))
    (not
      (func
        (symbol 'ancestors')
        (symbol '4'))))
  * optimized:
  (func
    (symbol 'only')
    (list
      (symbol '6')
      (symbol '4')))
  * set:
  <baseset+ [3, 5, 6]>
  3
  5
  6

no crash by empty group "()" while optimizing to "only()"

  $ try --optimize '::1 and ()'
  (and
    (dagrangepre
      (symbol '1'))
    (group
      None))
  * optimized:
  (andsmally
    (func
      (symbol 'ancestors')
      (symbol '1'))
    None)
  hg: parse error: missing argument
  [255]

optimization to only() works only if ancestors() takes only one argument

  $ hg debugrevspec -p optimized 'ancestors(6) - ancestors(4, 1)'
  * optimized:
  (difference
    (func
      (symbol 'ancestors')
      (symbol '6'))
    (func
      (symbol 'ancestors')
      (list
        (symbol '4')
        (symbol '1'))))
  0
  1
  3
  5
  6
  $ hg debugrevspec -p optimized 'ancestors(6, 1) - ancestors(4)'
  * optimized:
  (difference
    (func
      (symbol 'ancestors')
      (list
        (symbol '6')
        (symbol '1')))
    (func
      (symbol 'ancestors')
      (symbol '4')))
  5
  6

optimization disabled if keyword arguments passed (because we're too lazy
to support it)

  $ hg debugrevspec -p optimized 'ancestors(set=6) - ancestors(set=4)'
  * optimized:
  (difference
    (func
      (symbol 'ancestors')
      (keyvalue
        (symbol 'set')
        (symbol '6')))
    (func
      (symbol 'ancestors')
      (keyvalue
        (symbol 'set')
        (symbol '4'))))
  3
  5
  6

invalid function call should not be optimized to only()

  $ log '"ancestors"(6) and not ancestors(4)'
  hg: parse error: not a symbol
  [255]

  $ log 'ancestors(6) and not "ancestors"(4)'
  hg: parse error: not a symbol
  [255]

we can use patterns when searching for tags

  $ log 'tag("1..*")'
  abort: tag '1..*' does not exist!
  [255]
  $ log 'tag("re:1..*")'
  6
  $ log 'tag("re:[0-9].[0-9]")'
  6
  $ log 'tag("literal:1.0")'
  6
  $ log 'tag("re:0..*")'

  $ log 'tag(unknown)'
  abort: tag 'unknown' does not exist!
  [255]
  $ log 'tag("re:unknown")'
  $ log 'present(tag("unknown"))'
  $ log 'present(tag("re:unknown"))'
  $ log 'user(bob)'
  2

  $ log '4::8'
  4
  8
  $ log '4:8'
  4
  5
  6
  7
  8

  $ log 'sort(!merge() & (modifies(b) | user(bob) | keyword(bug) | keyword(issue) & 1::9), "-date")'
  4
  2
  5

  $ log 'not 0 and 0:2'
  1
  2
  $ log 'not 1 and 0:2'
  0
  2
  $ log 'not 2 and 0:2'
  0
  1
  $ log '(1 and 2)::'
  $ log '(1 and 2):'
  $ log '(1 and 2):3'
  $ log 'sort(head(), -rev)'
  9
  7
  $ log '4::8 - 8'
  4

matching() should preserve the order of the input set:

  $ log '(2 or 3 or 1) and matching(1 or 2 or 3)'
  2
  3
  1

  $ log 'named("unknown")'
  abort: namespace 'unknown' does not exist!
  [255]
  $ log 'named("re:unknown")'
  abort: no namespace exists that match 'unknown'!
  [255]
  $ log 'present(named("unknown"))'
  $ log 'present(named("re:unknown"))'

issue2437

  $ log '3 and p1(5)'
  3
  $ log '4 and p2(6)'
  4
  $ log '1 and parents(:2)'
  1
  $ log '2 and children(1:)'
  2
  $ log 'roots(all()) or roots(all())'
  0
  $ hg debugrevspec 'roots(all()) or roots(all())'
  0

issue2654: report a parse error if the revset was not completely parsed

  $ log '1 OR 2'
  hg: parse error at 2: invalid token
  (1 OR 2
     ^ here)
  [255]

or operator should preserve ordering:
  $ log 'reverse(2::4) or tip'
  4
  2
  9

parentrevspec

  $ log 'merge()^0'
  6
  $ log 'merge()^'
  5
  $ log 'merge()^1'
  5
  $ log 'merge()^2'
  4
  $ log '(not merge())^2'
  $ log 'merge()^^'
  3
  $ log 'merge()^1^'
  3
  $ log 'merge()^^^'
  1

  $ hg debugrevspec -s '(merge() | 0)~-1'
  * set:
  <baseset+ [1, 7]>
  1
  7
  $ log 'merge()~-1'
  7
  $ log 'tip~-1'
  $ log '(tip | merge())~-1'
  7
  $ log 'merge()~0'
  6
  $ log 'merge()~1'
  5
  $ log 'merge()~2'
  3
  $ log 'merge()~2^1'
  1
  $ log 'merge()~3'
  1

  $ log '(-3:tip)^'
  4
  6
  8

  $ log 'tip^foo'
  hg: parse error: ^ expects a number 0, 1, or 2
  [255]

  $ log 'branchpoint()~-1'
  abort: revision in set has more than one child!
  [255]

Bogus function gets suggestions
  $ log 'add()'
  hg: parse error: unknown identifier: add
  (did you mean adds?)
  [255]
  $ log 'added()'
  hg: parse error: unknown identifier: added
  (did you mean adds?)
  [255]
  $ log 'remo()'
  hg: parse error: unknown identifier: remo
  (did you mean one of remote, removes?)
  [255]
  $ log 'babar()'
  hg: parse error: unknown identifier: babar
  [255]

Bogus function with a similar internal name doesn't suggest the internal name
  $ log 'matches()'
  hg: parse error: unknown identifier: matches
  (did you mean matching?)
  [255]

Undocumented functions aren't suggested as similar either
  $ log 'tagged2()'
  hg: parse error: unknown identifier: tagged2
  [255]

multiple revspecs

  $ hg log -r 'tip~1:tip' -r 'tip~2:tip~1' --template '{rev}\n'
  8
  9
  4
  5
  6
  7

test usage in revpair (with "+")

(real pair)

  $ hg diff -r 'tip^^' -r 'tip'
  diff -r 2326846efdab -r 24286f4ae135 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0
  $ hg diff -r 'tip^^::tip'
  diff -r 2326846efdab -r 24286f4ae135 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0

(single rev)

  $ hg diff -r 'tip^' -r 'tip^'
  $ hg diff -r 'tip^:tip^'

(single rev that does not looks like a range)

  $ hg diff -r 'tip^::tip^ or tip^'
  diff -r d5d0dcbdc4d9 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	* (glob)
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0
  $ hg diff -r 'tip^ or tip^'
  diff -r d5d0dcbdc4d9 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	* (glob)
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0

(no rev)

  $ hg diff -r 'author("babar") or author("celeste")'
  abort: empty revision range
  [255]

aliases:

  $ echo '[revsetalias]' >> .hg/hgrc
  $ echo 'm = merge()' >> .hg/hgrc
(revset aliases can override builtin revsets)
  $ echo 'p2($1) = p1($1)' >> .hg/hgrc
  $ echo 'sincem = descendants(m)' >> .hg/hgrc
  $ echo 'd($1) = reverse(sort($1, date))' >> .hg/hgrc
  $ echo 'rs(ARG1, ARG2) = reverse(sort(ARG1, ARG2))' >> .hg/hgrc
  $ echo 'rs4(ARG1, ARGA, ARGB, ARG2) = reverse(sort(ARG1, ARG2))' >> .hg/hgrc

  $ try m
  (symbol 'm')
  * expanded:
  (func
    (symbol 'merge')
    None)
  * set:
  <filteredset
    <fullreposet+ 0:10>,
    <merge>>
  6

  $ HGPLAIN=1
  $ export HGPLAIN
  $ try m
  (symbol 'm')
  abort: unknown revision 'm'!
  (if m is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ HGPLAINEXCEPT=revsetalias
  $ export HGPLAINEXCEPT
  $ try m
  (symbol 'm')
  * expanded:
  (func
    (symbol 'merge')
    None)
  * set:
  <filteredset
    <fullreposet+ 0:10>,
    <merge>>
  6

  $ unset HGPLAIN
  $ unset HGPLAINEXCEPT

  $ try 'p2(.)'
  (func
    (symbol 'p2')
    (symbol '.'))
  * expanded:
  (func
    (symbol 'p1')
    (symbol '.'))
  * set:
  <baseset+ [8]>
  8

  $ HGPLAIN=1
  $ export HGPLAIN
  $ try 'p2(.)'
  (func
    (symbol 'p2')
    (symbol '.'))
  * set:
  <baseset+ []>

  $ HGPLAINEXCEPT=revsetalias
  $ export HGPLAINEXCEPT
  $ try 'p2(.)'
  (func
    (symbol 'p2')
    (symbol '.'))
  * expanded:
  (func
    (symbol 'p1')
    (symbol '.'))
  * set:
  <baseset+ [8]>
  8

  $ unset HGPLAIN
  $ unset HGPLAINEXCEPT

test alias recursion

  $ try sincem
  (symbol 'sincem')
  * expanded:
  (func
    (symbol 'descendants')
    (func
      (symbol 'merge')
      None))
  * set:
  <generatorset+>
  6
  7

test infinite recursion

  $ echo 'recurse1 = recurse2' >> .hg/hgrc
  $ echo 'recurse2 = recurse1' >> .hg/hgrc
  $ try recurse1
  (symbol 'recurse1')
  hg: parse error: infinite expansion of revset alias "recurse1" detected
  [255]

  $ echo 'level1($1, $2) = $1 or $2' >> .hg/hgrc
  $ echo 'level2($1, $2) = level1($2, $1)' >> .hg/hgrc
  $ try "level2(level1(1, 2), 3)"
  (func
    (symbol 'level2')
    (list
      (func
        (symbol 'level1')
        (list
          (symbol '1')
          (symbol '2')))
      (symbol '3')))
  * expanded:
  (or
    (list
      (symbol '3')
      (or
        (list
          (symbol '1')
          (symbol '2')))))
  * set:
  <addset
    <baseset [3]>,
    <baseset [1, 2]>>
  3
  1
  2

test nesting and variable passing

  $ echo 'nested($1) = nested2($1)' >> .hg/hgrc
  $ echo 'nested2($1) = nested3($1)' >> .hg/hgrc
  $ echo 'nested3($1) = max($1)' >> .hg/hgrc
  $ try 'nested(2:5)'
  (func
    (symbol 'nested')
    (range
      (symbol '2')
      (symbol '5')))
  * expanded:
  (func
    (symbol 'max')
    (range
      (symbol '2')
      (symbol '5')))
  * set:
  <baseset
    <max
      <fullreposet+ 0:10>,
      <spanset+ 2:6>>>
  5

test chained `or` operations are flattened at parsing phase

  $ echo 'chainedorops($1, $2, $3) = $1|$2|$3' >> .hg/hgrc
  $ try 'chainedorops(0:1, 1:2, 2:3)'
  (func
    (symbol 'chainedorops')
    (list
      (range
        (symbol '0')
        (symbol '1'))
      (range
        (symbol '1')
        (symbol '2'))
      (range
        (symbol '2')
        (symbol '3'))))
  * expanded:
  (or
    (list
      (range
        (symbol '0')
        (symbol '1'))
      (range
        (symbol '1')
        (symbol '2'))
      (range
        (symbol '2')
        (symbol '3'))))
  * set:
  <addset
    <spanset+ 0:2>,
    <addset
      <spanset+ 1:3>,
      <spanset+ 2:4>>>
  0
  1
  2
  3

test variable isolation, variable placeholders are rewritten as string
then parsed and matched again as string. Check they do not leak too
far away.

  $ echo 'injectparamasstring = max("$1")' >> .hg/hgrc
  $ echo 'callinjection($1) = descendants(injectparamasstring)' >> .hg/hgrc
  $ try 'callinjection(2:5)'
  (func
    (symbol 'callinjection')
    (range
      (symbol '2')
      (symbol '5')))
  * expanded:
  (func
    (symbol 'descendants')
    (func
      (symbol 'max')
      (string '$1')))
  abort: unknown revision '$1'!
  (if $1 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

test scope of alias expansion: 'universe' is expanded prior to 'shadowall(0)',
but 'all()' should never be substituted to '0()'.

  $ echo 'universe = all()' >> .hg/hgrc
  $ echo 'shadowall(all) = all and universe' >> .hg/hgrc
  $ try 'shadowall(0)'
  (func
    (symbol 'shadowall')
    (symbol '0'))
  * expanded:
  (and
    (symbol '0')
    (func
      (symbol 'all')
      None))
  * set:
  <filteredset
    <baseset [0]>,
    <spanset+ 0:10>>
  0

test unknown reference:

  $ try "unknownref(0)" --config 'revsetalias.unknownref($1)=$1:$2'
  (func
    (symbol 'unknownref')
    (symbol '0'))
  abort: bad definition of revset alias "unknownref": invalid symbol '$2'
  [255]

  $ hg debugrevspec --debug --config revsetalias.anotherbadone='branch(' "tip"
  (symbol 'tip')
  warning: bad definition of revset alias "anotherbadone": at 7: not a prefix: end
  * set:
  <baseset [9]>
  9

  $ try 'tip'
  (symbol 'tip')
  * set:
  <baseset [9]>
  9

  $ hg debugrevspec --debug --config revsetalias.'bad name'='tip' "tip"
  (symbol 'tip')
  warning: bad declaration of revset alias "bad name": at 4: invalid token
  * set:
  <baseset [9]>
  9
  $ echo 'strictreplacing($1, $10) = $10 or desc("$1")' >> .hg/hgrc
  $ try 'strictreplacing("foo", tip)'
  (func
    (symbol 'strictreplacing')
    (list
      (string 'foo')
      (symbol 'tip')))
  * expanded:
  (or
    (list
      (symbol 'tip')
      (func
        (symbol 'desc')
        (string '$1'))))
  * set:
  <addset
    <baseset [9]>,
    <filteredset
      <fullreposet+ 0:10>,
      <desc '$1'>>>
  9

  $ try 'd(2:5)'
  (func
    (symbol 'd')
    (range
      (symbol '2')
      (symbol '5')))
  * expanded:
  (func
    (symbol 'reverse')
    (func
      (symbol 'sort')
      (list
        (range
          (symbol '2')
          (symbol '5'))
        (symbol 'date'))))
  * set:
  <baseset [4, 5, 3, 2]>
  4
  5
  3
  2
  $ try 'rs(2 or 3, date)'
  (func
    (symbol 'rs')
    (list
      (or
        (list
          (symbol '2')
          (symbol '3')))
      (symbol 'date')))
  * expanded:
  (func
    (symbol 'reverse')
    (func
      (symbol 'sort')
      (list
        (or
          (list
            (symbol '2')
            (symbol '3')))
        (symbol 'date'))))
  * set:
  <baseset [3, 2]>
  3
  2
  $ try 'rs()'
  (func
    (symbol 'rs')
    None)
  hg: parse error: invalid number of arguments: 0
  [255]
  $ try 'rs(2)'
  (func
    (symbol 'rs')
    (symbol '2'))
  hg: parse error: invalid number of arguments: 1
  [255]
  $ try 'rs(2, data, 7)'
  (func
    (symbol 'rs')
    (list
      (symbol '2')
      (symbol 'data')
      (symbol '7')))
  hg: parse error: invalid number of arguments: 3
  [255]
  $ try 'rs4(2 or 3, x, x, date)'
  (func
    (symbol 'rs4')
    (list
      (or
        (list
          (symbol '2')
          (symbol '3')))
      (symbol 'x')
      (symbol 'x')
      (symbol 'date')))
  * expanded:
  (func
    (symbol 'reverse')
    (func
      (symbol 'sort')
      (list
        (or
          (list
            (symbol '2')
            (symbol '3')))
        (symbol 'date'))))
  * set:
  <baseset [3, 2]>
  3
  2

issue4553: check that revset aliases override existing hash prefix

  $ hg log -qr e
  6:e0cc66ef77e8

  $ hg log -qr e --config revsetalias.e="all()"
  0:2785f51eece5
  1:d75937da8da0
  2:5ed5505e9f1c
  3:8528aa5637f2
  4:2326846efdab
  5:904fa392b941
  6:e0cc66ef77e8
  7:013af1973af4
  8:d5d0dcbdc4d9
  9:24286f4ae135

  $ hg log -qr e: --config revsetalias.e="0"
  0:2785f51eece5
  1:d75937da8da0
  2:5ed5505e9f1c
  3:8528aa5637f2
  4:2326846efdab
  5:904fa392b941
  6:e0cc66ef77e8
  7:013af1973af4
  8:d5d0dcbdc4d9
  9:24286f4ae135

  $ hg log -qr :e --config revsetalias.e="9"
  0:2785f51eece5
  1:d75937da8da0
  2:5ed5505e9f1c
  3:8528aa5637f2
  4:2326846efdab
  5:904fa392b941
  6:e0cc66ef77e8
  7:013af1973af4
  8:d5d0dcbdc4d9
  9:24286f4ae135

  $ hg log -qr e:
  6:e0cc66ef77e8
  7:013af1973af4
  8:d5d0dcbdc4d9
  9:24286f4ae135

  $ hg log -qr :e
  0:2785f51eece5
  1:d75937da8da0
  2:5ed5505e9f1c
  3:8528aa5637f2
  4:2326846efdab
  5:904fa392b941
  6:e0cc66ef77e8

issue2549 - correct optimizations

  $ try 'limit(1 or 2 or 3, 2) and not 2'
  (and
    (func
      (symbol 'limit')
      (list
        (or
          (list
            (symbol '1')
            (symbol '2')
            (symbol '3')))
        (symbol '2')))
    (not
      (symbol '2')))
  * set:
  <filteredset
    <baseset [1, 2]>,
    <not
      <baseset [2]>>>
  1
  $ try 'max(1 or 2) and not 2'
  (and
    (func
      (symbol 'max')
      (or
        (list
          (symbol '1')
          (symbol '2'))))
    (not
      (symbol '2')))
  * set:
  <filteredset
    <baseset
      <max
        <fullreposet+ 0:10>,
        <baseset [1, 2]>>>,
    <not
      <baseset [2]>>>
  $ try 'min(1 or 2) and not 1'
  (and
    (func
      (symbol 'min')
      (or
        (list
          (symbol '1')
          (symbol '2'))))
    (not
      (symbol '1')))
  * set:
  <filteredset
    <baseset
      <min
        <fullreposet+ 0:10>,
        <baseset [1, 2]>>>,
    <not
      <baseset [1]>>>
  $ try 'last(1 or 2, 1) and not 2'
  (and
    (func
      (symbol 'last')
      (list
        (or
          (list
            (symbol '1')
            (symbol '2')))
        (symbol '1')))
    (not
      (symbol '2')))
  * set:
  <filteredset
    <baseset [2]>,
    <not
      <baseset [2]>>>

issue4289 - ordering of built-ins
  $ hg log -M -q -r 3:2
  3:8528aa5637f2
  2:5ed5505e9f1c

test revsets started with 40-chars hash (issue3669)

  $ ISSUE3669_TIP=`hg tip --template '{node}'`
  $ hg log -r "${ISSUE3669_TIP}" --template '{rev}\n'
  9
  $ hg log -r "${ISSUE3669_TIP}^" --template '{rev}\n'
  8

test or-ed indirect predicates (issue3775)

  $ log '6 or 6^1' | sort
  5
  6
  $ log '6^1 or 6' | sort
  5
  6
  $ log '4 or 4~1' | sort
  2
  4
  $ log '4~1 or 4' | sort
  2
  4
  $ log '(0 or 2):(4 or 6) or 0 or 6' | sort
  0
  1
  2
  3
  4
  5
  6
  $ log '0 or 6 or (0 or 2):(4 or 6)' | sort
  0
  1
  2
  3
  4
  5
  6

tests for 'remote()' predicate:
#.  (csets in remote) (id)            (remote)
1.  less than local   current branch  "default"
2.  same with local   specified       "default"
3.  more than local   specified       specified

  $ hg clone --quiet -U . ../remote3
  $ cd ../remote3
  $ hg update -q 7
  $ echo r > r
  $ hg ci -Aqm 10
  $ log 'remote()'
  7
  $ log 'remote("a-b-c-")'
  2
  $ cd ../repo

tests for concatenation of strings/symbols by "##"

  $ try "278 ## '5f5' ## 1ee ## 'ce5'"
  (_concat
    (_concat
      (_concat
        (symbol '278')
        (string '5f5'))
      (symbol '1ee'))
    (string 'ce5'))
  * concatenated:
  (string '2785f51eece5')
  * set:
  <baseset [0]>
  0

  $ echo 'cat4($1, $2, $3, $4) = $1 ## $2 ## $3 ## $4' >> .hg/hgrc
  $ try "cat4(278, '5f5', 1ee, 'ce5')"
  (func
    (symbol 'cat4')
    (list
      (symbol '278')
      (string '5f5')
      (symbol '1ee')
      (string 'ce5')))
  * expanded:
  (_concat
    (_concat
      (_concat
        (symbol '278')
        (string '5f5'))
      (symbol '1ee'))
    (string 'ce5'))
  * concatenated:
  (string '2785f51eece5')
  * set:
  <baseset [0]>
  0

(check concatenation in alias nesting)

  $ echo 'cat2($1, $2) = $1 ## $2' >> .hg/hgrc
  $ echo 'cat2x2($1, $2, $3, $4) = cat2($1 ## $2, $3 ## $4)' >> .hg/hgrc
  $ log "cat2x2(278, '5f5', 1ee, 'ce5')"
  0

(check operator priority)

  $ echo 'cat2n2($1, $2, $3, $4) = $1 ## $2 or $3 ## $4~2' >> .hg/hgrc
  $ log "cat2n2(2785f5, 1eece5, 24286f, 4ae135)"
  0
  4

  $ cd ..

prepare repository that has "default" branches of multiple roots

  $ hg init namedbranch
  $ cd namedbranch

  $ echo default0 >> a
  $ hg ci -Aqm0
  $ echo default1 >> a
  $ hg ci -m1

  $ setbranch stable
  $ echo stable2 >> a
  $ commit -m2
  $ echo stable3 >> a
  $ commit -m3

  $ hg update -q null
  $ echo default4 >> a
  $ hg ci -Aqm4
  $ echo default5 >> a
  $ hg ci -m5

  $ cd ..

test author/desc/keyword in problematic encoding
# unicode: cp932:
# u30A2    0x83 0x41(= 'A')
# u30C2    0x83 0x61(= 'a')

  $ hg init problematicencoding
  $ cd problematicencoding

  $ $PYTHON > setup.sh <<EOF
  > print u'''
  > echo a > text
  > hg add text
  > hg --encoding utf-8 commit -u '\u30A2' -m none
  > echo b > text
  > hg --encoding utf-8 commit -u '\u30C2' -m none
  > echo c > text
  > hg --encoding utf-8 commit -u none -m '\u30A2'
  > echo d > text
  > hg --encoding utf-8 commit -u none -m '\u30C2'
  > '''.encode('utf-8')
  > EOF
  $ sh < setup.sh

test in problematic encoding
  $ $PYTHON > test.sh <<EOF
  > print u'''
  > hg --encoding cp932 log --template '{rev}\\n' -r 'author(\u30A2)'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -r 'author(\u30C2)'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -r 'desc(\u30A2)'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -r 'desc(\u30C2)'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -r 'keyword(\u30A2)'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -r 'keyword(\u30C2)'
  > '''.encode('cp932')
  > EOF
  $ sh < test.sh
  abort: cannot decode command line arguments
  ====
  abort: cannot decode command line arguments
  ====
  abort: cannot decode command line arguments
  ====
  abort: cannot decode command line arguments
  ====
  abort: cannot decode command line arguments
  ====
  abort: cannot decode command line arguments
  [255]

test error message of bad revset
  $ hg log -r 'foo\\'
  hg: parse error at 3: syntax error in revset 'foo\\'
  (foo\\
      ^ here)
  [255]

  $ cd ..

Test that revset predicate of extension isn't loaded at failure of
loading it

  $ cd repo

  $ cat <<EOF > $TESTTMP/custompredicate.py
  > from edenscm.mercurial import error, registrar, revset
  > 
  > revsetpredicate = registrar.revsetpredicate()
  > 
  > @revsetpredicate('custom1()')
  > def custom1(repo, subset, x):
  >     return revset.baseset([1])
  > 
  > raise error.Abort('intentional failure of loading extension')
  > EOF
  $ cat <<EOF > .hg/hgrc
  > [extensions]
  > custompredicate = $TESTTMP/custompredicate.py
  > EOF

  $ hg debugrevspec "custom1()"
  *** failed to import extension custompredicate from $TESTTMP/custompredicate.py: intentional failure of loading extension
  hg: parse error: unknown identifier: custom1
  [255]

Test repo.anyrevs with customized revset overrides

  $ cat > $TESTTMP/printprevset.py <<EOF
  > from edenscm.mercurial import encoding, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('printprevset')
  > def printprevset(ui, repo):
  >     alias = {}
  >     p = encoding.environ.get('P')
  >     if p:
  >         alias['P'] = p
  >     revs = repo.anyrevs(['P'], user=True, localalias=alias)
  >     ui.write('P=%r\n' % list(revs))
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > custompredicate = !
  > printprevset = $TESTTMP/printprevset.py
  > EOF

  $ hg --config revsetalias.P=1 printprevset
  P=[1]
  $ P=3 hg --config revsetalias.P=2 printprevset
  P=[3]

  $ cd ..

Test obsstore related revsets

  $ hg init repo1
  $ cd repo1
  $ cat <<EOF >> .hg/hgrc
  > [experimental]
  > evolution.createmarkers=True
  > EOF

  $ drawdag <<'EOS'
  > H      F G
  > |      |/    # split: B -> E, F
  > B C D  E     # amend: B -> C -> D
  >  \|/   |     # amend: F -> G
  >   A    A  Z  # amend: A -> Z
  > EOS

  $ hg log -G -r "all()" -T '{desc}\n'
  o  H
  |
  | o  G
  | |
  | o  E
  | |
  | | o  D
  | |/
  x |  B
  |/
  | o  Z
  |
  x  A
  

  $ hg log -r "successors($Z)" -T '{desc}\n'
  Z

  $ hg log -r "successors($F)" -T '{desc}\n' --hidden
  F
  G

  $ hg log -r "predecessors($Z)" -T '{desc}\n'
  A
  Z

  $ hg log -r "predecessors($A)" -T '{desc}\n'
  A

  $ hg log -r "successors($B)" -T '{desc}\n'
  B
  D
  E
  G

  $ hg log -r "successors($B)" -T '{desc}\n' --hidden
  B
  C
  D
  E
  F
  G

  $ hg log -r "successors($B,1)" -T '{desc}\n' --hidden
  B
  C
  E
  F

  $ hg log -r "predecessors($D)" -T '{desc}\n'
  B
  D

  $ hg log -r "predecessors($D)" -T '{desc}\n' --hidden
  B
  C
  D

  $ hg log -r "predecessors($D,1)" -T '{desc}\n' --hidden
  C
  D

  $ hg log -r "successors($B)-obsolete()" -T '{desc}\n' --hidden
  D
  E
  G

  $ hg log -r "successors($B+$A)-contentdivergent()" -T '{desc}\n'
  A
  Z
  B

  $ hg log -r "successors($B+$A)-contentdivergent()-obsolete()" -T '{desc}\n'
  Z

Test `draft() & ::x` optimization

  $ hg init $TESTTMP/repo2
  $ cd $TESTTMP/repo2
  $ hg debugdrawdag <<'EOS'
  >   P5 S1
  >    |  |
  > S2 | D3
  >   \|/
  >   P4
  >    |
  >   P3 D2
  >    |  |
  >   P2 D1
  >    |/
  >   P1
  >    |
  >   P0
  > EOS
  $ hg phase --public -r P5
  $ hg phase --force --secret -r S1+S2
  $ hg log -G -T '{rev} {desc} {phase}' -r 'sort(all(), topo, topo.firstbranch=P5)'
  o  8 P5 public
  |
  | o  10 S1 secret
  | |
  | o  7 D3 draft
  |/
  | o  9 S2 secret
  |/
  o  6 P4 public
  |
  o  5 P3 public
  |
  o  3 P2 public
  |
  | o  4 D2 draft
  | |
  | o  2 D1 draft
  |/
  o  1 P1 public
  |
  o  0 P0 public
  
  $ hg debugrevspec --verify -p analyzed -p optimized 'draft() & ::(((S1+D1+P5)-D3)+S2)'
  * analyzed:
  (and
    (func
      (symbol 'draft')
      None)
    (func
      (symbol 'ancestors')
      (or
        (list
          (and
            (or
              (list
                (symbol 'S1')
                (symbol 'D1')
                (symbol 'P5')))
            (not
              (symbol 'D3')))
          (symbol 'S2')))))
  * optimized:
  (func
    (symbol '_phaseandancestors')
    (list
      (symbol 'draft')
      (or
        (list
          (difference
            (func
              (symbol '_list')
              (string 'S1\x00D1\x00P5'))
            (symbol 'D3'))
          (symbol 'S2')))))
  $ hg debugrevspec --verify -p analyzed -p optimized 'secret() & ::9'
  * analyzed:
  (and
    (func
      (symbol 'secret')
      None)
    (func
      (symbol 'ancestors')
      (symbol '9')))
  * optimized:
  (func
    (symbol '_phaseandancestors')
    (list
      (symbol 'secret')
      (symbol '9')))
  $ hg debugrevspec --verify -p analyzed -p optimized '7 & ( (not public()) & ::(tag()) )'
  * analyzed:
  (and
    (symbol '7')
    (and
      (not
        (func
          (symbol 'public')
          None))
      (func
        (symbol 'ancestors')
        (func
          (symbol 'tag')
          None))))
  * optimized:
  (and
    (symbol '7')
    (func
      (symbol '_phaseandancestors')
      (list
        (symbol '_notpublic')
        (func
          (symbol 'tag')
          None))))
  $ hg debugrevspec --verify -p optimized '(not public()) & ancestors(S1+D2+P5, 1)'
  * optimized:
  (and
    (func
      (symbol '_notpublic')
      None)
    (func
      (symbol 'ancestors')
      (list
        (func
          (symbol '_list')
          (string 'S1\x00D2\x00P5'))
        (symbol '1'))))
  $ hg debugrevspec --verify -p optimized '(not public()) & ancestors(S1+D2+P5, depth=1)'
  * optimized:
  (and
    (func
      (symbol '_notpublic')
      None)
    (func
      (symbol 'ancestors')
      (list
        (func
          (symbol '_list')
          (string 'S1\x00D2\x00P5'))
        (keyvalue
          (symbol 'depth')
          (symbol '1')))))
