  $ . $TESTDIR/require-ext.sh remotenames
  $ extpath=`dirname $TESTDIR`
  $ cp -r $extpath/infinitepush $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > infinitepush=$TESTTMP/infinitepush
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ scratchnodes() {
  >    for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
  >        echo ${node##*/}
  >    done
  > }
  $ scratchbookmarks() {
  >    for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
  >        echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  >    done
  > }

Check that we can send a scratch on the server and it does not show there in
the history but is stored on disk
  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF
  $ cd ..
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client -q
  $ cd client
  $ mkcommit scratchcommitwithremotenames
  $ hg push --config extensions.remotenames= -r . --to scratch/mybranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     7af6bee519b8  scratchcommitwithremotenames
  $ hg log -G
  @  changeset:   0:7af6bee519b8
     bookmark:    scratch/mybranch
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     scratchcommitwithremotenames
  
  $ hg -R ../repo log -G
  $ scratchnodes
  7af6bee519b89cb38baec00e893355f7c29f6d21
  $ scratchbookmarks
  scratch/mybranch 7af6bee519b89cb38baec00e893355f7c29f6d21
