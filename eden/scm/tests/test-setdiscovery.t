#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ setconfig experimental.allowfilepeer=True

Stabilize test

  $ PYTHONHASHSEED=0

Function to test discovery between two repos in both directions, using both the local shortcut
(which is currently not activated by default) and the full remotable protocol:

  $ testdesc() { # revs_a, revs_b, dagdesc
  >     if [ -d foo ]; then rm -rf foo; fi
  >     hg init foo
  >     cd foo
  >     hg debugbuilddag "$3"
  >     hg clone . a $1 --quiet
  >     hg clone . b $2 --quiet
  >     echo
  >     echo "% -- a -> b set"
  >     hg -R a debugdiscovery b --verbose --debug --config progress.debug=true
  >     echo
  >     echo "% -- a -> b set (tip only)"
  >     hg -R a debugdiscovery b --verbose --debug --config progress.debug=true --rev tip
  >     echo
  >     echo "% -- b -> a set"
  >     hg -R b debugdiscovery a --verbose --debug --config progress.debug=true
  >     echo
  >     echo "% -- b -> a set (tip only)"
  >     hg -R b debugdiscovery a --verbose --debug --config progress.debug=true --rev tip
  >     cd ..
  > }


Small superset:

  $ testdesc '-ra1 -ra2' '-rb1 -rb2 -rb3' '
  > +2:f +1:a1:b1
  > <f +4 :a2
  > +5 :b2
  > <f +3 :b3'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 2; remote heads: 3 (explicit: 0); initial common: 1
  all local heads known remotely
  common heads: 01241442b3c2 b5714e113bc0
  local is subset
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 3 (explicit: 0); initial common: 1
  all local heads known remotely
  common heads: b5714e113bc0
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 3; remote heads: 2 (explicit: 0); initial common: 2
  sampling from both directions (4 of 4)
  sampling undecided commits (6 of 6)
  progress: searching: checking 6 commits, 0 left 2 queries
  query 2; still undecided: 6, sample size is: 6
  progress: searching (end)
  2 total queries in 0.0000s
  common heads: 01241442b3c2 b5714e113bc0
  remote is subset
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 2 (explicit: 0); initial common: 2
  sampling from both directions (2 of 2)
  sampling undecided commits (2 of 2)
  progress: searching: checking 2 commits, 0 left 2 queries
  query 2; still undecided: 2, sample size is: 2
  progress: searching (end)
  2 total queries in 0.0000s
  common heads: 01241442b3c2 b5714e113bc0
  remote is subset


Many new:

  $ testdesc '-ra1 -ra2' '-rb' '
  > +2:f +3:a1 +3:b
  > <f +30 :a2'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 2; remote heads: 1 (explicit: 0); initial common: 0
  sampling from both directions (2 of 2)
  sampling undecided commits (29 of 29)
  progress: searching: checking 29 commits, 0 left 2 queries
  query 2; still undecided: 29, sample size is: 29
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: bebd167eb94d
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 2 (explicit: 0); initial common: 1
  sampling from both directions (2 of 2)
  sampling undecided commits (2 of 2)
  progress: searching: checking 2 commits, 0 left 2 queries
  query 2; still undecided: 2, sample size is: 2
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: bebd167eb94d
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 2 (explicit: 0); initial common: 1
  sampling from both directions (2 of 2)
  sampling undecided commits (2 of 2)
  progress: searching: checking 2 commits, 0 left 2 queries
  query 2; still undecided: 2, sample size is: 2
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: bebd167eb94d

Both sides many new with stub:

  $ testdesc '-ra1 -ra2' '-rb' '
  > +2:f +2:a1 +30 :b
  > <f +30 :a2'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 2; remote heads: 1 (explicit: 0); initial common: 0
  sampling from both directions (2 of 2)
  sampling undecided commits (29 of 29)
  progress: searching: checking 29 commits, 0 left 2 queries
  query 2; still undecided: 29, sample size is: 29
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 2dc09a01254d
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 2 (explicit: 0); initial common: 1
  sampling from both directions (2 of 2)
  sampling undecided commits (29 of 29)
  progress: searching: checking 29 commits, 0 left 2 queries
  query 2; still undecided: 29, sample size is: 29
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 2dc09a01254d
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 2 (explicit: 0); initial common: 1
  sampling from both directions (2 of 2)
  sampling undecided commits (29 of 29)
  progress: searching: checking 29 commits, 0 left 2 queries
  query 2; still undecided: 29, sample size is: 29
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 2dc09a01254d


Both many new:

  $ testdesc '-ra' '-rb' '
  > +2:f +30 :b
  > <f +30 :a'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b


Both many new skewed:

  $ testdesc '-ra' '-rb' '
  > +2:f +30 :b
  > <f +50 :a'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (51 of 51)
  progress: searching: checking 51 commits, 0 left 2 queries
  query 2; still undecided: 51, sample size is: 51
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (51 of 51)
  progress: searching: checking 51 commits, 0 left 2 queries
  query 2; still undecided: 51, sample size is: 51
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (31 of 31)
  progress: searching: checking 31 commits, 0 left 2 queries
  query 2; still undecided: 31, sample size is: 31
  progress: searching (end)
  2 total queries in *.????s (glob)
  common heads: 66f7d451a68b


Both many new on top of long history:

  $ testdesc '-ra' '-rb' '
  > +1000:f +30 :b
  > <f +50 :a'
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (200 of 1049)
  progress: searching: checking 200 commits, 849 left 2 queries
  query 2; still undecided: 1049, sample size is: 200
  sampling from both directions (2 of 2)
  sampling undecided commits (8 of 8)
  progress: searching: checking 8 commits, 0 left 3 queries
  query 3; still undecided: 8, sample size is: 8
  progress: searching (end)
  3 total queries in *.????s (glob)
  common heads: 7ead0cba2838
  
  % -- a -> b set (tip only)
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (200 of 1049)
  progress: searching: checking 200 commits, 849 left 2 queries
  query 2; still undecided: 1049, sample size is: 200
  sampling from both directions (2 of 2)
  sampling undecided commits (8 of 8)
  progress: searching: checking 8 commits, 0 left 3 queries
  query 3; still undecided: 8, sample size is: 8
  progress: searching (end)
  3 total queries in *.????s (glob)
  common heads: 7ead0cba2838
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (200 of 1029)
  progress: searching: checking 200 commits, 829 left 2 queries
  query 2; still undecided: 1029, sample size is: 200
  sampling from both directions (2 of 2)
  sampling undecided commits (14 of 14)
  progress: searching: checking 14 commits, 0 left 3 queries
  query 3; still undecided: 14, sample size is: 14
  progress: searching (end)
  3 total queries in *.????s (glob)
  common heads: 7ead0cba2838
  
  % -- b -> a set (tip only)
  comparing with a
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 0
  sampling undecided commits (200 of 1029)
  progress: searching: checking 200 commits, 829 left 2 queries
  query 2; still undecided: 1029, sample size is: 200
  sampling from both directions (2 of 2)
  sampling undecided commits (14 of 14)
  progress: searching: checking 14 commits, 0 left 3 queries
  query 3; still undecided: 14, sample size is: 14
  progress: searching (end)
  3 total queries in *.????s (glob)
  common heads: 7ead0cba2838


One with >200 heads, which used to use up all of the sample:

  $ hg init manyheads
  $ cd manyheads
  $ echo "+300:r @a" >dagdesc
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3 *r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3*r+3" >>dagdesc # 20 heads
  $ echo "@b *r+3" >>dagdesc # one more head
  $ hg debugbuilddag <dagdesc
  reading DAG from stdin

  $ hg heads -t --template . | wc -c
  \s*261 (re)

The graph looks like:

>  b a  (b is tip)
>  | |
>  b a
>  | |
>  b a
>  | |
>  b a
>  |/
>  :   (many other a-a-a-a branches)
>  | a
>  | |
>  | a
>  | |
>  | a
>  | |
>  | a
>  |/
>  |
>  default  (is also 'ancestor(head())')
>  |
>  default
>  |
>  :  (many other default commits)

  $ hg clone . a -q
  $ hg -R a up -q 'ancestor(head())'
  $ hg clone -r tip . b
  adding changesets
  adding manifests
  adding file changes
  updating to branch b
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R a debugdiscovery b --debug --verbose --config progress.debug=true
  comparing with b
  query 1; heads
  searching for changes
  local heads: 261; remote heads: 1 (explicit: 0); initial common: 1
  sampling from both directions (200 of 520)
  progress: searching: checking 200 commits, 741 left 2 queries
  query 2; still undecided: 941, sample size is: 200
  sampling from both directions (200 of 270)
  progress: searching: checking 200 commits, 257 left 3 queries
  query 3; still undecided: 457, sample size is: 200
  sampling from both directions (60 of 60)
  sampling undecided commits (84 of 84)
  progress: searching: checking 84 commits, 0 left 4 queries
  query 4; still undecided: 84, sample size is: 84
  progress: searching (end)
  4 total queries in 0.0000s
  common heads: 513314ca8b3a
  remote is subset
  $ hg -R a debugdiscovery b --debug --verbose --config progress.debug=true --rev tip
  comparing with b
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  all local heads known remotely
  common heads: 513314ca8b3a
  remote is subset

Test actual protocol when pulling one new head in addition to common heads

  $ hg clone -U b c
  $ hg -R c id -ir tip
  513314ca8b3a
  $ hg -R c up -qr default
  $ touch c/f
  $ hg -R c ci -Aqm "extra head"
  $ hg -R c id -i
  ecb0e2f146b3

  $ cd b
  $ configure dummyssh
  $ cd ../..


Issue 4438 - test coverage for 3ef893520a85 issues.

  $ mkdir issue4438
  $ cd issue4438
#if false
generate new bundles:
  $ hg init r1
  $ for i in `seq 101`; do hg -R r1 up -qr null && hg -R r1 branch -q b$i && hg -R r1 ci -qmb$i; done
  $ hg clone -q r1 r2
  $ for i in `seq 10`; do hg -R r1 up -qr null && hg -R r1 branch -q c$i && hg -R r1 ci -qmc$i; done
  $ hg -R r2 branch -q r2change && hg -R r2 ci -qmr2change
  $ hg -R r1 bundle -qa $TESTDIR/bundles/issue4438-r1.hg
  $ hg -R r2 bundle -qa $TESTDIR/bundles/issue4438-r2.hg
#else
use existing bundles:
  $ hg clone -q $TESTDIR/bundles/issue4438-r1.hg r1
  $ hg clone -q $TESTDIR/bundles/issue4438-r2.hg r2
#endif

Set iteration order could cause wrong and unstable results - fixed in 73cfaa348650:

The case where all the 'initialsamplesize' samples already were common would
give 'all remote heads known locally' without checking the remaining heads -
fixed in 86c35b7ae300:

  $ cat >> $TESTTMP/unrandomsample.py << EOF
  > import random
  > def sample(population, k):
  >     return sorted(population)[:k]
  > random.sample = sample
  > EOF

  $ cat >> r1/.hg/hgrc << EOF
  > [extensions]
  > unrandomsample = $TESTTMP/unrandomsample.py
  > EOF

  $ rm -rf r1/.hg/blackbox*
  $ hg -R r1 blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"discovery"}}'
  $ cd ..
