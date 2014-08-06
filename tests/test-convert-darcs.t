#require darcs

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ DARCS_EMAIL='test@example.org'; export DARCS_EMAIL

initialize darcs repo

  $ mkdir darcs-repo
  $ cd darcs-repo
  $ darcs init
  $ echo a > a
  $ darcs record -a -l -m p0
  Finished recording patch 'p0'
  $ cd ..

branch and update

  $ darcs get -q darcs-repo darcs-clone >/dev/null
  $ cd darcs-clone
  $ echo c >> a
  $ echo c > c
  $ darcs record -a -l -m p1.1
  Finished recording patch 'p1.1'
  $ cd ..

skip if we can't import elementtree

  $ if hg convert darcs-repo darcs-dummy 2>&1 | grep ElementTree > /dev/null; then
  >     echo 'skipped: missing feature: elementtree module'
  >     exit 80
  > fi

update source

  $ cd darcs-repo
  $ echo b >> a
  $ echo b > b
  $ darcs record -a -l -m p1.2
  Finished recording patch 'p1.2'

  $ darcs pull -q -a --no-set-default ../darcs-clone
  Backing up ./a(*) (glob)
  We have conflicts in the following files:
  ./a
  $ sleep 1
  $ echo e > a
  $ echo f > f
  $ mkdir dir
  $ echo d > dir/d
  $ echo d > dir/d2
  $ darcs record -a -l -m p2
  Finished recording patch 'p2'

test file and directory move

  $ darcs mv f ff

Test remove + move

  $ darcs remove dir/d2
  $ rm dir/d2
  $ darcs mv dir dir2
  $ darcs record -a -l -m p3
  Finished recording patch 'p3'

The converter does not currently handle patch conflicts very well.
When they occur, it reverts *all* changes and moves forward,
letting the conflict resolving patch fix collisions.
Unfortunately, non-conflicting changes, like the addition of the
"c" file in p1.1 patch are reverted too.
Just to say that manifest not listing "c" here is a bug.

  $ cd ..
  $ hg convert darcs-repo darcs-repo-hg
  initializing destination darcs-repo-hg repository
  scanning source...
  sorting...
  converting...
  4 p0
  3 p1.2
  2 p1.1
  1 p2
  0 p3
  $ hg log -R darcs-repo-hg -g --template '{rev} "{desc|firstline}" ({author}) files: {files}\n' "$@"
  4 "p3" (test@example.org) files: dir/d dir/d2 dir2/d f ff
  3 "p2" (test@example.org) files: a dir/d dir/d2 f
  2 "p1.1" (test@example.org) files: 
  1 "p1.2" (test@example.org) files: a b
  0 "p0" (test@example.org) files: a

  $ hg up -q -R darcs-repo-hg
  $ hg -R darcs-repo-hg manifest --debug
  7225b30cdf38257d5cc7780772c051b6f33e6d6b 644   a
  1e88685f5ddec574a34c70af492f95b6debc8741 644   b
  37406831adc447ec2385014019599dfec953c806 644   dir2/d
  b783a337463792a5c7d548ad85a7d3253c16ba8c 644   ff

#if no-outer-repo

try converting darcs1 repository

  $ hg clone -q "$TESTDIR/bundles/darcs1.hg" darcs
  $ hg convert -s darcs darcs/darcs1 2>&1 | grep darcs-1.0
  darcs-1.0 repository format is unsupported, please upgrade

#endif
