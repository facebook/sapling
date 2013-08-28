Helper functions:

  $ cacheexists() {
  >   [ -f .hg/cache/tags ] && echo "tag cache exists" || echo "no tag cache"
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
  $ hg id
  000000000000 tip
  $ cacheexists
  no tag cache
  $ echo a > a
  $ hg add a
  $ hg commit -m "test"
  $ hg co
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg identify
  acb14030fe0a tip
  $ cacheexists
  tag cache exists

Try corrupting the cache

  $ printf 'a b' > .hg/cache/tags
  $ hg identify
  .hg/cache/tags is corrupt, rebuilding it
  acb14030fe0a tip
  $ cacheexists
  tag cache exists
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

Repeat with cold tag cache:

  $ rm -f .hg/cache/tags
  $ hg identify
  b9154636be93 tip

And again, but now unable to write tag cache:

#if unix-permissions
  $ rm -f .hg/cache/tags
  $ chmod 555 .hg
  $ hg identify
  b9154636be93 tip
  $ chmod 755 .hg
#endif

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
  $ hg -v id
  acb14030fe0a+ first
  $ hg status
  M a
  $ echo 1 > b
  $ hg add b
  $ hg commit -m "branch"
  created new head
  $ hg id
  c8edf04160c7 tip

Merge the two heads:

  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
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

  $ hg tags
  .hgtags@75d9f02dfe28, line 2: cannot parse entry
  .hgtags@75d9f02dfe28, line 4: node 'foo' is not well formed
  .hgtags@c4be69a18c11, line 2: node 'x' is not well formed
  tip                                8:c4be69a18c11
  first                              0:acb14030fe0a
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

  $ cat .hg/cache/tags
  4 0c192d7d5e6b78a714de54a2e9627952a877e25a 0c04f2a8af31de17fab7422878ee5a2dadbc943d
  3 6fa450212aeb2a21ed616a54aea39a4a27894cd7 7d3b718c964ef37b89e550ebdafd5789e76ce1b0
  2 7a94127795a33c10a370c93f731fd9fea0b79af6 0c04f2a8af31de17fab7422878ee5a2dadbc943d
  
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  bbd179dfa0a71671c253b3ae0aa1513b60d199fa bar
  78391a272241d70354aa14c874552cad6b51bb42 bar

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
  $ rm -f .hg/cache/tags
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
