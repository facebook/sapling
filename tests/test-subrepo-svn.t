  $ "$TESTDIR/hghave" svn15 || exit 80

  $ fix_path()
  > {
  >     tr '\\' /
  > }

SVN wants all paths to start with a slash. Unfortunately, Windows ones
don't. Handle that.

  $ escapedwd=`pwd | fix_path`
  $ expr "$escapedwd" : '\/' > /dev/null || escapedwd="/$escapedwd"
  $ escapedwd=`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$escapedwd"`

create subversion repo

  $ SVNREPO="file://$escapedwd/svn-repo"
  $ WCROOT="`pwd`/svn-wc"
  $ svnadmin create svn-repo
  $ svn co "$SVNREPO" svn-wc
  Checked out revision 0.
  $ cd svn-wc
  $ mkdir src
  $ echo alpha > src/alpha
  $ svn add src
  A         src
  A         src/alpha
  $ mkdir externals
  $ echo other > externals/other
  $ svn add externals
  A         externals
  A         externals/other
  $ svn ci -m 'Add alpha'
  Adding         externals
  Adding         externals/other
  Adding         src
  Adding         src/alpha
  Transmitting file data ..
  Committed revision 1.
  $ svn up
  At revision 1.
  $ echo "externals -r1 $SVNREPO/externals" > extdef
  $ svn propset -F extdef svn:externals src
  property 'svn:externals' set on 'src'
  $ svn ci -m 'Setting externals'
  Sending        src
  
  Committed revision 2.
  $ cd ..

create hg repo

  $ mkdir sub
  $ cd sub
  $ hg init t
  $ cd t

first revision, no sub

  $ echo a > a
  $ hg ci -Am0
  adding a

add first svn sub with leading whitespaces

  $ echo "s =        [svn]       $SVNREPO/src" >> .hgsub
  $ echo "subdir/s = [svn]       $SVNREPO/src" >> .hgsub
  $ svn co --quiet "$SVNREPO"/src s
  $ mkdir subdir
  $ svn co --quiet "$SVNREPO"/src subdir/s
  $ hg add .hgsub
  $ hg ci -m1
  committing subrepository s
  committing subrepository subdir/s

make sure we avoid empty commits (issue2445)

  $ hg sum
  parent: 1:* tip (glob)
   1
  branch: default
  commit: (clean)
  update: (current)
  $ hg ci -moops
  nothing changed
  [1]

debugsub

  $ hg debugsub
  path s
   source   file://*/svn-repo/src (glob)
   revision 2
  path subdir/s
   source   file://*/svn-repo/src (glob)
   revision 2

change file in svn and hg, commit

  $ echo a >> a
  $ echo alpha >> s/alpha
  $ hg sum
  parent: 1:* tip (glob)
   1
  branch: default
  commit: 1 modified, 1 subrepos
  update: (current)
  $ hg commit --subrepos -m 'Message!'
  committing subrepository s
  Sending*s/alpha (glob)
  Transmitting file data .
  Committed revision 3.
  
  Fetching external item into '$TESTTMP/sub/t/s/externals'
  External at revision 1.
  
  At revision 3.
  $ hg debugsub
  path s
   source   file://*/svn-repo/src (glob)
   revision 3
  path subdir/s
   source   file://*/svn-repo/src (glob)
   revision 2

add an unrelated revision in svn and update the subrepo to without
bringing any changes.

  $ svn mkdir "$SVNREPO/unrelated" -m 'create unrelated'
  
  Committed revision 4.
  $ svn up s
  
  Fetching external item into 's/externals'
  External at revision 1.
  
  At revision 4.
  $ hg sum
  parent: 2:* tip (glob)
   Message!
  branch: default
  commit: (clean)
  update: (current)

  $ echo a > s/a

should be empty despite change to s/a

  $ hg st

add a commit from svn

  $ cd "$WCROOT"/src
  $ svn up
  U    alpha
  
  Fetching external item into 'externals'
  A    externals/other
  Updated external to revision 1.
  
  Updated to revision 4.
  $ echo xyz >> alpha
  $ svn propset svn:mime-type 'text/xml' alpha
  property 'svn:mime-type' set on 'alpha'
  $ svn ci -m 'amend a from svn'
  Sending        src/alpha
  Transmitting file data .
  Committed revision 5.
  $ cd ../../sub/t

this commit from hg will fail

  $ echo zzz >> s/alpha
  $ hg ci --subrepos -m 'amend alpha from hg'
  committing subrepository s
  abort: svn: Commit failed (details follow):
  svn: (Out of date)?.*/src/alpha.*(is out of date)? (re)
  [255]
  $ svn revert -q s/alpha

this commit fails because of meta changes

  $ svn propset svn:mime-type 'text/html' s/alpha
  property 'svn:mime-type' set on 's/alpha'
  $ hg ci --subrepos -m 'amend alpha from hg'
  committing subrepository s
  abort: svn: Commit failed (details follow):
  svn: (Out of date)?.*/src/alpha.*(is out of date)? (re)
  [255]
  $ svn revert -q s/alpha

this commit fails because of externals changes

  $ echo zzz > s/externals/other
  $ hg ci --subrepos -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals
  [255]
  $ hg diff --subrepos -r 1:2 | grep -v diff
  --- a/.hgsubstate	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgsubstate	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,2 @@
  -2 s
  +3 s
   2 subdir/s
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  $ svn revert -q s/externals/other

this commit fails because of externals meta changes

  $ svn propset svn:mime-type 'text/html' s/externals/other
  property 'svn:mime-type' set on 's/externals/other'
  $ hg ci --subrepos -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals
  [255]
  $ svn revert -q s/externals/other

clone

  $ cd ..
  $ hg clone t tc | fix_path
  updating to branch default
  A    tc/s/alpha
   U   tc/s
  
  Fetching external item into 'tc/s/externals'
  A    tc/s/externals/other
  Checked out external at revision 1.
  
  Checked out revision 3.
  A    tc/subdir/s/alpha
   U   tc/subdir/s
  
  Fetching external item into 'tc/subdir/s/externals'
  A    tc/subdir/s/externals/other
  Checked out external at revision 1.
  
  Checked out revision 2.
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tc

debugsub in clone

  $ hg debugsub
  path s
   source   file://*/svn-repo/src (glob)
   revision 3
  path subdir/s
   source   file://*/svn-repo/src (glob)
   revision 2

verify subrepo is contained within the repo directory

  $ python -c "import os.path; print os.path.exists('s')"
  True

update to nullrev (must delete the subrepo)

  $ hg up null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ ls

Check hg update --clean
  $ cd $TESTTMP/sub/t
  $ cd s
  $ echo c0 > alpha
  $ echo c1 > f1
  $ echo c1 > f2
  $ svn add f1 -q
  $ svn status
  ? *    a (glob)
  X *    externals (glob)
  ? *    f2 (glob)
  M *    alpha (glob)
  A *    f1 (glob)
  
  Performing status on external item at 'externals'
  $ cd ../..
  $ hg -R t update -C
  
  Fetching external item into 't/s/externals'
  Checked out external at revision 1.
  
  Checked out revision 3.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd t/s
  $ svn status
  ? *    a (glob)
  X *    externals (glob)
  ? *    f1 (glob)
  ? *    f2 (glob)
  
  Performing status on external item at 'externals'

Sticky subrepositories, no changes
  $ cd $TESTTMP/sub/t
  $ hg id -n
  2
  $ cd s
  $ svnversion
  3
  $ cd ..
  $ hg update 1
  U    $TESTTMP/sub/t/s/alpha
  
  Fetching external item into '$TESTTMP/sub/t/s/externals'
  Checked out external at revision 1.
  
  Checked out revision 2.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1
  $ cd s
  $ svnversion
  2
  $ cd ..

Sticky subrepositorys, file changes
  $ touch s/f1
  $ cd s
  $ svn add f1
  A         f1
  $ cd ..
  $ hg id -n
  1
  $ cd s
  $ svnversion
  2M
  $ cd ..
  $ hg update tip
   subrepository sources for s differ
  use (l)ocal source (2) or (r)emote source (3)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  2+
  $ cd s
  $ svnversion
  2M
  $ cd ..
  $ hg update --clean tip
  U    $TESTTMP/sub/t/s/alpha
  
  Fetching external item into '$TESTTMP/sub/t/s/externals'
  Checked out external at revision 1.
  
  Checked out revision 3.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Sticky subrepository, revision updates
  $ hg id -n
  2
  $ cd s
  $ svnversion
  3
  $ cd ..
  $ cd s
  $ svn update -r 1
  U    alpha
   U   .
  
  Fetching external item into 'externals'
  Updated external to revision 1.
  
  Updated to revision 1.
  $ cd ..
  $ hg update 1
   subrepository sources for s differ (in checked out version)
  use (l)ocal source (1) or (r)emote source (2)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1+
  $ cd s
  $ svnversion
  1
  $ cd ..

Sticky subrepository, file changes and revision updates
  $ touch s/f1
  $ cd s
  $ svn add f1
  A         f1
  $ svnversion
  1M
  $ cd ..
  $ hg id -n
  1+
  $ hg update tip
   subrepository sources for s differ
  use (l)ocal source (1) or (r)emote source (3)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  2
  $ cd s
  $ svnversion
  1M
  $ cd ..

Sticky repository, update --clean
  $ hg update --clean tip
  U    $TESTTMP/sub/t/s/alpha
   U   $TESTTMP/sub/t/s
  
  Fetching external item into '$TESTTMP/sub/t/s/externals'
  Checked out external at revision 1.
  
  Checked out revision 3.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  2
  $ cd s
  $ svnversion
  3
  $ cd ..

Test subrepo already at intended revision:
  $ cd s
  $ svn update -r 2
  U    alpha
  
  Fetching external item into 'externals'
  Updated external to revision 1.
  
  Updated to revision 2.
  $ cd ..
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1+
  $ cd s
  $ svnversion
  2
  $ cd ..

Test case where subversion would fail to update the subrepo because there
are unknown directories being replaced by tracked ones (happens with rebase).

  $ cd $WCROOT/src
  $ mkdir dir
  $ echo epsilon.py > dir/epsilon.py
  $ svn add dir
  A         dir
  A         dir/epsilon.py
  $ svn ci -m 'Add dir/epsilon.py'
  Adding         src/dir
  Adding         src/dir/epsilon.py
  Transmitting file data .
  Committed revision 6.
  $ cd ../..
  $ hg init rebaserepo
  $ cd rebaserepo
  $ svn co -r5 --quiet "$SVNREPO"/src s
  $ echo "s =        [svn]       $SVNREPO/src" >> .hgsub
  $ hg add .hgsub
  $ hg ci -m addsub
  committing subrepository s
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ svn up -r6 s
  A    s/dir
  A    s/dir/epsilon.py
  
  Fetching external item into 's/externals'
  Updated external to revision 1.
  
  Updated to revision 6.
  $ hg ci -m updatesub
  committing subrepository s
  created new head
  $ echo pyc > s/dir/epsilon.pyc
  $ hg up 1
  D    $TESTTMP/rebaserepo/s/dir
  
  Fetching external item into '$TESTTMP/rebaserepo/s/externals'
  Checked out external at revision 1.
  
  Checked out revision 5.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ if "$TESTDIR/hghave" -q svn15; then
  > hg up 2 >/dev/null 2>&1 || echo update failed
  > fi

Modify one of the externals to point to a different path so we can
test having obstructions when switching branches on checkout:
  $ hg checkout tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "obstruct =        [svn]       $SVNREPO/externals" >> .hgsub
  $ svn co -r5 --quiet "$SVNREPO"/externals obstruct
  $ hg commit -m 'Start making obstructed working copy'
  committing subrepository obstruct
  $ hg book other
  $ hg co -r 'p1(tip)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "obstruct =        [svn]       $SVNREPO/src" >> .hgsub
  $ svn co -r5 --quiet "$SVNREPO"/src obstruct
  $ hg commit -m 'Other branch which will be obstructed'
  committing subrepository obstruct
  created new head

Switching back to the head where we have another path mapped to the
same subrepo should work if the subrepo is clean.
  $ hg co other
  A    $TESTTMP/rebaserepo/obstruct/other
  Checked out revision 1.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

This is surprising, but is also correct based on the current code:
  $ echo "updating should (maybe) fail" > obstruct/other
  $ hg co tip
  abort: crosses branches (merge branches or use --clean to discard changes)
  [255]

Point to a Subversion branch which has since been deleted and recreated
First, create that condition in the repository.

  $ hg ci --subrepos -m cleanup
  committing subrepository obstruct
  Sending        obstruct/other
  Transmitting file data .
  Committed revision 7.
  At revision 7.
  $ svn mkdir -m "baseline" $SVNREPO/trunk
  
  Committed revision 8.
  $ svn copy -m "initial branch" $SVNREPO/trunk $SVNREPO/branch
  
  Committed revision 9.
  $ svn co --quiet "$SVNREPO"/branch tempwc
  $ cd tempwc
  $ echo "something old" > somethingold
  $ svn add somethingold
  A         somethingold
  $ svn ci -m 'Something old'
  Adding         somethingold
  Transmitting file data .
  Committed revision 10.
  $ svn rm -m "remove branch" $SVNREPO/branch
  
  Committed revision 11.
  $ svn copy -m "recreate branch" $SVNREPO/trunk $SVNREPO/branch
  
  Committed revision 12.
  $ svn up
  D    somethingold
  Updated to revision 12.
  $ echo "something new" > somethingnew
  $ svn add somethingnew
  A         somethingnew
  $ svn ci -m 'Something new'
  Adding         somethingnew
  Transmitting file data .
  Committed revision 13.
  $ cd ..
  $ rm -rf tempwc
  $ svn co "$SVNREPO/branch"@10 recreated
  A    recreated/somethingold
  Checked out revision 10.
  $ echo "recreated =        [svn]       $SVNREPO/branch" >> .hgsub
  $ hg ci -m addsub
  committing subrepository recreated
  $ cd recreated
  $ svn up
  D    somethingold
  A    somethingnew
  Updated to revision 13.
  $ cd ..
  $ hg ci -m updatesub
  committing subrepository recreated
  $ hg up -r-2
  D    $TESTTMP/rebaserepo/recreated/somethingnew
  A    $TESTTMP/rebaserepo/recreated/somethingold
  Checked out revision 10.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ test -f recreated/somethingold

