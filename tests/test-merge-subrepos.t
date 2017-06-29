  $ hg init

  $ echo a > a
  $ hg ci -qAm 'add a'

  $ hg init subrepo
  $ echo 'subrepo = http://example.net/libfoo' > .hgsub
  $ hg ci -qAm 'added subrepo'

  $ hg up -qC 0
  $ echo ax > a
  $ hg ci -m 'changed a'
  created new head

  $ hg up -qC 1
  $ cd subrepo
  $ echo b > b
  $ hg add b
  $ cd ..

Should fail, since there are added files to subrepo:

  $ hg merge
  abort: uncommitted changes in subrepository 'subrepo'
  [255]

Deleted files trigger a '+' marker in top level repos.  Deleted files are also
noticed by `update --check` in the top level repo.

  $ hg ci -Sqm 'add b'
  $ rm a
  $ hg id
  cb66ec850af7+ tip
  $ hg sum
  parent: 3:cb66ec850af7 tip
   add b
  branch: default
  commit: 1 deleted (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 4 draft

  $ hg up --check -r '.^'
  abort: uncommitted changes
  [255]
  $ hg st -S
  ! a
  $ hg up -Cq .

Test that dirty is consistent through subrepos

  $ rm subrepo/b

TODO: a deleted subrepo file should be flagged as dirty, like the top level repo

  $ hg id
  cb66ec850af7 tip

TODO: a deleted file should be listed as such, like the top level repo

  $ hg sum
  parent: 3:cb66ec850af7 tip
   add b
  branch: default
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 4 draft

Modified subrepo files are noticed by `update --check` and `summary`

  $ echo mod > subrepo/b
  $ hg st -S
  M subrepo/b

  $ hg up -r '.^' --check
  abort: uncommitted changes in subrepository 'subrepo'
  [255]

  $ hg sum
  parent: 3:cb66ec850af7 tip
   add b
  branch: default
  commit: 1 subrepos
  update: 1 new changesets, 2 branch heads (merge)
  phases: 4 draft

TODO: why is -R needed here?  If it's because the subrepo is treated as a
discrete unit, then this should probably warn or something.
  $ hg revert -R subrepo --no-backup subrepo/b -r .

  $ rm subrepo/b
  $ hg st -S
  ! subrepo/b

TODO: --check should notice a subrepo with a missing file.  It already notices
a modified file.

  $ hg up -r '.^' --check
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

TODO: update without --clean shouldn't restore a deleted subrepo file, since it
doesn't restore a deleted top level repo file.
  $ hg st -S

  $ hg bookmark -r tip @other
  $ echo xyz > subrepo/c
  $ hg ci -SAm 'add c'
  adding subrepo/c
  committing subrepository subrepo
  created new head
  $ rm subrepo/c

Merge sees deleted subrepo files as an uncommitted change

  $ hg merge @other
   subrepository subrepo diverged (local revision: 2b4750dcc93f, remote revision: cde40f86152f)
  (M)erge, keep (l)ocal [working copy] or keep (r)emote [merge rev]? m
  abort: uncommitted changes (in subrepo subrepo)
  (use 'hg status' to list changes)
  [255]
