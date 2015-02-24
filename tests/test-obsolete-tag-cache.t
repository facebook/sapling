  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > 
  > [experimental]
  > evolution = createmarkers
  > EOF

Create a repo with some tags

  $ hg init repo
  $ cd repo
  $ echo initial > foo
  $ hg -q commit -A -m initial
  $ hg tag -m 'test tag' test1
  $ echo first > first
  $ hg -q commit -A -m first
  $ hg tag -m 'test2 tag' test2
  $ hg -q up -r 0
  $ echo newhead > newhead
  $ hg commit -A -m newhead
  adding newhead
  created new head

Trigger tags cache population by doing something that accesses tags info

  $ hg log -G -T '{rev}:{node|short} {tags} {desc}\n'
  @  4:042eb6bfcc49 tip newhead
  |
  | o  3:c3cb30f2d2cd  test2 tag
  | |
  | o  2:d75775ffbc6b test2 first
  | |
  | o  1:5f97d42da03f  test tag
  |/
  o  0:55482a6fb4b1 test1 initial
  

  $ cat .hg/cache/tags
  4 042eb6bfcc4909bad84a1cbf6eb1ddf0ab587d41
  3 c3cb30f2d2cd0aae008cc91a07876e3c5131fd22 b3bce87817fe7ac9dca2834366c1d7534c095cf1
  
  55482a6fb4b1881fa8f746fd52cf6f096bb21c89 test1
  d75775ffbc6bca1794d300f5571272879bd280da test2

Create some hidden changesets via a rebase and trigger tags cache
repopulation

  $ hg -q rebase -s 1 -d 4
  $ hg log -G -T '{rev}:{node|short} {tags} {desc}\n'
  o  7:eb610439e10e tip test2 tag
  |
  o  6:7b4af00c3c83  first
  |
  o  5:43ac2a539b3c  test tag
  |
  @  4:042eb6bfcc49  newhead
  |
  o  0:55482a6fb4b1 test1 initial
  

.hgtags filenodes for hidden heads should be visible (issue4550)
(currently broken)

  $ cat .hg/cache/tags
  7 eb610439e10e0c6b296f97b59624c2e24fc59e30 b3bce87817fe7ac9dca2834366c1d7534c095cf1
  
  55482a6fb4b1881fa8f746fd52cf6f096bb21c89 test1
  d75775ffbc6bca1794d300f5571272879bd280da test2

