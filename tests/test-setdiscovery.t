
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
  >     echo "% -- a -> b tree"
  >     hg -R a debugdiscovery b --verbose --old
  >     echo
  >     echo "% -- a -> b set"
  >     hg -R a debugdiscovery b --verbose --debug
  >     echo
  >     echo "% -- b -> a tree"
  >     hg -R b debugdiscovery a --verbose --old
  >     echo
  >     echo "% -- b -> a set"
  >     hg -R b debugdiscovery a --verbose --debug
  >     cd ..
  > }


Small superset:

  $ testdesc '-ra1 -ra2' '-rb1 -rb2 -rb3' '
  > +2:f +1:a1:b1
  > <f +4 :a2
  > +5 :b2
  > <f +3 :b3'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: 01241442b3c2 66f7d451a68b b5714e113bc0
  common heads: 01241442b3c2 b5714e113bc0
  local is subset
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  all local heads known remotely
  common heads: 01241442b3c2 b5714e113bc0
  local is subset
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 01241442b3c2 b5714e113bc0
  common heads: 01241442b3c2 b5714e113bc0
  remote is subset
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  all remote heads known locally
  common heads: 01241442b3c2 b5714e113bc0
  remote is subset


Many new:

  $ testdesc '-ra1 -ra2' '-rb' '
  > +2:f +3:a1 +3:b
  > <f +30 :a2'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: bebd167eb94d
  common heads: bebd167eb94d
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  taking initial sample
  searching: 2 queries
  query 2; still undecided: 29, sample size is: 29
  2 total queries
  common heads: bebd167eb94d
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 66f7d451a68b bebd167eb94d
  common heads: bebd167eb94d
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  taking initial sample
  searching: 2 queries
  query 2; still undecided: 2, sample size is: 2
  2 total queries
  common heads: bebd167eb94d


Both sides many new with stub:

  $ testdesc '-ra1 -ra2' '-rb' '
  > +2:f +2:a1 +30 :b
  > <f +30 :a2'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: 2dc09a01254d
  common heads: 2dc09a01254d
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  taking initial sample
  searching: 2 queries
  query 2; still undecided: 29, sample size is: 29
  2 total queries
  common heads: 2dc09a01254d
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 2dc09a01254d 66f7d451a68b
  common heads: 2dc09a01254d
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  taking initial sample
  searching: 2 queries
  query 2; still undecided: 29, sample size is: 29
  2 total queries
  common heads: 2dc09a01254d


Both many new:

  $ testdesc '-ra' '-rb' '
  > +2:f +30 :b
  > <f +30 :a'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: 66f7d451a68b
  common heads: 66f7d451a68b
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 31, sample size is: 31
  2 total queries
  common heads: 66f7d451a68b
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 66f7d451a68b
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 31, sample size is: 31
  2 total queries
  common heads: 66f7d451a68b


Both many new skewed:

  $ testdesc '-ra' '-rb' '
  > +2:f +30 :b
  > <f +50 :a'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: 66f7d451a68b
  common heads: 66f7d451a68b
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 51, sample size is: 51
  2 total queries
  common heads: 66f7d451a68b
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 66f7d451a68b
  common heads: 66f7d451a68b
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 31, sample size is: 31
  2 total queries
  common heads: 66f7d451a68b


Both many new on top of long history:

  $ testdesc '-ra' '-rb' '
  > +1000:f +30 :b
  > <f +50 :a'
  
  % -- a -> b tree
  comparing with b
  searching for changes
  unpruned common: 7ead0cba2838
  common heads: 7ead0cba2838
  
  % -- a -> b set
  comparing with b
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 1049, sample size is: 11
  sampling from both directions
  searching: 3 queries
  query 3; still undecided: 31, sample size is: 31
  3 total queries
  common heads: 7ead0cba2838
  
  % -- b -> a tree
  comparing with a
  searching for changes
  unpruned common: 7ead0cba2838
  common heads: 7ead0cba2838
  
  % -- b -> a set
  comparing with a
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 1029, sample size is: 11
  sampling from both directions
  searching: 3 queries
  query 3; still undecided: 15, sample size is: 15
  3 total queries
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

  $ hg clone -b a . a
  adding changesets
  adding manifests
  adding file changes
  added 1340 changesets with 0 changes to 0 files (+259 heads)
  updating to branch a
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone -b b . b
  adding changesets
  adding manifests
  adding file changes
  added 304 changesets with 0 changes to 0 files
  updating to branch b
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R a debugdiscovery b --debug --verbose
  comparing with b
  query 1; heads
  searching for changes
  taking quick initial sample
  searching: 2 queries
  query 2; still undecided: 1080, sample size is: 260
  sampling from both directions
  searching: 3 queries
  query 3; still undecided: 820, sample size is: 260
  sampling from both directions
  searching: 4 queries
  query 4; still undecided: 560, sample size is: 260
  sampling from both directions
  searching: 5 queries
  query 5; still undecided: 300, sample size is: 200
  5 total queries
  common heads: 3ee37d65064a

  $ cd ..
