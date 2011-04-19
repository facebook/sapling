Create a repository:

  $ hg init t
  $ cd t

Make a changeset:

  $ echo a > a
  $ hg add a
  $ hg commit -m test

This command is ancient:

  $ hg history
  changeset:   0:acb14030fe0a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Verify that updating to revision 0 via commands.update() works properly

  $ cat <<EOF > update_to_rev0.py
  > from mercurial import ui, hg, commands
  > myui = ui.ui()
  > repo = hg.repository(myui, path='.')
  > commands.update(myui, repo, rev=0)
  > EOF
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ python ./update_to_rev0.py
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg identify -n
  0
 

Poke around at hashes:

  $ hg manifest --debug
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a

  $ hg cat a
  a

Verify should succeed:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

At the end...
