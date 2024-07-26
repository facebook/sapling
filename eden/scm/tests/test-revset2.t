
#require no-eden


  $ setconfig devel.segmented-changelog-rev-compat=true
  $ enable commitextras
  $ setconfig ui.allowemptycommit=1
  $ setconfig checkout.use-rust=false

  $ HGENCODING=utf-8
  $ export HGENCODING
  $ newext testrevset << EOF
  > import sapling.revset
  > 
  > baseset = sapling.revset.baseset
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
  > sapling.revset.symbols['r3232'] = r3232
  > EOF

  $ try() {
  >   hg debugrevspec --debug "$@"
  > }

  $ log() {
  >   hg log --template '{node}\n' -r "$1"
  > }

  $ commit() {
  >   hg commit "$@"
  > }

extension to build '_intlist()' and '_hexlist()', which is necessary because
these predicates use '\0' as a separator:

  $ cat <<EOF > debugrevlistspec.py
  > from __future__ import absolute_import
  > from sapling import (
  >     node as nodemod,
  >     registrar,
  >     revset,
  >     revsetlang,
  >     smartset,
  > )
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('debugrevlistspec',
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
  $ commit -Aqm0

  $ echo b > b
  $ commit -Aqm1

  $ rm a
  $ commit -Aqm2 -u Bob

  $ hg log -r "extra('branch')" --template '{node}\n'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  925d80f479bb026b0fb3deb27503780b13f74123
  ca0049c702f186378942cfea3da87905eea720a3
  $ hg log -r "extra('branch', 're:a')" --template '{branch}\n'
  default
  default
  default

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ commit -Aqm3

  $ hg co -C 2  # interleave
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bb > b
  $ commit -Aqm4 -d "May 12 2005"

  $ hg co -C 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ commit -Aqm"5 bug"

  $ hg merge 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ commit -Aqm"6 issue619"

  $ commit -Aqm7


  $ hg co 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ commit -Aqm9

  $ hg book -fr 6 1.0
  $ echo "e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0" >> .hgtags
  $ hg add .hgtags
  $ hg commit -Aqm "add 1.0 tag"
  $ hg bookmark -r6 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

  $ hg clone --quiet -U -r 7 . ../remote1
  $ hg clone --quiet -U -r 8 . ../remote2
  $ echo "[paths]" >> .hg/hgrc
  $ echo "default = ../remote1" >> .hg/hgrc

test subtracting something from an addset

  $ log '(outgoing() or removes(a)) - removes(a)'
  6d98e1a8658014ae6501fab0b5e4d1be2d393880
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf

test intersecting something with an addset

  $ log 'parents(outgoing() or removes(a))'
  925d80f479bb026b0fb3deb27503780b13f74123
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  6d98e1a8658014ae6501fab0b5e4d1be2d393880

test that `or` operation:
order is no longer preserved with `idset`, which enforces DESC order internally.

  $ log '3:4 or 2:5'
  ca0049c702f186378942cfea3da87905eea720a3
  8c386d836a07c457eff065d28be13dd131ed35ce
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log '3:4 or 5:2'
  ca0049c702f186378942cfea3da87905eea720a3
  8c386d836a07c457eff065d28be13dd131ed35ce
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log 'sort(3:4 or 2:5)'
  ca0049c702f186378942cfea3da87905eea720a3
  8c386d836a07c457eff065d28be13dd131ed35ce
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log 'sort(3:4 or 5:2)'
  ca0049c702f186378942cfea3da87905eea720a3
  8c386d836a07c457eff065d28be13dd131ed35ce
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae

test that more than one `-r`s are combined in the right order and deduplicated:

  $ hg log -T '{node}\n' -r 3 -r 3 -r 4 -r 5:2 -r 'ancestors(4)'
  8c386d836a07c457eff065d28be13dd131ed35ce
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  ca0049c702f186378942cfea3da87905eea720a3
  925d80f479bb026b0fb3deb27503780b13f74123
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

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
  <nameset-
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:cbd103c35a18fb5ef34e414924807969a3ed80ae+0:5]>>
  5
  4
  3
  2
  1
  0
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
  <nameset+
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:cbd103c35a18fb5ef34e414924807969a3ed80ae+0:5]>>
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
    <nameset+
      <spans [ca0049c702f186378942cfea3da87905eea720a3:8c386d836a07c457eff065d28be13dd131ed35ce+2:3]>>>
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
  <nameset+
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:b1166c9cdb4bf159910e366395bee663c8dfb681+0:6]>>
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
  925d80f479bb026b0fb3deb27503780b13f74123
  ca0049c702f186378942cfea3da87905eea720a3

test that `_list` should remove duplicates

  $ log '0|1|2|1|2|-1|tip'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  925d80f479bb026b0fb3deb27503780b13f74123
  ca0049c702f186378942cfea3da87905eea720a3
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf

test unknown revision in `_list`

  $ log '0|unknown'
  abort: unknown revision 'unknown'!
  [255]

test integer range in `_list`

  $ log '-1|-10'
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

  $ log '-10|-11'
  abort: unknown revision '-11'!
  [255]

  $ log '9|11'
  abort: unknown revision '11'!
  [255]

test '0000' != '0' in `_list`

  $ log '0|0000'
  abort: unknown revision '0000'!
  [255]

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
  <nameset+
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:cbd103c35a18fb5ef34e414924807969a3ed80ae+0:5]>>
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

  $ hg log -T '{node}\n' -r `hg debugsh -c "ui.write('+'.join(['0:1'] * 500))"`
  devel-warn: excess usage of repo.__contains__ at: * (glob)
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  925d80f479bb026b0fb3deb27503780b13f74123

test that repeated `-r` options never eat up stack (issue4565)
(uses `-r 0::1` to avoid possible optimization at old-style parser)

  $ hg log -T '{node}\n' `hg debugsh -c "for i in range(500): ui.write('-r 0::1 '),"`
  devel-warn: excess usage of repo.__contains__ at: * (glob)
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  925d80f479bb026b0fb3deb27503780b13f74123

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
  <nameset+
    <spans [8c386d836a07c457eff065d28be13dd131ed35ce+3]>>
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
  <nameset+
    <spans []>>
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
  <nameset+
    <spans [8c386d836a07c457eff065d28be13dd131ed35ce:b1166c9cdb4bf159910e366395bee663c8dfb681+3:6]>>
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
  <nameset+
    <spans [cbd103c35a18fb5ef34e414924807969a3ed80ae:b1166c9cdb4bf159910e366395bee663c8dfb681+5:6, 8c386d836a07c457eff065d28be13dd131ed35ce+3]>>
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

  $ log 'user(bob)'
  ca0049c702f186378942cfea3da87905eea720a3

  $ log '4::8'
  f97e7d9434783652739570e7cfdac469336f9e24
  6d98e1a8658014ae6501fab0b5e4d1be2d393880
  $ log '4:8'
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  b1166c9cdb4bf159910e366395bee663c8dfb681
  c42d3b18b7b19d997b65800072b703b19989cc12
  6d98e1a8658014ae6501fab0b5e4d1be2d393880

  $ log 'sort(!merge() & (modifies(b) | user(bob) | keyword(bug) | keyword(issue) & 1::9), "-date")'
  f97e7d9434783652739570e7cfdac469336f9e24
  ca0049c702f186378942cfea3da87905eea720a3
  cbd103c35a18fb5ef34e414924807969a3ed80ae

  $ log 'not 0 and 0:2'
  925d80f479bb026b0fb3deb27503780b13f74123
  ca0049c702f186378942cfea3da87905eea720a3
  $ log 'not 1 and 0:2'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  ca0049c702f186378942cfea3da87905eea720a3
  $ log 'not 2 and 0:2'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  925d80f479bb026b0fb3deb27503780b13f74123
  $ log '(1 and 2)::'
  $ log '(1 and 2):'
  $ log '(1 and 2):3'
  $ log 'sort(head(), -rev)'
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf
  c42d3b18b7b19d997b65800072b703b19989cc12
  $ log '4::8 - 8'
  f97e7d9434783652739570e7cfdac469336f9e24

matching() should preserve the order of the input set:

  $ log '(2 or 3 or 1) and matching(1 or 2 or 3)'
  ca0049c702f186378942cfea3da87905eea720a3
  8c386d836a07c457eff065d28be13dd131ed35ce
  925d80f479bb026b0fb3deb27503780b13f74123

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
  8c386d836a07c457eff065d28be13dd131ed35ce
  $ log '4 and p2(6)'
  f97e7d9434783652739570e7cfdac469336f9e24
  $ log '1 and parents(:2)'
  925d80f479bb026b0fb3deb27503780b13f74123
  $ log '2 and children(1:)'
  ca0049c702f186378942cfea3da87905eea720a3
  $ log 'roots(all()) or roots(all())'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  $ hg debugrevspec 'roots(all()) or roots(all())'
  0

issue2654: report a parse error if the revset was not completely parsed

  $ log '1 OR 2'
  hg: parse error at 2: invalid token
  (1 OR 2
     ^ here)
  [255]

or operator should preserve ordering (no longer true with nameset fast paths):
  $ log 'reverse(2::4) or tip'
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf
  f97e7d9434783652739570e7cfdac469336f9e24
  ca0049c702f186378942cfea3da87905eea720a3

parentrevspec

  $ log 'merge()^0'
  b1166c9cdb4bf159910e366395bee663c8dfb681
  $ log 'merge()^'
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log 'merge()^1'
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log 'merge()^2'
  f97e7d9434783652739570e7cfdac469336f9e24
  $ log '(not merge())^2'
  $ log 'merge()^^'
  8c386d836a07c457eff065d28be13dd131ed35ce
  $ log 'merge()^1^'
  8c386d836a07c457eff065d28be13dd131ed35ce
  $ log 'merge()^^^'
  925d80f479bb026b0fb3deb27503780b13f74123

  $ hg debugrevspec -s '(merge() | 0)~-1'
  * set:
  <baseset+ [1, 7]>
  1
  7
  $ log 'merge()~-1'
  c42d3b18b7b19d997b65800072b703b19989cc12
  $ log 'tip~-1'
  $ log '(tip | merge())~-1'
  c42d3b18b7b19d997b65800072b703b19989cc12
  $ log 'merge()~0'
  b1166c9cdb4bf159910e366395bee663c8dfb681
  $ log 'merge()~1'
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log 'merge()~2'
  8c386d836a07c457eff065d28be13dd131ed35ce
  $ log 'merge()~2^1'
  925d80f479bb026b0fb3deb27503780b13f74123
  $ log 'merge()~3'
  925d80f479bb026b0fb3deb27503780b13f74123

  $ log '(-3:tip)^'
  f97e7d9434783652739570e7cfdac469336f9e24
  b1166c9cdb4bf159910e366395bee663c8dfb681
  6d98e1a8658014ae6501fab0b5e4d1be2d393880

  $ log 'tip^foo'
  hg: parse error: ^ expects a number 0, 1, or 2
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
(idset does not preserve '+' order for optimization)

  $ hg log -r 'tip~1:tip' -r 'tip~2:tip~1' --template '{node}\n'
  f97e7d9434783652739570e7cfdac469336f9e24
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  b1166c9cdb4bf159910e366395bee663c8dfb681
  c42d3b18b7b19d997b65800072b703b19989cc12
  6d98e1a8658014ae6501fab0b5e4d1be2d393880
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf

test usage in revpair (with "+")

(real pair)

  $ hg diff -r 'tip^^' -r 'tip'
  diff -r f97e7d943478 -r d57d206a3bab .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0
  $ hg diff -r 'tip^^::tip'
  diff -r f97e7d943478 -r d57d206a3bab .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0

(single rev)

  $ hg diff -r 'tip^' -r 'tip^'
  $ hg diff -r 'tip^:tip^'

(single rev that does not looks like a range)

  $ hg diff -r 'tip^::tip^ or tip^'
  diff -r 6d98e1a86580 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	* (glob)
  @@ -0,0 +1,1 @@
  +e0cc66ef77e8b6f711815af4e001a6594fde3ba5 1.0
  $ hg diff -r 'tip^ or tip^'
  diff -r 6d98e1a86580 .hgtags
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
  <nameset-
    <spans [b1166c9cdb4bf159910e366395bee663c8dfb681+6]>>
  6

  $ HGPLAIN=1
  $ export HGPLAIN
  $ try m
  (symbol 'm')
  abort: unknown revision 'm'!
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
  <nameset-
    <spans [b1166c9cdb4bf159910e366395bee663c8dfb681+6]>>
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
  <nameset+
    <spans [b1166c9cdb4bf159910e366395bee663c8dfb681:c42d3b18b7b19d997b65800072b703b19989cc12+6:7]>>
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
      <fullreposet+
        <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf+0:9]>>,
      <nameset+
        <spans [ca0049c702f186378942cfea3da87905eea720a3:cbd103c35a18fb5ef34e414924807969a3ed80ae+2:5]>>>>
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
  <nameset+
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:8c386d836a07c457eff065d28be13dd131ed35ce+0:3]>>
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
  <nameset-
    <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac+0]>>
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
      <fullreposet+
        <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf+0:9]>>,
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
  hg: parse error: unknown identifier: rs
  [255]
  $ try 'rs(2)'
  (func
    (symbol 'rs')
    (symbol '2'))
  hg: parse error: unknown identifier: rs
  [255]
  $ try 'rs(2, data, 7)'
  (func
    (symbol 'rs')
    (list
      (symbol '2')
      (symbol 'data')
      (symbol '7')))
  hg: parse error: unknown identifier: rs
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

  $ hg log -qr b
  b1166c9cdb4b

  $ hg log -qr b --config revsetalias.b="all()"
  f7b1eb17ad24
  925d80f479bb
  ca0049c702f1
  8c386d836a07
  f97e7d943478
  cbd103c35a18
  b1166c9cdb4b
  c42d3b18b7b1
  6d98e1a86580
  d57d206a3bab

  $ hg log -qr b: --config revsetalias.b="0"
  f7b1eb17ad24
  925d80f479bb
  ca0049c702f1
  8c386d836a07
  f97e7d943478
  cbd103c35a18
  b1166c9cdb4b
  c42d3b18b7b1
  6d98e1a86580
  d57d206a3bab

  $ hg log -qr :b --config revsetalias.b="9"
  f7b1eb17ad24
  925d80f479bb
  ca0049c702f1
  8c386d836a07
  f97e7d943478
  cbd103c35a18
  b1166c9cdb4b
  c42d3b18b7b1
  6d98e1a86580
  d57d206a3bab

  $ hg log -qr b:
  b1166c9cdb4b
  c42d3b18b7b1
  6d98e1a86580
  d57d206a3bab

  $ hg log -qr :b
  f7b1eb17ad24
  925d80f479bb
  ca0049c702f1
  8c386d836a07
  f97e7d943478
  cbd103c35a18
  b1166c9cdb4b

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
        <fullreposet+
          <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf+0:9]>>,
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
        <fullreposet+
          <spans [f7b1eb17ad24730a1651fccd46c43826d1bbc2ac:d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf+0:9]>>,
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
  8c386d836a07
  ca0049c702f1

test revsets started with 40-chars hash (issue3669)

  $ ISSUE3669_TIP=`hg tip --template '{node}'`
  $ hg log -r "${ISSUE3669_TIP}" --template '{node}\n'
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf
  $ hg log -r "${ISSUE3669_TIP}^" --template '{node}\n'
  6d98e1a8658014ae6501fab0b5e4d1be2d393880

test or-ed indirect predicates (issue3775)

  $ log '6 or 6^1' | sort
  b1166c9cdb4bf159910e366395bee663c8dfb681
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log '6^1 or 6' | sort
  b1166c9cdb4bf159910e366395bee663c8dfb681
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  $ log '4 or 4~1' | sort
  ca0049c702f186378942cfea3da87905eea720a3
  f97e7d9434783652739570e7cfdac469336f9e24
  $ log '4~1 or 4' | sort
  ca0049c702f186378942cfea3da87905eea720a3
  f97e7d9434783652739570e7cfdac469336f9e24
  $ log '(0 or 2):(4 or 6) or 0 or 6' | sort
  8c386d836a07c457eff065d28be13dd131ed35ce
  925d80f479bb026b0fb3deb27503780b13f74123
  b1166c9cdb4bf159910e366395bee663c8dfb681
  ca0049c702f186378942cfea3da87905eea720a3
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  f97e7d9434783652739570e7cfdac469336f9e24
  $ log '0 or 6 or (0 or 2):(4 or 6)' | sort
  8c386d836a07c457eff065d28be13dd131ed35ce
  925d80f479bb026b0fb3deb27503780b13f74123
  b1166c9cdb4bf159910e366395bee663c8dfb681
  ca0049c702f186378942cfea3da87905eea720a3
  cbd103c35a18fb5ef34e414924807969a3ed80ae
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  f97e7d9434783652739570e7cfdac469336f9e24

tests for 'remote()' predicate:
#.  (csets in remote) (id)            (remote)
1.  less than local   current branch  "default"
2.  same with local   specified       "default"
3.  more than local   specified       specified

  $ hg clone --quiet -U . ../remote3
  $ hg book -r 7 ".a.b.c."
  $ hg book -r 2 "a-b-c-"
  $ cd ../remote3
  $ hg goto -q 7
  $ echo r > r
  $ hg ci -Aqm 10
  $ log 'remote()'
  d57d206a3bab91a65cbc8c7fb6c7de5a235c6bcf
  $ log 'remote("a-b-c-")'
  ca0049c702f186378942cfea3da87905eea720a3
  $ cd ../repo

tests for concatenation of strings/symbols by "##"
  $ try "f7b ## '1eb' ## 17a ## 'd24'"
  (_concat
    (_concat
      (_concat
        (symbol 'f7b')
        (string '1eb'))
      (symbol '17a'))
    (string 'd24'))
  * concatenated:
  (string 'f7b1eb17ad24')
  * set:
  <baseset [0]>
  0

  $ echo 'cat4($1, $2, $3, $4) = $1 ## $2 ## $3 ## $4' >> .hg/hgrc
  $ try "cat4(f7b, '1eb', 17a, 'd24')"
  (func
    (symbol 'cat4')
    (list
      (symbol 'f7b')
      (string '1eb')
      (symbol '17a')
      (string 'd24')))
  * expanded:
  (_concat
    (_concat
      (_concat
        (symbol 'f7b')
        (string '1eb'))
      (symbol '17a'))
    (string 'd24'))
  * concatenated:
  (string 'f7b1eb17ad24')
  * set:
  <baseset [0]>
  0

(check concatenation in alias nesting)

  $ echo 'cat2($1, $2) = $1 ## $2' >> .hg/hgrc
  $ echo 'cat2x2($1, $2, $3, $4) = cat2($1 ## $2, $3 ## $4)' >> .hg/hgrc
  $ log "cat2x2(f7b, '1eb', 17a, 'd24')"
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

(check operator priority)

  $ echo 'cat2n2($1, $2, $3, $4) = $1 ## $2 or $3 ## $4~2' >> .hg/hgrc
  $ log "cat2n2(f7b1eb, 17ad24, d57d20, 6a3bab)"
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  f97e7d9434783652739570e7cfdac469336f9e24

  $ cd ..

prepare repository that has "default" branches of multiple roots

  $ hg init namedbranch
  $ cd namedbranch

  $ echo default0 >> a
  $ hg ci -Aqm0
  $ echo default1 >> a
  $ hg ci -m1

  $ echo stable2 >> a
  $ commit -m2
  $ echo stable3 >> a
  $ commit -m3

  $ hg goto -q null
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
  > print(u'''
  > echo a > text
  > hg add text
  > hg --encoding utf-8 commit -u '\\\u30A2' -m none
  > echo b > text
  > hg --encoding utf-8 commit -u '\\\u30C2' -m none
  > echo c > text
  > hg --encoding utf-8 commit -u none -m '\\\u30A2'
  > echo d > text
  > hg --encoding utf-8 commit -u none -m '\\\u30C2'
  > ''')
  > EOF
  $ sh < setup.sh

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
  > from sapling import error, registrar, revset
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
  warning: extension custompredicate is disabled because it cannot be imported from $TESTTMP/custompredicate.py: intentional failure of loading extension
  hg: parse error: unknown identifier: custom1
  [255]

Test repo.anyrevs with customized revset overrides

  $ cat > $TESTTMP/printprevset.py <<EOF
  > from sapling import encoding, registrar
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

Test mutation related revsets

  $ hg init repo1
  $ cd repo1

  $ drawdag <<'EOS'
  > H      F G
  > |      |/    # split: B -> E, F
  > B C D  E     # amend: B -> C -> D
  >  \|/   |     # amend: F -> G
  >   A    A  Z  # amend: A -> Z
  > EOS

 (all() revset does not show hidden (obsoleted) commits)
  $ hg log -G -r "all()" -T '{desc}\n'
  o  G
  │
  │ o  D
  │ │
  │ │ o  H
  │ │ │
  o │ │  E
  ├─╯ │
  │ o │  Z
  │   │
  │   x  B
  ├───╯
  x  A
  

  $ hg log -r "successors($Z)" -T '{desc}\n'
  Z

  $ hg log -r "successors($F)" -T '{desc}\n' --hidden
  D
  F
  G

  $ hg log -r "predecessors($Z)" -T '{desc}\n'
  A
  Z

  $ hg log -r "predecessors($A)" -T '{desc}\n'
  A

 (hidden commits like C and F are not shown)
  $ hg log -r "successors($B)" -T '{desc}\n'
  B
  E
  D
  G

 (hidden commits like C and F are shown with --hidden)
  $ hg log -r "successors($B)" -T '{desc}\n' --hidden
  B
  C
  E
  D
  F
  G

  $ hg log -r "successors($B,1)" -T '{desc}\n' --hidden
  B
  C
  E
  F

  $ hg log -r "predecessors($D)" -T '{desc}\n'
  B
  C
  D
  F

  $ hg log -r "predecessors($D)" -T '{desc}\n' --hidden
  B
  C
  D
  F

  $ hg log -r "predecessors($D,1)" -T '{desc}\n' --hidden
  C
  D
  F

  $ hg log -r "successors($B)-obsolete()" -T '{desc}\n' --hidden
  E
  D
  G

Test `draft() & ::x` is not optimized to _phaseandancestors:

  $ hg init $TESTTMP/repo2
  $ cd $TESTTMP/repo2
  $ hg debugdrawdag <<'EOS'
  >   P5 D2
  >    |  |
  >    : D1
  >    |/
  >   P1
  >    |
  >   P0
  > EOS
  $ hg debugmakepublic -r P5
  $ hg debugrevspec --verify -p analyzed -p optimized 'draft() & ::(D1+P5)'
  * analyzed:
  (and
    (func
      (symbol 'draft')
      None)
    (func
      (symbol 'ancestors')
      (or
        (list
          (symbol 'D1')
          (symbol 'P5')))))
  * optimized:
  (and
    (func
      (symbol 'draft')
      None)
    (func
      (symbol 'ancestors')
      (func
        (symbol '_list')
        (string 'D1\x00P5'))))
