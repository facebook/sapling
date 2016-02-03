  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > blackbox=
  > rebase=
  > mock=$TESTDIR/mockblackbox.py
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
  $ hg tag -m 'test head 2 tag' head2

  $ hg log -G -T '{rev}:{node|short} {tags} {desc}\n'
  @  5:2942a772f72a tip test head 2 tag
  |
  o  4:042eb6bfcc49 head2 newhead
  |
  | o  3:c3cb30f2d2cd  test2 tag
  | |
  | o  2:d75775ffbc6b test2 first
  | |
  | o  1:5f97d42da03f  test tag
  |/
  o  0:55482a6fb4b1 test1 initial
  

Trigger tags cache population by doing something that accesses tags info

  $ hg tags
  tip                                5:2942a772f72a
  head2                              4:042eb6bfcc49
  test2                              2:d75775ffbc6b
  test1                              0:55482a6fb4b1

  $ cat .hg/cache/tags2-visible
  5 2942a772f72a444bef4bef13874d515f50fa27b6
  042eb6bfcc4909bad84a1cbf6eb1ddf0ab587d41 head2
  55482a6fb4b1881fa8f746fd52cf6f096bb21c89 test1
  d75775ffbc6bca1794d300f5571272879bd280da test2

Hiding a non-tip changeset should change filtered hash and cause tags recompute

  $ hg debugobsolete -d '0 0' c3cb30f2d2cd0aae008cc91a07876e3c5131fd22 -u dummyuser

  $ hg tags
  tip                                5:2942a772f72a
  head2                              4:042eb6bfcc49
  test1                              0:55482a6fb4b1

  $ cat .hg/cache/tags2-visible
  5 2942a772f72a444bef4bef13874d515f50fa27b6 f34fbc9a9769ba9eff5aff3d008a6b49f85c08b1
  042eb6bfcc4909bad84a1cbf6eb1ddf0ab587d41 head2
  55482a6fb4b1881fa8f746fd52cf6f096bb21c89 test1

  $ hg blackbox -l 5
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> 2/2 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> writing .hg/cache/tags2-visible with 2 tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> blackbox -l 5

Hiding another changeset should cause the filtered hash to change

  $ hg debugobsolete -d '0 0' d75775ffbc6bca1794d300f5571272879bd280da -u dummyuser
  $ hg debugobsolete -d '0 0' 5f97d42da03fd56f3b228b03dfe48af5c0adf75b -u dummyuser

  $ hg tags
  tip                                5:2942a772f72a
  head2                              4:042eb6bfcc49

  $ cat .hg/cache/tags2-visible
  5 2942a772f72a444bef4bef13874d515f50fa27b6 2fce1eec33263d08a4d04293960fc73a555230e4
  042eb6bfcc4909bad84a1cbf6eb1ddf0ab587d41 head2

  $ hg blackbox -l 5
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> 1/1 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> writing .hg/cache/tags2-visible with 1 tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> blackbox -l 5

Resolving tags on an unfiltered repo writes a separate tags cache

  $ hg --hidden tags
  tip                                5:2942a772f72a
  head2                              4:042eb6bfcc49
  test2                              2:d75775ffbc6b
  test1                              0:55482a6fb4b1

  $ cat .hg/cache/tags2
  5 2942a772f72a444bef4bef13874d515f50fa27b6
  042eb6bfcc4909bad84a1cbf6eb1ddf0ab587d41 head2
  55482a6fb4b1881fa8f746fd52cf6f096bb21c89 test1
  d75775ffbc6bca1794d300f5571272879bd280da test2

  $ hg blackbox -l 5
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> --hidden tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> 2/2 cache hits/lookups in * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> writing .hg/cache/tags2 with 3 tags
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> --hidden tags exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @2942a772f72a444bef4bef13874d515f50fa27b6 (5000)> blackbox -l 5
