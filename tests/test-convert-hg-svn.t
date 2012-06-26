
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH
  $ echo "mq = " >> $HGRCPATH

  $ SVNREPOPATH=`pwd`/svn-repo
#if windows
  $ SVNREPOURL=file:///`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#else
  $ SVNREPOURL=file://`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#endif

  $ svnadmin create "$SVNREPOPATH"
  $ cat > "$SVNREPOPATH"/hooks/pre-revprop-change <<EOF
  > #!/bin/sh
  > 
  > REPOS="$1"
  > REV="$2"
  > USER="$3"
  > PROPNAME="$4"
  > ACTION="$5"
  > 
  > if [ "$ACTION" = "M" -a "$PROPNAME" = "svn:log" ]; then exit 0; fi
  > if [ "$ACTION" = "A" -a "$PROPNAME" = "hg:convert-branch" ]; then exit 0; fi
  > if [ "$ACTION" = "A" -a "$PROPNAME" = "hg:convert-rev" ]; then exit 0; fi
  > 
  > echo "Changing prohibited revision property" >&2
  > exit 1
  > EOF
  $ chmod +x "$SVNREPOPATH"/hooks/pre-revprop-change
  $ svn co "$SVNREPOURL" "$SVNREPOPATH"-wc
  Checked out revision 0.
  $ cd "$SVNREPOPATH"-wc
  $ echo a > a
  $ svn add a
  A         a
  $ svn ci -m'added a' a
  Adding         a
  Transmitting file data .
  Committed revision 1.
  $ cd ..

initial roundtrip

  $ hg convert -s svn -d hg "$SVNREPOPATH"-wc "$SVNREPOPATH"-hg | grep -v initializing
  scanning source...
  sorting...
  converting...
  0 added a
  $ hg convert -s hg -d svn "$SVNREPOPATH"-hg "$SVNREPOPATH"-wc
  scanning source...
  sorting...
  converting...

second roundtrip should do nothing

  $ hg convert -s svn -d hg "$SVNREPOPATH"-wc "$SVNREPOPATH"-hg
  scanning source...
  sorting...
  converting...
  $ hg convert -s hg -d svn "$SVNREPOPATH"-hg "$SVNREPOPATH"-wc
  scanning source...
  sorting...
  converting...

new hg rev

  $ hg clone "$SVNREPOPATH"-hg "$SVNREPOPATH"-work
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd "$SVNREPOPATH"-work
  $ echo b > b
  $ hg add b
  $ hg ci -mb

adding an empty revision

  $ hg qnew -m emtpy empty
  $ hg qfinish -a
  $ cd ..

echo hg to svn

  $ hg --cwd "$SVNREPOPATH"-hg pull -q "$SVNREPOPATH"-work
  $ hg convert -s hg -d svn "$SVNREPOPATH"-hg "$SVNREPOPATH"-wc
  scanning source...
  sorting...
  converting...
  1 b
  0 emtpy

svn back to hg should do nothing

  $ hg convert -s svn -d hg "$SVNREPOPATH"-wc "$SVNREPOPATH"-hg
  scanning source...
  sorting...
  converting...

hg back to svn should do nothing

  $ hg convert -s hg -d svn "$SVNREPOPATH"-hg "$SVNREPOPATH"-wc
  scanning source...
  sorting...
  converting...
