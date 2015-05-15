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
  > testrevset=$TESTTMP/testrevset.py
  > EOF

  $ try() {
  >   hg debugrevspec --debug "$@"
  > }

  $ log() {
  >   hg log --template '{rev}\n' -r "$1"
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
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm1

  $ rm a
  $ hg branch a-b-c-
  marked working directory as branch a-b-c-
  (branches are permanent and global, did you want a bookmark?)
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
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm3

  $ hg co 2  # interleave
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bb > b
  $ hg branch -- -a-b-c-
  marked working directory as branch -a-b-c-
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm4 -d "May 12 2005"

  $ hg co 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch !a/b/c/
  marked working directory as branch !a/b/c/
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm"5 bug"

  $ hg merge 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg branch _a_b_c_
  marked working directory as branch _a_b_c_
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm"6 issue619"

  $ hg branch .a.b.c.
  marked working directory as branch .a.b.c.
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Aqm7

  $ hg branch all
  marked working directory as branch all
  (branches are permanent and global, did you want a bookmark?)

  $ hg co 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch é
  marked working directory as branch \xc3\xa9 (esc)
  (branches are permanent and global, did you want a bookmark?)
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
    ('symbol', '0')
    ('symbol', '1'))
  * set:
  <spanset+ 0:1>
  0
  1
  $ try 3::6
  (dagrange
    ('symbol', '3')
    ('symbol', '6'))
  * set:
  <baseset [3, 5, 6]>
  3
  5
  6
  $ try '0|1|2'
  (or
    (or
      ('symbol', '0')
      ('symbol', '1'))
    ('symbol', '2'))
  * set:
  <addset
    <addset
      <baseset [0]>,
      <baseset [1]>>,
    <baseset [2]>>
  0
  1
  2

names that should work without quoting

  $ try a
  ('symbol', 'a')
  * set:
  <baseset [0]>
  0
  $ try b-a
  (minus
    ('symbol', 'b')
    ('symbol', 'a'))
  * set:
  <filteredset
    <baseset [1]>>
  1
  $ try _a_b_c_
  ('symbol', '_a_b_c_')
  * set:
  <baseset [6]>
  6
  $ try _a_b_c_-a
  (minus
    ('symbol', '_a_b_c_')
    ('symbol', 'a'))
  * set:
  <filteredset
    <baseset [6]>>
  6
  $ try .a.b.c.
  ('symbol', '.a.b.c.')
  * set:
  <baseset [7]>
  7
  $ try .a.b.c.-a
  (minus
    ('symbol', '.a.b.c.')
    ('symbol', 'a'))
  * set:
  <filteredset
    <baseset [7]>>
  7
  $ try -- '-a-b-c-' # complains
  hg: parse error at 7: not a prefix: end
  [255]
  $ log -a-b-c- # succeeds with fallback
  4

  $ try -- -a-b-c--a # complains
  (minus
    (minus
      (minus
        (negate
          ('symbol', 'a'))
        ('symbol', 'b'))
      ('symbol', 'c'))
    (negate
      ('symbol', 'a')))
  abort: unknown revision '-a'!
  [255]
  $ try é
  ('symbol', '\xc3\xa9')
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
    ('string', '-a-b-c-')
    ('symbol', 'a'))
  * set:
  <filteredset
    <baseset [4]>>
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
    (and
      ('symbol', '1')
      ('symbol', '2'))
    ('symbol', '3'))
  * set:
  <addset
    <baseset []>,
    <baseset [3]>>
  3
  $ try '1|2&3'
  (or
    ('symbol', '1')
    (and
      ('symbol', '2')
      ('symbol', '3')))
  * set:
  <addset
    <baseset [1]>,
    <baseset []>>
  1
  $ try '1&2&3' # associativity
  (and
    (and
      ('symbol', '1')
      ('symbol', '2'))
    ('symbol', '3'))
  * set:
  <baseset []>
  $ try '1|(2|3)'
  (or
    ('symbol', '1')
    (group
      (or
        ('symbol', '2')
        ('symbol', '3'))))
  * set:
  <addset
    <baseset [1]>,
    <addset
      <baseset [2]>,
      <baseset [3]>>>
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
  $ log 'date(tip)'
  abort: invalid date: 'tip'
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

Test that symbols only get parsed as functions if there's an opening
parenthesis.

  $ hg book only -r 9
  $ log 'only(only)'   # Outer "only" is a function, inner "only" is the bookmark
  8
  9

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
    ('symbol', 'grep')
    ('string', '('))
  hg: parse error: invalid match pattern: unbalanced parenthesis
  [255]
  $ try 'grep("\bissue\d+")'
  (func
    ('symbol', 'grep')
    ('string', '\x08issue\\d+'))
  * set:
  <filteredset
    <fullreposet+ 0:9>>
  $ try 'grep(r"\bissue\d+")'
  (func
    ('symbol', 'grep')
    ('string', '\\bissue\\d+'))
  * set:
  <filteredset
    <fullreposet+ 0:9>>
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
  $ log 'limit(head(), 1)'
  0
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

Test opreand of '%' is optimized recursively (issue4670)

  $ try --optimize '8:9-8%'
  (onlypost
    (minus
      (range
        ('symbol', '8')
        ('symbol', '9'))
      ('symbol', '8')))
  * optimized:
  (func
    ('symbol', 'only')
    (and
      (range
        ('symbol', '8')
        ('symbol', '9'))
      (not
        ('symbol', '8'))))
  * set:
  <baseset+ [8, 9]>
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
  -1
  $ log 'tip:null and all()' | tail -2
  1
  0

Test working-directory revision
  $ hg debugrevspec 'wdir()'
  None
  $ hg debugrevspec 'tip or wdir()'
  9
  None
  $ hg debugrevspec '0:tip and wdir()'

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

test that `or` operation skips duplicated revisions from right-hand side

  $ try 'reverse(1::5) or ancestors(4)'
  (or
    (func
      ('symbol', 'reverse')
      (dagrange
        ('symbol', '1')
        ('symbol', '5')))
    (func
      ('symbol', 'ancestors')
      ('symbol', '4')))
  * set:
  <addset
    <baseset [5, 3, 1]>,
    <filteredset
      <filteredset
        <fullreposet+ 0:9>>>>
  5
  3
  1
  0
  2
  4
  $ try 'sort(ancestors(4) or reverse(1::5))'
  (func
    ('symbol', 'sort')
    (or
      (func
        ('symbol', 'ancestors')
        ('symbol', '4'))
      (func
        ('symbol', 'reverse')
        (dagrange
          ('symbol', '1')
          ('symbol', '5')))))
  * set:
  <addset+
    <generatorset+>,
    <filteredset
      <baseset [5, 3, 1]>>>
  0
  1
  2
  3
  4
  5

check that conversion to only works
  $ try --optimize '::3 - ::1'
  (minus
    (dagrangepre
      ('symbol', '3'))
    (dagrangepre
      ('symbol', '1')))
  * optimized:
  (func
    ('symbol', 'only')
    (list
      ('symbol', '3')
      ('symbol', '1')))
  * set:
  <baseset+ [3]>
  3
  $ try --optimize 'ancestors(1) - ancestors(3)'
  (minus
    (func
      ('symbol', 'ancestors')
      ('symbol', '1'))
    (func
      ('symbol', 'ancestors')
      ('symbol', '3')))
  * optimized:
  (func
    ('symbol', 'only')
    (list
      ('symbol', '1')
      ('symbol', '3')))
  * set:
  <baseset+ []>
  $ try --optimize 'not ::2 and ::6'
  (and
    (not
      (dagrangepre
        ('symbol', '2')))
    (dagrangepre
      ('symbol', '6')))
  * optimized:
  (func
    ('symbol', 'only')
    (list
      ('symbol', '6')
      ('symbol', '2')))
  * set:
  <baseset+ [3, 4, 5, 6]>
  3
  4
  5
  6
  $ try --optimize 'ancestors(6) and not ancestors(4)'
  (and
    (func
      ('symbol', 'ancestors')
      ('symbol', '6'))
    (not
      (func
        ('symbol', 'ancestors')
        ('symbol', '4'))))
  * optimized:
  (func
    ('symbol', 'only')
    (list
      ('symbol', '6')
      ('symbol', '4')))
  * set:
  <baseset+ [3, 5, 6]>
  3
  5
  6

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
  $ log 'branch(unknown)'
  abort: unknown revision 'unknown'!
  [255]
  $ log 'branch("re:unknown")'
  $ log 'present(branch("unknown"))'
  $ log 'present(branch("re:unknown"))'
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
  6
  5
  4
  3
  2
  1
  0
  $ log '4::8 - 8'
  4
  $ log 'matching(1 or 2 or 3) and (2 or 3 or 1)'
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

  $ log 'tag()'
  6
  $ log 'named("tags")'
  6

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
  $ log 'heads(branch(é)) or heads(branch(é))'
  9
  $ log 'ancestors(8) and (heads(branch("-a-b-c-")) or heads(branch(é)))'
  4

issue2654: report a parse error if the revset was not completely parsed

  $ log '1 OR 2'
  hg: parse error at 2: invalid token
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
  $ log 'merge()^^'
  3
  $ log 'merge()^1^'
  3
  $ log 'merge()^^^'
  1

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

Bogus function gets suggestions
  $ log 'add()'
  hg: parse error: unknown identifier: add
  (did you mean 'adds'?)
  [255]
  $ log 'added()'
  hg: parse error: unknown identifier: added
  (did you mean 'adds'?)
  [255]
  $ log 'remo()'
  hg: parse error: unknown identifier: remo
  (did you mean one of remote, removes?)
  [255]
  $ log 'babar()'
  hg: parse error: unknown identifier: babar
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
  $ hg diff -r 'tip^::tip^ or tip^'

(single rev that does not looks like a range)

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
  ('symbol', 'm')
  (func
    ('symbol', 'merge')
    None)
  * set:
  <filteredset
    <fullreposet+ 0:9>>
  6

  $ HGPLAIN=1
  $ export HGPLAIN
  $ try m
  ('symbol', 'm')
  abort: unknown revision 'm'!
  [255]

  $ HGPLAINEXCEPT=revsetalias
  $ export HGPLAINEXCEPT
  $ try m
  ('symbol', 'm')
  (func
    ('symbol', 'merge')
    None)
  * set:
  <filteredset
    <fullreposet+ 0:9>>
  6

  $ unset HGPLAIN
  $ unset HGPLAINEXCEPT

  $ try 'p2(.)'
  (func
    ('symbol', 'p2')
    ('symbol', '.'))
  (func
    ('symbol', 'p1')
    ('symbol', '.'))
  * set:
  <baseset+ [8]>
  8

  $ HGPLAIN=1
  $ export HGPLAIN
  $ try 'p2(.)'
  (func
    ('symbol', 'p2')
    ('symbol', '.'))
  * set:
  <baseset+ []>

  $ HGPLAINEXCEPT=revsetalias
  $ export HGPLAINEXCEPT
  $ try 'p2(.)'
  (func
    ('symbol', 'p2')
    ('symbol', '.'))
  (func
    ('symbol', 'p1')
    ('symbol', '.'))
  * set:
  <baseset+ [8]>
  8

  $ unset HGPLAIN
  $ unset HGPLAINEXCEPT

test alias recursion

  $ try sincem
  ('symbol', 'sincem')
  (func
    ('symbol', 'descendants')
    (func
      ('symbol', 'merge')
      None))
  * set:
  <addset+
    <filteredset
      <fullreposet+ 0:9>>,
    <generatorset+>>
  6
  7

test infinite recursion

  $ echo 'recurse1 = recurse2' >> .hg/hgrc
  $ echo 'recurse2 = recurse1' >> .hg/hgrc
  $ try recurse1
  ('symbol', 'recurse1')
  hg: parse error: infinite expansion of revset alias "recurse1" detected
  [255]

  $ echo 'level1($1, $2) = $1 or $2' >> .hg/hgrc
  $ echo 'level2($1, $2) = level1($2, $1)' >> .hg/hgrc
  $ try "level2(level1(1, 2), 3)"
  (func
    ('symbol', 'level2')
    (list
      (func
        ('symbol', 'level1')
        (list
          ('symbol', '1')
          ('symbol', '2')))
      ('symbol', '3')))
  (or
    ('symbol', '3')
    (or
      ('symbol', '1')
      ('symbol', '2')))
  * set:
  <addset
    <baseset [3]>,
    <addset
      <baseset [1]>,
      <baseset [2]>>>
  3
  1
  2

test nesting and variable passing

  $ echo 'nested($1) = nested2($1)' >> .hg/hgrc
  $ echo 'nested2($1) = nested3($1)' >> .hg/hgrc
  $ echo 'nested3($1) = max($1)' >> .hg/hgrc
  $ try 'nested(2:5)'
  (func
    ('symbol', 'nested')
    (range
      ('symbol', '2')
      ('symbol', '5')))
  (func
    ('symbol', 'max')
    (range
      ('symbol', '2')
      ('symbol', '5')))
  * set:
  <baseset [5]>
  5

test variable isolation, variable placeholders are rewritten as string
then parsed and matched again as string. Check they do not leak too
far away.

  $ echo 'injectparamasstring = max("$1")' >> .hg/hgrc
  $ echo 'callinjection($1) = descendants(injectparamasstring)' >> .hg/hgrc
  $ try 'callinjection(2:5)'
  (func
    ('symbol', 'callinjection')
    (range
      ('symbol', '2')
      ('symbol', '5')))
  (func
    ('symbol', 'descendants')
    (func
      ('symbol', 'max')
      ('string', '$1')))
  abort: unknown revision '$1'!
  [255]

  $ echo 'injectparamasstring2 = max(_aliasarg("$1"))' >> .hg/hgrc
  $ echo 'callinjection2($1) = descendants(injectparamasstring2)' >> .hg/hgrc
  $ try 'callinjection2(2:5)'
  (func
    ('symbol', 'callinjection2')
    (range
      ('symbol', '2')
      ('symbol', '5')))
  abort: failed to parse the definition of revset alias "injectparamasstring2": unknown identifier: _aliasarg
  [255]
  $ hg debugrevspec --debug --config revsetalias.anotherbadone='branch(' "tip"
  ('symbol', 'tip')
  warning: failed to parse the definition of revset alias "anotherbadone": at 7: not a prefix: end
  warning: failed to parse the definition of revset alias "injectparamasstring2": unknown identifier: _aliasarg
  * set:
  <baseset [9]>
  9
  >>> data = file('.hg/hgrc', 'rb').read()
  >>> file('.hg/hgrc', 'wb').write(data.replace('_aliasarg', ''))

  $ try 'tip'
  ('symbol', 'tip')
  * set:
  <baseset [9]>
  9

  $ hg debugrevspec --debug --config revsetalias.'bad name'='tip' "tip"
  ('symbol', 'tip')
  warning: failed to parse the declaration of revset alias "bad name": at 4: invalid token
  * set:
  <baseset [9]>
  9
  $ echo 'strictreplacing($1, $10) = $10 or desc("$1")' >> .hg/hgrc
  $ try 'strictreplacing("foo", tip)'
  (func
    ('symbol', 'strictreplacing')
    (list
      ('string', 'foo')
      ('symbol', 'tip')))
  (or
    ('symbol', 'tip')
    (func
      ('symbol', 'desc')
      ('string', '$1')))
  * set:
  <addset
    <baseset [9]>,
    <filteredset
      <filteredset
        <fullreposet+ 0:9>>>>
  9

  $ try 'd(2:5)'
  (func
    ('symbol', 'd')
    (range
      ('symbol', '2')
      ('symbol', '5')))
  (func
    ('symbol', 'reverse')
    (func
      ('symbol', 'sort')
      (list
        (range
          ('symbol', '2')
          ('symbol', '5'))
        ('symbol', 'date'))))
  * set:
  <baseset [4, 5, 3, 2]>
  4
  5
  3
  2
  $ try 'rs(2 or 3, date)'
  (func
    ('symbol', 'rs')
    (list
      (or
        ('symbol', '2')
        ('symbol', '3'))
      ('symbol', 'date')))
  (func
    ('symbol', 'reverse')
    (func
      ('symbol', 'sort')
      (list
        (or
          ('symbol', '2')
          ('symbol', '3'))
        ('symbol', 'date'))))
  * set:
  <baseset [3, 2]>
  3
  2
  $ try 'rs()'
  (func
    ('symbol', 'rs')
    None)
  hg: parse error: invalid number of arguments: 0
  [255]
  $ try 'rs(2)'
  (func
    ('symbol', 'rs')
    ('symbol', '2'))
  hg: parse error: invalid number of arguments: 1
  [255]
  $ try 'rs(2, data, 7)'
  (func
    ('symbol', 'rs')
    (list
      (list
        ('symbol', '2')
        ('symbol', 'data'))
      ('symbol', '7')))
  hg: parse error: invalid number of arguments: 3
  [255]
  $ try 'rs4(2 or 3, x, x, date)'
  (func
    ('symbol', 'rs4')
    (list
      (list
        (list
          (or
            ('symbol', '2')
            ('symbol', '3'))
          ('symbol', 'x'))
        ('symbol', 'x'))
      ('symbol', 'date')))
  (func
    ('symbol', 'reverse')
    (func
      ('symbol', 'sort')
      (list
        (or
          ('symbol', '2')
          ('symbol', '3'))
        ('symbol', 'date'))))
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

  $ log 'limit(1 or 2 or 3, 2) and not 2'
  1
  $ log 'max(1 or 2) and not 2'
  $ log 'min(1 or 2) and not 1'
  $ log 'last(1 or 2, 1) and not 2'

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
  $ log 'remote(".a.b.c.", "../remote3")'

tests for concatenation of strings/symbols by "##"

  $ try "278 ## '5f5' ## 1ee ## 'ce5'"
  (_concat
    (_concat
      (_concat
        ('symbol', '278')
        ('string', '5f5'))
      ('symbol', '1ee'))
    ('string', 'ce5'))
  ('string', '2785f51eece5')
  * set:
  <baseset [0]>
  0

  $ echo 'cat4($1, $2, $3, $4) = $1 ## $2 ## $3 ## $4' >> .hg/hgrc
  $ try "cat4(278, '5f5', 1ee, 'ce5')"
  (func
    ('symbol', 'cat4')
    (list
      (list
        (list
          ('symbol', '278')
          ('string', '5f5'))
        ('symbol', '1ee'))
      ('string', 'ce5')))
  (_concat
    (_concat
      (_concat
        ('symbol', '278')
        ('string', '5f5'))
      ('symbol', '1ee'))
    ('string', 'ce5'))
  ('string', '2785f51eece5')
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

test author/desc/keyword in problematic encoding
# unicode: cp932:
# u30A2    0x83 0x41(= 'A')
# u30C2    0x83 0x61(= 'a')

  $ hg init problematicencoding
  $ cd problematicencoding

  $ python > setup.sh <<EOF
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
  $ python > test.sh <<EOF
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
  0
  ====
  1
  ====
  2
  ====
  3
  ====
  0
  2
  ====
  1
  3

test error message of bad revset
  $ hg log -r 'foo\\'
  hg: parse error at 3: syntax error in revset 'foo\\'
  [255]

  $ cd ..
