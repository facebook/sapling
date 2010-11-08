  $ HGENCODING=utf-8
  $ export HGENCODING

  $ try() {
  >   hg debugrevspec --debug $@
  > }

  $ log() {
  >   hg log --template '{rev}\n' -r "$1"
  > }

  $ hg init repo
  $ cd repo

  $ echo a > a
  $ hg branch a
  marked working directory as branch a
  $ hg ci -Aqm0

  $ echo b > b
  $ hg branch b
  marked working directory as branch b
  $ hg ci -Aqm1

  $ rm a
  $ hg branch a-b-c-
  marked working directory as branch a-b-c-
  $ hg ci -Aqm2 -u Bob

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
  $ hg branch /a/b/c/
  marked working directory as branch /a/b/c/
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
  $ hg ci --close-branch -Aqm8
  abort: can only close branch heads
  [255]

  $ hg co 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch é
  marked working directory as branch \xc3\xa9 (esc)
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
  ('minus', ('symbol', 'b'), ('symbol', 'a'))
  1
  $ try _a_b_c_
  ('symbol', '_a_b_c_')
  6
  $ try _a_b_c_-a
  ('minus', ('symbol', '_a_b_c_'), ('symbol', 'a'))
  6
  $ try .a.b.c.
  ('symbol', '.a.b.c.')
  7
  $ try .a.b.c.-a
  ('minus', ('symbol', '.a.b.c.'), ('symbol', 'a'))
  7
  $ try -- '-a-b-c-' # complains
  hg: parse error at 7: not a prefix: end
  [255]
  $ log -a-b-c- # succeeds with fallback
  4
  $ try -- -a-b-c--a # complains
  ('minus', ('minus', ('minus', ('negate', ('symbol', 'a')), ('symbol', 'b')), ('symbol', 'c')), ('negate', ('symbol', 'a')))
  abort: unknown revision '-a'!
  [255]
  $ try é
  ('symbol', '\xc3\xa9')
  9

quoting needed

  $ try '"-a-b-c-"-a'
  ('minus', ('string', '-a-b-c-'), ('symbol', 'a'))
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
  ('or', ('and', ('symbol', '1'), ('symbol', '2')), ('symbol', '3'))
  3
  $ try '1|2&3'
  ('or', ('symbol', '1'), ('and', ('symbol', '2'), ('symbol', '3')))
  1
  $ try '1&2&3' # associativity
  ('and', ('and', ('symbol', '1'), ('symbol', '2')), ('symbol', '3'))
  $ try '1|(2|3)'
  ('or', ('symbol', '1'), ('group', ('or', ('symbol', '2'), ('symbol', '3'))))
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

  $ log 'ancestor(1)'
  hg: parse error: ancestor requires two arguments
  [255]
  $ log 'ancestor(4,5)'
  1
  $ log 'ancestor(4,5) and 4'
  $ log 'ancestors(5)'
  0
  1
  3
  5
  $ log 'author(bob)'
  2
  $ log 'branch(é)'
  8
  9
  $ log 'children(ancestor(4,5))'
  2
  3
  $ log 'closed()'
  $ log 'contains(a)'
  0
  1
  3
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
  $ log 'file(b)'
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
  ('func', ('symbol', 'grep'), ('string', '('))
  hg: parse error: invalid match pattern: unbalanced parenthesis
  [255]
  $ try 'grep("\bissue\d+")'
  ('func', ('symbol', 'grep'), ('string', '\x08issue\\d+'))
  $ try 'grep(r"\bissue\d+")'
  ('func', ('symbol', 'grep'), ('string', '\\bissue\\d+'))
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
  $ log 'limit(head(), 1)'
  0
  $ log 'max(contains(a))'
  5
  $ log 'min(contains(a))'
  0
  $ log 'merge()'
  6
  $ log 'modifies(b)'
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
  $ log 'heads(branch(é)) or heads(branch(é))'
  9
  $ log 'ancestors(8) and (heads(branch("-a-b-c-")) or heads(branch(é)))'
  4
