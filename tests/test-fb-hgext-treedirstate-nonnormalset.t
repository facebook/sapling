Copy of test-dirstate-nonnormalsets.t for treedirstate

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > treedirstate=
  > [treedirstate]
  > useinnewrepos=True
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate="{rev}:{node|short} ({phase}) [{tags} {bookmarks}] {desc|firstline}\n"
  > [extensions]
  > dirstateparanoidcheck = $RUNTESTDIR/../contrib/dirstatenonnormalcheck.py
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
