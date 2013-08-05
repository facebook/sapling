  $ HGENCODING=utf-8
  $ export HGENCODING

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

  $ hg clone --quiet -U -r 7 . ../remote1
  $ hg clone --quiet -U -r 8 . ../remote2
  $ echo "[paths]" >> .hg/hgrc
  $ echo "default = ../remote1" >> .hg/hgrc

names that should work without quoting

  $ try a
  ('symbol', 'a')
  0
  $ try b-a
  (minus
    ('symbol', 'b')
    ('symbol', 'a'))
  1
  $ try _a_b_c_
  ('symbol', '_a_b_c_')
  6
  $ try _a_b_c_-a
  (minus
    ('symbol', '_a_b_c_')
    ('symbol', 'a'))
  6
  $ try .a.b.c.
  ('symbol', '.a.b.c.')
  7
  $ try .a.b.c.-a
  (minus
    ('symbol', '.a.b.c.')
    ('symbol', 'a'))
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
  9

quoting needed

  $ try '"-a-b-c-"-a'
  (minus
    ('string', '-a-b-c-')
    ('symbol', 'a'))
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
  3
  $ try '1|2&3'
  (or
    ('symbol', '1')
    (and
      ('symbol', '2')
      ('symbol', '3')))
  1
  $ try '1&2&3' # associativity
  (and
    (and
      ('symbol', '1')
      ('symbol', '2'))
    ('symbol', '3'))
  $ try '1|(2|3)'
  (or
    ('symbol', '1')
    (group
      (or
        ('symbol', '2')
        ('symbol', '3'))))
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
  hg: parse error: can't use date here
  [255]
  $ log 'date('
  hg: parse error at 5: not a prefix: end
  [255]
  $ log 'date(tip)'
  abort: invalid date: 'tip'
  [255]
  $ log '"date"'
  abort: unknown revision 'date'!
  [255]
  $ log 'date(2005) and 1::'
  4

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
  $ log 'ancestors(5)'
  0
  1
  3
  5
  $ log 'ancestor(ancestors(5))'
  0
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
  $ try 'grep(r"\bissue\d+")'
  (func
    ('symbol', 'grep')
    ('string', '\\bissue\\d+'))
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
  $ log 'max(contains(a))'
  5
  $ log 'min(contains(a))'
  0
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

we can use patterns when searching for tags

  $ log 'tag("1..*")'
  abort: tag '1..*' does not exist
  [255]
  $ log 'tag("re:1..*")'
  6
  $ log 'tag("re:[0-9].[0-9]")'
  6
  $ log 'tag("literal:1.0")'
  6
  $ log 'tag("re:0..*")'

  $ log 'tag(unknown)'
  abort: tag 'unknown' does not exist
  [255]
  $ log 'branch(unknown)'
  abort: unknown revision 'unknown'!
  [255]
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

aliases:

  $ echo '[revsetalias]' >> .hg/hgrc
  $ echo 'm = merge()' >> .hg/hgrc
  $ echo 'sincem = descendants(m)' >> .hg/hgrc
  $ echo 'd($1) = reverse(sort($1, date))' >> .hg/hgrc
  $ echo 'rs(ARG1, ARG2) = reverse(sort(ARG1, ARG2))' >> .hg/hgrc
  $ echo 'rs4(ARG1, ARGA, ARGB, ARG2) = reverse(sort(ARG1, ARG2))' >> .hg/hgrc

  $ try m
  ('symbol', 'm')
  (func
    ('symbol', 'merge')
    None)
  6

test alias recursion

  $ try sincem
  ('symbol', 'sincem')
  (func
    ('symbol', 'descendants')
    (func
      ('symbol', 'merge')
      None))
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
  hg: parse error: not a function: _aliasarg
  [255]
  >>> data = file('.hg/hgrc', 'rb').read()
  >>> file('.hg/hgrc', 'wb').write(data.replace('_aliasarg', ''))

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
  3
  2

issue2549 - correct optimizations

  $ log 'limit(1 or 2 or 3, 2) and not 2'
  1
  $ log 'max(1 or 2) and not 2'
  $ log 'min(1 or 2) and not 1'
  $ log 'last(1 or 2, 1) and not 2'

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

  $ cd ..
