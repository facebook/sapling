  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > backups=$TESTDIR/../hgext3rd/backups.py
  > strip=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo

Test backups list and recover

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -l $1
  > }
  $ mkcommit a
  $ mkcommit b
  $ hg strip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d2ae7f538514-2953539b-backup.hg (glob)
  $ hg backups
  Recover changesets using: hg backups --recover <changeset hash>
  
  * (glob)
  d2ae7f538514 b

  $ hg backups --config experimental.evolution=createmarkers
  Marker creation is enabled so no changeset should be
  * (glob)
  stripped changesets. If you are trying to recover a changeset hidden from a
  previous command, use hg journal to get its sha1 and you will be able to access
  it directly without recovering a backup.Recover changesets using: hg backups --recover <changeset hash>
  
  * (glob)
  d2ae7f538514 b
  $ hg backups --recover d2ae7f538514
  Unbundling d2ae7f538514
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
