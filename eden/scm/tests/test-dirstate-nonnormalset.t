#chg-compatible

#chg-compatible

#chg-compatible

#testcases v0 v1 v2

#if v0
  $ setconfig format.dirstate=0
#endif

#if v1
  $ setconfig format.dirstate=1
#endif

#if v2
  $ setconfig format.dirstate=2
#endif

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate="{rev}:{node|short} ({phase}) [{tags} {bookmarks}] {desc|firstline}\n"
  > [extensions]
  > dirstateparanoidcheck = $TESTDIR/../contrib/dirstatenonnormalcheck.py
  > [experimental]
  > nonnormalparanoidcheck = True
  > [devel]
  > all-warnings=True
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }

  $ hg init testrepo
  $ cd testrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg status
