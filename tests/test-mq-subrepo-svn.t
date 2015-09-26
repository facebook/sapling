#require svn13

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > [diff]
  > nodates = 1
  > EOF

fn to create new repository, and cd into it
  $ mkrepo() {
  >     hg init $1
  >     cd $1
  >     hg qinit
  > }


handle svn subrepos safely

  $ svnadmin create svn-repo-2499

  $ SVNREPOPATH=`pwd`/svn-repo-2499/project
#if windows
  $ SVNREPOURL=file:///`$PYTHON -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#else
  $ SVNREPOURL=file://`$PYTHON -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#endif

  $ mkdir -p svn-project-2499/trunk
  $ svn import -qm 'init project' svn-project-2499 "$SVNREPOURL"

qnew on repo w/svn subrepo
  $ mkrepo repo-2499-svn-subrepo
  $ svn co "$SVNREPOURL"/trunk sub
  Checked out revision 1.
  $ echo 'sub = [svn]sub' >> .hgsub
  $ hg add .hgsub
  $ hg status -S -X '**/format'
  A .hgsub
  $ hg qnew -m0 0.diff
  $ cd sub
  $ echo a > a
  $ svn add a
  A         a
  $ svn st
  A*    a (glob)
  $ cd ..
  $ hg status -S        # doesn't show status for svn subrepos (yet)
  $ hg qnew -m1 1.diff
  abort: uncommitted changes in subrepository 'sub'
  [255]

  $ cd ..
