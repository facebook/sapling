  $ "$TESTDIR/hghave" svn13 || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

fn to create new repository, and cd into it
  $ mkrepo() {
  >     hg init $1
  >     cd $1
  >     hg qinit
  > }


handle svn subrepos safely

  $ svnadmin create svn-repo-2499
  $ curpath=`pwd | tr '\\\\' /`
  $ expr "$svnpath" : "\/" > /dev/null
  > if [ $? -ne 0 ]; then
  >   curpath="/$curpath"
  > fi
  $ svnurl="file://$curpath/svn-repo-2499/project"
  $ mkdir -p svn-project-2499/trunk
  $ svn import -m 'init project' svn-project-2499 "$svnurl"
  Adding         svn-project-2499/trunk
  
  Committed revision 1.

qnew on repo w/svn subrepo
  $ mkrepo repo-2499-svn-subrepo
  $ svn co "$svnurl"/trunk sub
  Checked out revision 1.
  $ echo 'sub = [svn]sub' >> .hgsub
  $ hg add .hgsub
  $ hg status -S -X '**/format'
  A .hgsub
  $ hg qnew -m0 0.diff
  committing subrepository sub
  $ cd sub
  $ echo a > a
  $ svn add a
  A         a
  $ svn st
  A*    a (glob)
  $ cd ..
  $ hg status -S        # doesn't show status for svn subrepos (yet)
  $ hg qnew -m1 1.diff
  abort: uncommitted changes in subrepository sub
  [255]
