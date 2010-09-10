  $ "$TESTDIR/hghave" svn || exit 80

  $ fix_path()
  > {
  >     tr '\\' /
  > }

  $ escapedwd=`pwd | fix_path`

SVN wants all paths to start with a slash. Unfortunately, Windows ones
don't. Handle that.

  $ expr "$escapedwd" : "\/" > /dev/null
  $ if [ $? -ne 0 ]; then
  >     escapedwd="/$escapedwd"
  > fi
  $ escapedwd=`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$escapedwd"`
  $ filterpath="s|$escapedwd|/root|"
  $ filteroutofdate='s/ in transaction.*/ is out of date/;s/Out of date: /File /'

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

  $ echo "s = [svn]       $SVNREPO/src" >> .hgsub
  $ svn co --quiet "$SVNREPO"/src s
  $ hg add .hgsub
  $ hg ci -m1
  committing subrepository s

debugsub

  $ hg debugsub | sed "$filterpath"
  path s
   source   file:///root/svn-repo/src
   revision 2

change file in svn and hg, commit

  $ echo a >> a
  $ echo alpha >> s/alpha
  $ hg commit -m 'Message!' \
  >     | sed 's:Sending.*s/alpha:Sending        s/alpha:g'
  committing subrepository s
  Sending        s/alpha
  Transmitting file data .
  Committed revision 3.
  
  Fetching external item into '.*/s/externals'
  External at revision 1.
  
  At revision 3.
  $ hg debugsub | sed "$filterpath"
  path s
   source   file:///root/svn-repo/src
   revision 3

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
  
  Updated to revision 3.
  $ echo xyz >> alpha
  $ svn propset svn:mime-type 'text/xml' alpha
  property 'svn:mime-type' set on 'alpha'
  $ svn ci -m 'amend a from svn'
  Sending        src/alpha
  Transmitting file data .
  Committed revision 4.
  $ cd ../../sub/t

this commit from hg will fail

  $ echo zzz >> s/alpha
  $ hg ci -m 'amend alpha from hg' 2>&1 | sed "$filteroutofdate"
  committing subrepository s
  abort: svn: Commit failed (details follow):
  svn: File '/src/alpha' is out of date
  $ svn revert -q s/alpha

this commit fails because of meta changes

  $ svn propset svn:mime-type 'text/html' s/alpha
  property 'svn:mime-type' set on 's/alpha'
  $ hg ci -m 'amend alpha from hg' 2>&1 | sed "$filteroutofdate"
  committing subrepository s
  abort: svn: Commit failed (details follow):
  svn: File '/src/alpha' is out of date
  $ svn revert -q s/alpha

this commit fails because of externals changes

  $ echo zzz > s/externals/other
  $ hg ci -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals
  $ hg diff --subrepos -r 1:2 | grep -v diff
  --- a/.hgsubstate	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgsubstate	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -2 s
  +3 s
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  $ svn revert -q s/externals/other

this commit fails because of externals meta changes

  $ svn propset svn:mime-type 'text/html' s/externals/other
  property 'svn:mime-type' set on 's/externals/other'
  $ hg ci -m 'amend externals from hg'
  committing subrepository s
  abort: cannot commit svn externals
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
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tc

debugsub in clone

  $ hg debugsub | sed "$filterpath"
  path s
   source   file:///root/svn-repo/src
   revision 3

verify subrepo is contained within the repo directory

  $ python -c "import os.path; print os.path.exists('s')"
  True
