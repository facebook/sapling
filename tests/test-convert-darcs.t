
  $ "$TESTDIR/hghave" darcs || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ echo 'graphlog =' >> $HGRCPATH
  $ DARCS_EMAIL='test@example.org'; export DARCS_EMAIL
  $ HOME=`pwd`/do_not_use_HOME_darcs; export HOME

skip if we can't import elementtree

  $ mkdir dummy
  $ mkdir dummy/_darcs
  $ if hg convert dummy 2>&1 | grep ElementTree > /dev/null; then
  >     echo 'skipped: missing feature: elementtree module'
  >     exit 80
  > fi

try converting darcs1 repository

  $ hg clone -q "$TESTDIR/darcs1.hg" darcs
  $ hg convert -s darcs darcs/darcs1 2>&1 | grep darcs-1.0
  darcs-1.0 repository format is unsupported, please upgrade

initialize darcs repo

  $ mkdir darcs-repo
  $ cd darcs-repo
  $ darcs init
  $ echo a > a
  $ darcs record -a -l -m p0
  Finished recording patch 'p0'
  $ cd ..

branch and update

  $ darcs get darcs-repo darcs-clone >/dev/null
  $ cd darcs-clone
  $ echo c >> a
  $ echo c > c
  $ darcs record -a -l -m p1.1
  Finished recording patch 'p1.1'
  $ cd ..

update source

  $ cd darcs-repo
  $ echo b >> a
  $ echo b > b
  $ darcs record -a -l -m p1.2
  Finished recording patch 'p1.2'

  $ darcs pull -a ../darcs-clone
  Backing up ./a(-darcs-backup0)
  We have conflicts in the following files:
  ./a
  Finished pulling and applying.
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

test utf-8 commit message and author

  $ echo g > g

darcs is encoding agnostic, so it takes whatever bytes it's given

  $ darcs record -a -l -m 'p4: desc ñ' -A 'author ñ'
  Finished recording patch 'p4: desc \xc3\xb1' (esc)

Test latin-1 commit message

  $ echo h > h
  $ printf "p5: desc " > ../p5
  $ python -c 'print "".join([chr(i) for i in range(128, 256)])' >> ../p5
  $ darcs record -a -l --logfile ../p5
  Finished recording patch 'p5: desc \x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff' (esc)
 
  $ glog()
  > {
  >     HGENCODING=utf-8 hg glog --template '{rev} "{desc|firstline}" ({author}) files: {files}\n' "$@"
  > }
  $ cd ..
  $ hg convert darcs-repo darcs-repo-hg
  initializing destination darcs-repo-hg repository
  scanning source...
  sorting...
  converting...
  6 p0
  5 p1.2
  4 p1.1
  3 p2
  2 p3
  1 p4: desc ?
  0 p5: desc ????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????

The converter does not currently handle patch conflicts very well.
When they occur, it reverts *all* changes and moves forward,
letting the conflict resolving patch fix collisions.
Unfortunately, non-conflicting changes, like the addition of the
"c" file in p1.1 patch are reverted too.
Just to say that manifest not listing "c" here is a bug.

  $ HGENCODING=latin-1 glog -R darcs-repo-hg -r 6
  o  6 "p5: desc \xc2\x80\xc2\x81\xc2\x82\xc2\x83\xc2\x84\xc2\x85\xc2\x86\xc2\x87\xc2\x88\xc2\x89\xc2\x8a\xc2\x8b\xc2\x8c\xc2\x8d\xc2\x8e\xc2\x8f\xc2\x90\xc2\x91\xc2\x92\xc2\x93\xc2\x94\xc2\x95\xc2\x96\xc2\x97\xc2\x98\xc2\x99\xc2\x9a\xc2\x9b\xc2\x9c\xc2\x9d\xc2\x9e\xc2\x9f\xc2\xa0\xc2\xa1\xc2\xa2\xc2\xa3\xc2\xa4\xc2\xa5\xc2\xa6\xc2\xa7\xc2\xa8\xc2\xa9\xc2\xaa\xc2\xab\xc2\xac\xc2\xad\xc2\xae\xc2\xaf\xc2\xb0\xc2\xb1\xc2\xb2\xc2\xb3\xc2\xb4\xc2\xb5\xc2\xb6\xc2\xb7\xc2\xb8\xc2\xb9\xc2\xba\xc2\xbb\xc2\xbc\xc2\xbd\xc2\xbe\xc2\xbf\xc3\x80\xc3\x81\xc3\x82\xc3\x83\xc3\x84\xc3\x85\xc3\x86\xc3\x87\xc3\x88\xc3\x89\xc3\x8a\xc3\x8b\xc3\x8c\xc3\x8d\xc3\x8e\xc3\x8f\xc3\x90\xc3\x91\xc3\x92\xc3\x93\xc3\x94\xc3\x95\xc3\x96\xc3\x97\xc3\x98\xc3\x99\xc3\x9a\xc3\x9b\xc3\x9c\xc3\x9d\xc3\x9e\xc3\x9f\xc3\xa0\xc3\xa1\xc3\xa2\xc3\xa3\xc3\xa4\xc3\xa5\xc3\xa6\xc3\xa7\xc3\xa8\xc3\xa9\xc3\xaa\xc3\xab\xc3\xac\xc3\xad\xc3\xae\xc3\xaf\xc3\xb0\xc3\xb1\xc3\xb2\xc3\xb3\xc3\xb4\xc3\xb5\xc3\xb6\xc3\xb7\xc3\xb8\xc3\xb9\xc3\xba\xc3\xbb\xc3\xbc\xc3\xbd\xc3\xbe\xc3\xbf" (test@example.org) files: h (esc)
  |
  $ HGENCODING=utf-8 glog -R darcs-repo-hg -r 0:5
  o  5 "p4: desc \xc3\xb1" (author \xc3\xb1) files: g (esc)
  |
  o  4 "p3" (test@example.org) files: dir/d dir/d2 dir2/d f ff
  |
  o  3 "p2" (test@example.org) files: a dir/d dir/d2 f
  |
  o  2 "p1.1" (test@example.org) files:
  |
  o  1 "p1.2" (test@example.org) files: a b
  |
  o  0 "p0" (test@example.org) files: a
  

  $ hg up -q -R darcs-repo-hg
  $ hg -R darcs-repo-hg manifest --debug
  7225b30cdf38257d5cc7780772c051b6f33e6d6b 644   a
  1e88685f5ddec574a34c70af492f95b6debc8741 644   b
  37406831adc447ec2385014019599dfec953c806 644   dir2/d
  b783a337463792a5c7d548ad85a7d3253c16ba8c 644   ff
  0973eb1b2ecc4de7fafe7447ce1b7462108b4848 644   g
  fe6f8b4f507fe3eb524c527192a84920a4288dac 644   h
