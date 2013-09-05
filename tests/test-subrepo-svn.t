  $ "$TESTDIR/hghave" svn15 || exit 80

  $ SVNREPOPATH=`pwd`/svn-repo
#if windows
  $ SVNREPOURL=file:///`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#else
  $ SVNREPOURL=file://`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#endif

create subversion repo

  $ WCROOT="`pwd`/svn-wc"
  $ svnadmin create svn-repo
  $ svn co "$SVNREPOURL" svn-wc
  Checked out revision 0.
  $ cd svn-wc
  $ mkdir src
  $ echo alpha > src/alpha
  $ svn add src
  A         src
  A         src/alpha (glob)
  $ mkdir externals
  $ echo other > externals/other
  $ svn add externals
  A         externals
  A         externals/other (glob)
  $ svn ci -m 'Add alpha'
  Adding         externals
  Adding         externals/other (glob)
  Adding         src
  Adding         src/alpha (glob)
  Transmitting file data ..
  Committed revision 1.
  $ svn up -q
  $ echo "externals -r1 $SVNREPOURL/externals" > extdef
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

  $ echo "s =        [svn]       $SVNREPOURL/src" >> .hgsub
  $ echo "subdir/s = [svn]       $SVNREPOURL/src" >> .hgsub
  $ svn co --quiet "$SVNREPOURL"/src s
  $ mkdir subdir
  $ svn co --quiet "$SVNREPOURL"/src subdir/s
  $ hg add .hgsub
  $ hg ci -m1

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
  $ hg commit --subrepos -m 'Message!' | grep -v Updating
  committing subrepository s
  Sending*s/alpha (glob)
  Transmitting file data .
  Committed revision 3.
  
  Fetching external item into '*s/externals'* (glob)
  External at revision 1.
  
  At revision 3.
  $ hg debugsub
  path s
   source   file://*/svn-repo/src (glob)
   revision 3
  path subdir/s
   source   file://*/svn-repo/src (glob)
   revision 2

missing svn file, commit should fail

  $ rm s/alpha
  $ hg commit --subrepos -m 'abort on missing file'
  committing subrepository s
  abort: cannot commit missing svn entries (in subrepo s)
  [255]
  $ svn revert s/alpha > /dev/null

add an unrelated revision in svn and update the subrepo to without
bringing any changes.

  $ svn mkdir "$SVNREPOURL/unrelated" -m 'create unrelated'
  
  Committed revision 4.
  $ svn up -q s
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

  $ cd "$WCROOT/src"
  $ svn up -q
  $ echo xyz >> alpha
  $ svn propset svn:mime-type 'text/xml' alpha
  property 'svn:mime-type' set on 'alpha'
  $ svn ci -m 'amend a from svn'
  Sending        *alpha (glob)
  Transmitting file data .
  Committed revision 5.
  $ cd ../../sub/t

this commit from hg will fail

  $ echo zzz >> s/alpha
  $ (hg ci --subrepos -m 'amend alpha from hg' 2>&1; echo "[$?]") | grep -vi 'out of date'
  committing subrepository s
  abort: svn:*Commit failed (details follow): (glob)
  [255]
  $ svn revert -q s/alpha

this commit fails because of meta changes

  $ svn propset svn:mime-type 'text/html' s/alpha
  property 'svn:mime-type' set on 's/alpha' (glob)
  $ (hg ci --subrepos -m 'amend alpha from hg' 2>&1; echo "[$?]") | grep -vi 'out of date'
  committing subrepository s
  abort: svn:*Commit failed (details follow): (glob)
  [255]
  $ svn revert -q s/alpha

this commit fails because of externals changes

  $ echo zzz > s/externals/other
  $ hg ci --subrepos -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals (in subrepo s)
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
  property 'svn:mime-type' set on 's/externals/other' (glob)
  $ hg ci --subrepos -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals (in subrepo s)
  [255]
  $ svn revert -q s/externals/other

clone

  $ cd ..
  $ hg clone t tc
  updating to branch default
  A    tc/s/alpha (glob)
   U   tc/s (glob)
  
  Fetching external item into 'tc/s/externals'* (glob)
  A    tc/s/externals/other (glob)
  Checked out external at revision 1.
  
  Checked out revision 3.
  A    tc/subdir/s/alpha (glob)
   U   tc/subdir/s (glob)
  
  Fetching external item into 'tc/subdir/s/externals'* (glob)
  A    tc/subdir/s/externals/other (glob)
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
  $ cd "$TESTTMP/sub/t"
  $ cd s
  $ echo c0 > alpha
  $ echo c1 > f1
  $ echo c1 > f2
  $ svn add f1 -q
  $ svn status | sort
  
  ? *    a (glob)
  ? *    f2 (glob)
  A *    f1 (glob)
  M *    alpha (glob)
  Performing status on external item at 'externals'* (glob)
  X *    externals (glob)
  $ cd ../..
  $ hg -R t update -C
  
  Fetching external item into 't/s/externals'* (glob)
  Checked out external at revision 1.
  
  Checked out revision 3.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd t/s
  $ svn status | sort
  
  ? *    a (glob)
  ? *    f1 (glob)
  ? *    f2 (glob)
  Performing status on external item at 'externals'* (glob)
  X *    externals (glob)

Sticky subrepositories, no changes
  $ cd "$TESTTMP/sub/t"
  $ hg id -n
  2
  $ cd s
  $ svnversion
  3
  $ cd ..
  $ hg update 1
  U    *s/alpha (glob)
  
  Fetching external item into '*s/externals'* (glob)
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
  1+
  $ cd s
  $ svnversion
  2M
  $ cd ..
  $ hg update tip
   subrepository s diverged (local revision: 2, remote revision: 3)
  (M)erge, keep (l)ocal or keep (r)emote? m
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
  U    *s/alpha (glob)
  
  Fetching external item into '*s/externals'* (glob)
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
  $ svn update -qr 1
  $ cd ..
  $ hg update 1
   subrepository s diverged (local revision: 3, remote revision: 2)
  (M)erge, keep (l)ocal or keep (r)emote? m
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
   subrepository s diverged (local revision: 3, remote revision: 3)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ
  use (l)ocal source (1) or (r)emote source (3)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  2+
  $ cd s
  $ svnversion
  1M
  $ cd ..

Sticky repository, update --clean
  $ hg update --clean tip | grep -v 's[/\]externals[/\]other'
  U    *s/alpha (glob)
   U   *s (glob)
  
  Fetching external item into '*s/externals'* (glob)
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
  $ svn update -qr 2
  $ cd ..
  $ hg update 1
   subrepository s diverged (local revision: 3, remote revision: 2)
  (M)erge, keep (l)ocal or keep (r)emote? m
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1+
  $ cd s
  $ svnversion
  2
  $ cd ..

Test case where subversion would fail to update the subrepo because there
are unknown directories being replaced by tracked ones (happens with rebase).

  $ cd "$WCROOT/src"
  $ mkdir dir
  $ echo epsilon.py > dir/epsilon.py
  $ svn add dir
  A         dir
  A         dir/epsilon.py (glob)
  $ svn ci -m 'Add dir/epsilon.py'
  Adding         *dir (glob)
  Adding         *dir/epsilon.py (glob)
  Transmitting file data .
  Committed revision 6.
  $ cd ../..
  $ hg init rebaserepo
  $ cd rebaserepo
  $ svn co -r5 --quiet "$SVNREPOURL"/src s
  $ echo "s =        [svn]       $SVNREPOURL/src" >> .hgsub
  $ hg add .hgsub
  $ hg ci -m addsub
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ svn up -qr6 s
  $ hg ci -m updatesub
  created new head
  $ echo pyc > s/dir/epsilon.pyc
  $ hg up 1
  D    *s/dir (glob)
  
  Fetching external item into '*s/externals'* (glob)
  Checked out external at revision 1.
  
  Checked out revision 5.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up -q 2

Modify one of the externals to point to a different path so we can
test having obstructions when switching branches on checkout:
  $ hg checkout tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "obstruct =        [svn]       $SVNREPOURL/externals" >> .hgsub
  $ svn co -r5 --quiet "$SVNREPOURL"/externals obstruct
  $ hg commit -m 'Start making obstructed working copy'
  $ hg book other
  $ hg co -r 'p1(tip)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "obstruct =        [svn]       $SVNREPOURL/src" >> .hgsub
  $ svn co -r5 --quiet "$SVNREPOURL"/src obstruct
  $ hg commit -m 'Other branch which will be obstructed'
  created new head

Switching back to the head where we have another path mapped to the
same subrepo should work if the subrepo is clean.
  $ hg co other
  A    *obstruct/other (glob)
  Checked out revision 1.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

This is surprising, but is also correct based on the current code:
  $ echo "updating should (maybe) fail" > obstruct/other
  $ hg co tip
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  [255]

Point to a Subversion branch which has since been deleted and recreated
First, create that condition in the repository.

  $ hg ci --subrepos -m cleanup | grep -v Updating
  committing subrepository obstruct
  Sending        obstruct/other (glob)
  Transmitting file data .
  Committed revision 7.
  At revision 7.
  $ svn mkdir -m "baseline" $SVNREPOURL/trunk
  
  Committed revision 8.
  $ svn copy -m "initial branch" $SVNREPOURL/trunk $SVNREPOURL/branch
  
  Committed revision 9.
  $ svn co --quiet "$SVNREPOURL"/branch tempwc
  $ cd tempwc
  $ echo "something old" > somethingold
  $ svn add somethingold
  A         somethingold
  $ svn ci -m 'Something old'
  Adding         somethingold
  Transmitting file data .
  Committed revision 10.
  $ svn rm -m "remove branch" $SVNREPOURL/branch
  
  Committed revision 11.
  $ svn copy -m "recreate branch" $SVNREPOURL/trunk $SVNREPOURL/branch
  
  Committed revision 12.
  $ svn up -q
  $ echo "something new" > somethingnew
  $ svn add somethingnew
  A         somethingnew
  $ svn ci -m 'Something new'
  Adding         somethingnew
  Transmitting file data .
  Committed revision 13.
  $ cd ..
  $ rm -rf tempwc
  $ svn co "$SVNREPOURL/branch"@10 recreated
  A    recreated/somethingold (glob)
  Checked out revision 10.
  $ echo "recreated =        [svn]       $SVNREPOURL/branch" >> .hgsub
  $ hg ci -m addsub
  $ cd recreated
  $ svn up -q
  $ cd ..
  $ hg ci -m updatesub
  $ hg up -r-2
  D    *recreated/somethingnew (glob)
  A    *recreated/somethingold (glob)
  Checked out revision 10.
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ test -f recreated/somethingold

Test archive

  $ hg archive -S ../archive-all --debug
  archiving: 0/2 files (0.00%)
  archiving: .hgsub 1/2 files (50.00%)
  archiving: .hgsubstate 2/2 files (100.00%)
  archiving (obstruct): 0/1 files (0.00%)
  archiving (obstruct): 1/1 files (100.00%)
  archiving (recreated): 0/1 files (0.00%)
  archiving (recreated): 1/1 files (100.00%)
  archiving (s): 0/2 files (0.00%)
  archiving (s): 1/2 files (50.00%)
  archiving (s): 2/2 files (100.00%)

  $ hg archive -S ../archive-exclude --debug -X **old
  archiving: 0/2 files (0.00%)
  archiving: .hgsub 1/2 files (50.00%)
  archiving: .hgsubstate 2/2 files (100.00%)
  archiving (obstruct): 0/1 files (0.00%)
  archiving (obstruct): 1/1 files (100.00%)
  archiving (recreated): 0 files
  archiving (s): 0/2 files (0.00%)
  archiving (s): 1/2 files (50.00%)
  archiving (s): 2/2 files (100.00%)
  $ find ../archive-exclude | sort
  ../archive-exclude
  ../archive-exclude/.hg_archival.txt
  ../archive-exclude/.hgsub
  ../archive-exclude/.hgsubstate
  ../archive-exclude/obstruct
  ../archive-exclude/obstruct/other
  ../archive-exclude/s
  ../archive-exclude/s/alpha
  ../archive-exclude/s/dir
  ../archive-exclude/s/dir/epsilon.py

Test forgetting files, not implemented in svn subrepo, used to
traceback

#if no-windows
  $ hg forget 'notafile*'
  notafile*: No such file or directory
  [1]
#else
  $ hg forget 'notafile'
  notafile: * (glob)
  [1]
#endif

Test a subrepo referencing a just moved svn path. Last commit rev will
be different from the revision, and the path will be different as
well.

  $ cd "$WCROOT"
  $ svn up > /dev/null
  $ mkdir trunk/subdir branches
  $ echo a > trunk/subdir/a
  $ svn add trunk/subdir branches
  A         trunk/subdir (glob)
  A         trunk/subdir/a (glob)
  A         branches
  $ svn ci -m addsubdir
  Adding         branches
  Adding         trunk/subdir (glob)
  Adding         trunk/subdir/a (glob)
  Transmitting file data .
  Committed revision 14.
  $ svn cp -m branchtrunk $SVNREPOURL/trunk $SVNREPOURL/branches/somebranch
  
  Committed revision 15.
  $ cd ..

  $ hg init repo2
  $ cd repo2
  $ svn co $SVNREPOURL/branches/somebranch/subdir
  A    subdir/a (glob)
  Checked out revision 15.
  $ echo "subdir = [svn] $SVNREPOURL/branches/somebranch/subdir" > .hgsub
  $ hg add .hgsub
  $ hg ci -m addsub
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up
  A    *subdir/a (glob)
  Checked out revision 15.
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
