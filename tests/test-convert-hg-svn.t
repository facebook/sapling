
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80
  $ fix_path()
  > {
  >     tr '\\' /
  > }
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH
  $ echo "mq = " >> $HGRCPATH
  $ svnpath=`pwd | fix_path`/svn-repo
  $ svnadmin create "$svnpath"
  $ cat > "$svnpath"/hooks/pre-revprop-change <<EOF
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
  $ chmod +x "$svnpath"/hooks/pre-revprop-change
  $ 
  $ # SVN wants all paths to start with a slash. Unfortunately,
  $ # Windows ones don't. Handle that.
  $ svnurl="$svnpath"
  $ expr "$svnurl" : "\/" > /dev/null || svnurl="/$svnurl"
  $ svnurl="file://$svnurl"
  $ svn co "$svnurl" "$svnpath"-wc
  Checked out revision 0.
  $ cd "$svnpath"-wc
  $ echo a > a
  $ svn add a
  A         a
  $ svn ci -m'added a' a
  Adding         a
  Transmitting file data .
  Committed revision 1.
  $ cd ..

initial roundtrip

  $ hg convert -s svn -d hg "$svnpath"-wc "$svnpath"-hg | grep -v initializing
  scanning source...
  sorting...
  converting...
  0 added a
  $ hg convert -s hg -d svn "$svnpath"-hg "$svnpath"-wc
  scanning source...
  sorting...
  converting...

second roundtrip should do nothing

  $ hg convert -s svn -d hg "$svnpath"-wc "$svnpath"-hg
  scanning source...
  sorting...
  converting...
  $ hg convert -s hg -d svn "$svnpath"-hg "$svnpath"-wc
  scanning source...
  sorting...
  converting...

new hg rev

  $ hg clone "$svnpath"-hg "$svnpath"-work
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd "$svnpath"-work
  $ echo b > b
  $ hg add b
  $ hg ci -mb

adding an empty revision

  $ hg qnew -m emtpy empty
  $ hg qfinish -a
  $ cd ..

echo hg to svn

  $ hg --cwd "$svnpath"-hg pull -q "$svnpath"-work
  $ hg convert -s hg -d svn "$svnpath"-hg "$svnpath"-wc
  scanning source...
  sorting...
  converting...
  1 b
  0 emtpy

svn back to hg should do nothing

  $ hg convert -s svn -d hg "$svnpath"-wc "$svnpath"-hg
  scanning source...
  sorting...
  converting...

hg back to svn should do nothing

  $ hg convert -s hg -d svn "$svnpath"-hg "$svnpath"-wc
  scanning source...
  sorting...
  converting...
