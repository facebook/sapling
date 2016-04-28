setup

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > blackbox=
  > mock=$TESTDIR/mockblackbox.py
  > EOF

Helper functions:

  $ cacheexists() {
  >   [ -f .hg/cache/tags2-visible ] && echo "tag cache exists" || echo "no tag cache"
  > }

  $ fnodescacheexists() {
  >   [ -f .hg/cache/hgtagsfnodes1 ] && echo "fnodes cache exists" || echo "no fnodes cache"
  > }

  $ dumptags() {
  >     rev=$1
  >     echo "rev $rev: .hgtags:"
  >     hg cat -r$rev .hgtags
  > }

# XXX need to test that the tag cache works when we strip an old head
# and add a new one rooted off non-tip: i.e. node and rev of tip are the
# same, but stuff has changed behind tip.

Setup:

  $ hg init t
  $ cd t
  $ cacheexists
  no tag cache
  $ fnodescacheexists
  no fnodes cache
  $ hg id
  000000000000 tip
  $ cacheexists
  no tag cache
  $ fnodescacheexists
  no fnodes cache
  $ echo a > a
  $ hg add a
  $ hg commit -m "test"
  $ hg co
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg identify
  acb14030fe0a tip
  $ hg identify -r 'wdir()'
  acb14030fe0a tip
  $ cacheexists
  tag cache exists
No fnodes cache because .hgtags file doesn't exist
(this is an implementation detail)
  $ fnodescacheexists
  no fnodes cache

Try corrupting the cache

  $ printf 'a b' > .hg/cache/tags2-visible
  $ hg identify
  acb14030fe0a tip
  $ cacheexists
  tag cache exists
  $ fnodescacheexists
  no fnodes cache
  $ hg identify
  acb14030fe0a tip

Create local tag with long name:

  $ T=`hg identify --debug --id`
  $ hg tag -l "This is a local tag with a really long name!"
  $ hg tags
  tip                                0:acb14030fe0a
  This is a local tag with a really long name!     0:acb14030fe0a
  $ rm .hg/localtags

Create a tag behind hg's back:

  $ echo "$T first" > .hgtags
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 first
  $ hg add .hgtags
  $ hg commit -m "add tags"
  $ hg tags
  tip                                1:b9154636be93
  first                              0:acb14030fe0a
  $ hg identify
  b9154636be93 tip

We should have a fnodes cache now that we have a real tag
The cache should have an empty entry for rev 0 and a valid entry for rev 1.


  $ fnodescacheexists
  fnodes cache exists
  $ f --size --hexdump .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=48
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff b9 15 46 36 26 b7 b4 a7 |..........F6&...|
  0020: 73 e0 9e e3 c5 2f 51 0e 19 e0 5e 1f f9 66 d8 59 |s..../Q...^..f.Y|

Repeat with cold tag cache:

  $ rm -f .hg/cache/tags2-visible .hg/cache/hgtagsfnodes1
  $ hg identify
  b9154636be93 tip

  $ fnodescacheexists
  fnodes cache exists
  $ f --size --hexdump .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=48
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff b9 15 46 36 26 b7 b4 a7 |..........F6&...|
  0020: 73 e0 9e e3 c5 2f 51 0e 19 e0 5e 1f f9 66 d8 59 |s..../Q...^..f.Y|

And again, but now unable to write tag cache or lock file:

#if unix-permissions
  $ rm -f .hg/cache/tags2-visible .hg/cache/hgtagsfnodes1
  $ chmod 555 .hg/cache
  $ hg identify
  b9154636be93 tip
  $ chmod 755 .hg/cache

  $ chmod 555 .hg
  $ hg identify
  b9154636be93 tip
  $ chmod 755 .hg
#endif

Tag cache debug info written to blackbox log

  $ rm -f .hg/cache/tags2-visible .hg/cache/hgtagsfnodes1
  $ hg identify
  b9154636be93 tip
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> identify
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> writing 48 bytes to cache/hgtagsfnodes1
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> 0/1 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> identify exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> blackbox -l 6

Failure to acquire lock results in no write

  $ rm -f .hg/cache/tags2-visible .hg/cache/hgtagsfnodes1
  $ echo 'foo:1' > .hg/wlock
  $ hg identify
  b9154636be93 tip
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> identify
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> not writing .hg/cache/hgtagsfnodes1 because lock cannot be acquired
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> 0/1 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> identify exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @b9154636be938d3d431e75a7c906504a079bfe07 (5000)> blackbox -l 6

  $ fnodescacheexists
  no fnodes cache

  $ rm .hg/wlock

  $ rm -f .hg/cache/tags2-visible .hg/cache/hgtagsfnodes1
  $ hg identify
  b9154636be93 tip

Create a branch:

  $ echo bb > a
  $ hg status
  M a
  $ hg identify
  b9154636be93+ tip
  $ hg co first
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg id
  acb14030fe0a+ first
  $ hg id -r 'wdir()'
  acb14030fe0a+ first
  $ hg -v id
  acb14030fe0a+ first
  $ hg status
  M a
  $ echo 1 > b
  $ hg add b
  $ hg commit -m "branch"
  created new head

Creating a new commit shouldn't append the .hgtags fnodes cache until
tags info is accessed

  $ f --size --hexdump .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=48
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff b9 15 46 36 26 b7 b4 a7 |..........F6&...|
  0020: 73 e0 9e e3 c5 2f 51 0e 19 e0 5e 1f f9 66 d8 59 |s..../Q...^..f.Y|

  $ hg id
  c8edf04160c7 tip

First 4 bytes of record 3 are changeset fragment

  $ f --size --hexdump .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=72
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff b9 15 46 36 26 b7 b4 a7 |..........F6&...|
  0020: 73 e0 9e e3 c5 2f 51 0e 19 e0 5e 1f f9 66 d8 59 |s..../Q...^..f.Y|
  0030: c8 ed f0 41 00 00 00 00 00 00 00 00 00 00 00 00 |...A............|
  0040: 00 00 00 00 00 00 00 00                         |........|

Merge the two heads:

  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg blackbox -l3
  1970/01/01 00:00:00 bob @c8edf04160c7f731e4589d66ab3ab3486a64ac28 (5000)> merge 1
  1970/01/01 00:00:00 bob @c8edf04160c7f731e4589d66ab3ab3486a64ac28+b9154636be938d3d431e75a7c906504a079bfe07 (5000)> merge 1 exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @c8edf04160c7f731e4589d66ab3ab3486a64ac28+b9154636be938d3d431e75a7c906504a079bfe07 (5000)> blackbox -l3
  $ hg id
  c8edf04160c7+b9154636be93+ tip
  $ hg status
  M .hgtags
  $ hg commit -m "merge"

Create a fake head, make sure tag not visible afterwards:

  $ cp .hgtags tags
  $ hg tag last
  $ hg rm .hgtags
  $ hg commit -m "remove"

  $ mv tags .hgtags
  $ hg add .hgtags
  $ hg commit -m "readd"
  $ 
  $ hg tags
  tip                                6:35ff301afafe
  first                              0:acb14030fe0a

Add invalid tags:

  $ echo "spam" >> .hgtags
  $ echo >> .hgtags
  $ echo "foo bar" >> .hgtags
  $ echo "a5a5 invalid" >> .hg/localtags
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 first
  spam
  
  foo bar
  $ hg commit -m "tags"

Report tag parse error on other head:

  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'x y' >> .hgtags
  $ hg commit -m "head"
  created new head

  $ hg tags --debug
  .hgtags@75d9f02dfe28, line 2: cannot parse entry
  .hgtags@75d9f02dfe28, line 4: node 'foo' is not well formed
  .hgtags@c4be69a18c11, line 2: node 'x' is not well formed
  tip                                8:c4be69a18c11e8bc3a5fdbb576017c25f7d84663
  first                              0:acb14030fe0a21b60322c440ad2d20cf7685a376
  $ hg tip
  changeset:   8:c4be69a18c11
  tag:         tip
  parent:      3:ac5e980c4dc0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     head
  

Test tag precedence rules:

  $ cd ..
  $ hg init t2
  $ cd t2
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'      # rev 0
  $ hg tag bar              # rev 1
  $ echo >> foo
  $ hg ci -m 'change foo 1' # rev 2
  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tag -r 1 -f bar      # rev 3
  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo >> foo
  $ hg ci -m 'change foo 2' # rev 4
  created new head
  $ hg tags
  tip                                4:0c192d7d5e6b
  bar                                1:78391a272241

Repeat in case of cache effects:

  $ hg tags
  tip                                4:0c192d7d5e6b
  bar                                1:78391a272241

Detailed dump of tag info:

  $ hg heads -q             # expect 4, 3, 2
  4:0c192d7d5e6b
  3:6fa450212aeb
  2:7a94127795a3
  $ dumptags 2
  rev 2: .hgtags:
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  $ dumptags 3
  rev 3: .hgtags:
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  78391a272241d70354aa14c874552cad6b51bb42 bar
  $ dumptags 4
  rev 4: .hgtags:
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar

Dump cache:

  $ cat .hg/cache/tags2-visible
  4 0c192d7d5e6b78a714de54a2e9627952a877e25a
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  78391a272241d70354aa14c874552cad6b51bb42 bar

  $ f --size --hexdump .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=120
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0020: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0030: 7a 94 12 77 0c 04 f2 a8 af 31 de 17 fa b7 42 28 |z..w.....1....B(|
  0040: 78 ee 5a 2d ad bc 94 3d 6f a4 50 21 7d 3b 71 8c |x.Z-...=o.P!};q.|
  0050: 96 4e f3 7b 89 e5 50 eb da fd 57 89 e7 6c e1 b0 |.N.{..P...W..l..|
  0060: 0c 19 2d 7d 0c 04 f2 a8 af 31 de 17 fa b7 42 28 |..-}.....1....B(|
  0070: 78 ee 5a 2d ad bc 94 3d                         |x.Z-...=|

Corrupt the .hgtags fnodes cache
Extra junk data at the end should get overwritten on next cache update

  $ echo extra >> .hg/cache/hgtagsfnodes1
  $ echo dummy1 > foo
  $ hg commit -m throwaway1

  $ hg tags
  tip                                5:8dbfe60eff30
  bar                                1:78391a272241

  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> tags
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> writing 24 bytes to cache/hgtagsfnodes1
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> 2/3 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @8dbfe60eff306a54259cfe007db9e330e7ecf866 (5000)> blackbox -l 6

#if unix-permissions no-root
Errors writing to .hgtags fnodes cache are silently ignored

  $ echo dummy2 > foo
  $ hg commit -m throwaway2

  $ chmod a-w .hg/cache/hgtagsfnodes1
  $ rm -f .hg/cache/tags2-visible

  $ hg tags
  tip                                6:b968051b5cf3
  bar                                1:78391a272241

  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> tags
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> couldn't write cache/hgtagsfnodes1: [Errno 13] Permission denied: '$TESTTMP/t2/.hg/cache/hgtagsfnodes1'
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> 2/3 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> blackbox -l 6

  $ chmod a+w .hg/cache/hgtagsfnodes1

  $ rm -f .hg/cache/tags2-visible
  $ hg tags
  tip                                6:b968051b5cf3
  bar                                1:78391a272241

  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> tags
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> writing 24 bytes to cache/hgtagsfnodes1
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> 2/3 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @b968051b5cf3f624b771779c6d5f84f1d4c3fb5d (5000)> blackbox -l 6

  $ f --size .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=168

  $ hg -q --config extensions.strip= strip -r 6 --no-backup
#endif

Stripping doesn't truncate the tags cache until new data is available

  $ rm -f .hg/cache/hgtagsfnodes1 .hg/cache/tags2-visible
  $ hg tags
  tip                                5:8dbfe60eff30
  bar                                1:78391a272241

  $ f --size .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=144

  $ hg -q --config extensions.strip= strip -r 5 --no-backup
  $ hg tags
  tip                                4:0c192d7d5e6b
  bar                                1:78391a272241

  $ hg blackbox -l 5
  1970/01/01 00:00:00 bob @0c192d7d5e6b78a714de54a2e9627952a877e25a (5000)> writing 24 bytes to cache/hgtagsfnodes1
  1970/01/01 00:00:00 bob @0c192d7d5e6b78a714de54a2e9627952a877e25a (5000)> 2/3 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @0c192d7d5e6b78a714de54a2e9627952a877e25a (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @0c192d7d5e6b78a714de54a2e9627952a877e25a (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0c192d7d5e6b78a714de54a2e9627952a877e25a (5000)> blackbox -l 5

  $ f --size .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=120

  $ echo dummy > foo
  $ hg commit -m throwaway3

  $ hg tags
  tip                                5:035f65efb448
  bar                                1:78391a272241

  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> tags
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> writing 24 bytes to cache/hgtagsfnodes1
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> 2/3 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @035f65efb448350f4772141702a81ab1df48c465 (5000)> blackbox -l 6
  $ f --size .hg/cache/hgtagsfnodes1
  .hg/cache/hgtagsfnodes1: size=144

  $ hg -q --config extensions.strip= strip -r 5 --no-backup

Test tag removal:

  $ hg tag --remove bar     # rev 5
  $ hg tip -vp
  changeset:   5:5f6e8655b1c7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       .hgtags
  description:
  Removed tag bar
  
  
  diff -r 0c192d7d5e6b -r 5f6e8655b1c7 .hgtags
  --- a/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  +78391a272241d70354aa14c874552cad6b51bb42 bar
  +0000000000000000000000000000000000000000 bar
  
  $ hg tags
  tip                                5:5f6e8655b1c7
  $ hg tags                 # again, try to expose cache bugs
  tip                                5:5f6e8655b1c7

Remove nonexistent tag:

  $ hg tag --remove foobar
  abort: tag 'foobar' does not exist
  [255]
  $ hg tip
  changeset:   5:5f6e8655b1c7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Removed tag bar
  

Undo a tag with rollback:

  $ hg rollback             # destroy rev 5 (restore bar)
  repository tip rolled back to revision 4 (undo commit)
  working directory now based on revision 4
  $ hg tags
  tip                                4:0c192d7d5e6b
  bar                                1:78391a272241
  $ hg tags
  tip                                4:0c192d7d5e6b
  bar                                1:78391a272241

Test tag rank:

  $ cd ..
  $ hg init t3
  $ cd t3
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'       # rev 0
  $ hg tag -f bar            # rev 1 bar -> 0
  $ hg tag -f bar            # rev 2 bar -> 1
  $ hg tag -fr 0 bar         # rev 3 bar -> 0
  $ hg tag -fr 1 bar         # rev 4 bar -> 1
  $ hg tag -fr 0 bar         # rev 5 bar -> 0
  $ hg tags
  tip                                5:85f05169d91d
  bar                                0:bbd179dfa0a7
  $ hg co 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo barbar > foo
  $ hg ci -m 'change foo'    # rev 6
  created new head
  $ hg tags
  tip                                6:735c3ca72986
  bar                                0:bbd179dfa0a7

Don't allow moving tag without -f:

  $ hg tag -r 3 bar
  abort: tag 'bar' already exists (use -f to force)
  [255]
  $ hg tags
  tip                                6:735c3ca72986
  bar                                0:bbd179dfa0a7

Strip 1: expose an old head:

  $ hg --config extensions.mq= strip 5
  saved backup bundle to $TESTTMP/t3/.hg/strip-backup/*-backup.hg (glob)
  $ hg tags                  # partly stale cache
  tip                                5:735c3ca72986
  bar                                1:78391a272241
  $ hg tags                  # up-to-date cache
  tip                                5:735c3ca72986
  bar                                1:78391a272241

Strip 2: destroy whole branch, no old head exposed

  $ hg --config extensions.mq= strip 4
  saved backup bundle to $TESTTMP/t3/.hg/strip-backup/*-backup.hg (glob)
  $ hg tags                  # partly stale
  tip                                4:735c3ca72986
  bar                                0:bbd179dfa0a7
  $ rm -f .hg/cache/tags2-visible
  $ hg tags                  # cold cache
  tip                                4:735c3ca72986
  bar                                0:bbd179dfa0a7

Test tag rank with 3 heads:

  $ cd ..
  $ hg init t4
  $ cd t4
  $ echo foo > foo
  $ hg add
  adding foo
  $ hg ci -m 'add foo'                 # rev 0
  $ hg tag bar                         # rev 1 bar -> 0
  $ hg tag -f bar                      # rev 2 bar -> 1
  $ hg up -qC 0
  $ hg tag -fr 2 bar                   # rev 3 bar -> 2
  $ hg tags
  tip                                3:197c21bbbf2c
  bar                                2:6fa450212aeb
  $ hg up -qC 0
  $ hg tag -m 'retag rev 0' -fr 0 bar  # rev 4 bar -> 0, but bar stays at 2

Bar should still point to rev 2:

  $ hg tags
  tip                                4:3b4b14ed0202
  bar                                2:6fa450212aeb

Test that removing global/local tags does not get confused when trying
to remove a tag of type X which actually only exists as a type Y:

  $ cd ..
  $ hg init t5
  $ cd t5
  $ echo foo > foo
  $ hg add
  adding foo
  $ hg ci -m 'add foo'                 # rev 0

  $ hg tag -r 0 -l localtag
  $ hg tag --remove localtag
  abort: tag 'localtag' is not a global tag
  [255]
  $ 
  $ hg tag -r 0 globaltag
  $ hg tag --remove -l globaltag
  abort: tag 'globaltag' is not a local tag
  [255]
  $ hg tags -v
  tip                                1:a0b6fe111088
  localtag                           0:bbd179dfa0a7 local
  globaltag                          0:bbd179dfa0a7

Test for issue3911

  $ hg tag -r 0 -l localtag2
  $ hg tag -l --remove localtag2
  $ hg tags -v
  tip                                1:a0b6fe111088
  localtag                           0:bbd179dfa0a7 local
  globaltag                          0:bbd179dfa0a7

  $ hg tag -r 1 -f localtag
  $ hg tags -v
  tip                                2:5c70a037bb37
  localtag                           1:a0b6fe111088
  globaltag                          0:bbd179dfa0a7

  $ hg tags -v
  tip                                2:5c70a037bb37
  localtag                           1:a0b6fe111088
  globaltag                          0:bbd179dfa0a7

  $ hg tag -r 1 localtag2
  $ hg tags -v
  tip                                3:bbfb8cd42be2
  localtag2                          1:a0b6fe111088
  localtag                           1:a0b6fe111088
  globaltag                          0:bbd179dfa0a7

  $ hg tags -v
  tip                                3:bbfb8cd42be2
  localtag2                          1:a0b6fe111088
  localtag                           1:a0b6fe111088
  globaltag                          0:bbd179dfa0a7

  $ cd ..

Create a repository with tags data to test .hgtags fnodes transfer

  $ hg init tagsserver
  $ cd tagsserver
  $ cat > .hg/hgrc << EOF
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ touch foo
  $ hg -q commit -A -m initial
  $ hg tag -m 'tag 0.1' 0.1
  $ echo second > foo
  $ hg commit -m second
  $ hg tag -m 'tag 0.2' 0.2
  $ hg tags
  tip                                3:40f0358cb314
  0.2                                2:f63cc8fe54e4
  0.1                                0:96ee1d7354c4
  $ cd ..

Cloning should pull down hgtags fnodes mappings and write the cache file

  $ hg --config experimental.bundle2-exp=True clone --pull tagsserver tagsclient
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Missing tags2* files means the cache wasn't written through the normal mechanism.

  $ ls tagsclient/.hg/cache
  branch2-served
  hgtagsfnodes1
  rbc-names-v1
  rbc-revs-v1

Cache should contain the head only, even though other nodes have tags data

  $ f --size --hexdump tagsclient/.hg/cache/hgtagsfnodes1
  tagsclient/.hg/cache/hgtagsfnodes1: size=96
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0020: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0030: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0040: ff ff ff ff ff ff ff ff 40 f0 35 8c 19 e0 a7 d3 |........@.5.....|
  0050: 8a 5c 6a 82 4d cf fb a5 87 d0 2f a3 1e 4f 2f 8a |.\j.M...../..O/.|

Running hg tags should produce tags2* file and not change cache

  $ hg -R tagsclient tags
  tip                                3:40f0358cb314
  0.2                                2:f63cc8fe54e4
  0.1                                0:96ee1d7354c4

  $ ls tagsclient/.hg/cache
  branch2-served
  hgtagsfnodes1
  rbc-names-v1
  rbc-revs-v1
  tags2-visible

  $ f --size --hexdump tagsclient/.hg/cache/hgtagsfnodes1
  tagsclient/.hg/cache/hgtagsfnodes1: size=96
  0000: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0010: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0020: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0030: ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff |................|
  0040: ff ff ff ff ff ff ff ff 40 f0 35 8c 19 e0 a7 d3 |........@.5.....|
  0050: 8a 5c 6a 82 4d cf fb a5 87 d0 2f a3 1e 4f 2f 8a |.\j.M...../..O/.|

