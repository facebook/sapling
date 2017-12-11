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
  abort: uncommitted changes in subrepository "subrepo"
  [255]

Deleted files trigger a '+' marker in top level repos.  Deleted files are also
noticed by `update --check` in the top level repo.

  $ hg ci -Sqm 'add b'
  $ echo change > subrepo/b

  $ hg ci -Sm 'change b'
  committing subrepository subrepo

  $ rm a
  $ hg id
  9bfe45a197d7+ tip
  $ hg sum
  parent: 4:9bfe45a197d7 tip
   change b
  branch: default
  commit: 1 deleted (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 5 draft

  $ hg up --check -r '.^'
  abort: uncommitted changes
  [255]
  $ hg st -S
  ! a
  $ hg up -Cq .

Test that dirty is consistent through subrepos

  $ rm subrepo/b

A deleted subrepo file is flagged as dirty, like the top level repo

  $ hg id --config extensions.blackbox= --config blackbox.dirty=True
  9bfe45a197d7+ tip
  $ cat .hg/blackbox.log
  * @9bfe45a197d7b0ab09bf287729dd57e9619c9da5+ (*)> serve --cmdserver chgunix * (glob) (chg !)
  * @9bfe45a197d7b0ab09bf287729dd57e9619c9da5+ (*)> id --config *extensions.blackbox=* --config *blackbox.dirty=True* (glob)
  * @9bfe45a197d7b0ab09bf287729dd57e9619c9da5+ (*)> id --config *extensions.blackbox=* --config *blackbox.dirty=True* exited 0 * (glob)

TODO: a deleted file should be listed as such, like the top level repo

  $ hg sum
  parent: 4:9bfe45a197d7 tip
   change b
  branch: default
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 5 draft

Modified subrepo files are noticed by `update --check` and `summary`

  $ echo mod > subrepo/b
  $ hg st -S
  M subrepo/b

  $ hg up -r '.^' --check
  abort: uncommitted changes in subrepository "subrepo"
  [255]

  $ hg sum
  parent: 4:9bfe45a197d7 tip
   change b
  branch: default
  commit: 1 subrepos
  update: 1 new changesets, 2 branch heads (merge)
  phases: 5 draft

TODO: why is -R needed here?  If it's because the subrepo is treated as a
discrete unit, then this should probably warn or something.
  $ hg revert -R subrepo --no-backup subrepo/b -r .

  $ rm subrepo/b
  $ hg st -S
  ! subrepo/b

`hg update --check` notices a subrepo with a missing file, like it notices a
missing file in the top level repo.

  $ hg up -r '.^' --check
  abort: uncommitted changes in subrepository "subrepo"
  [255]

  $ hg up -r '.^' --config ui.interactive=True << EOF
  > d
  > EOF
  other [destination] changed b which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? d
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

XXX: There's a difference between wdir() and '.', so there should be a status.
`hg files -S` from the top is also missing 'subrepo/b'.

  $ hg st -S
  $ hg st -R subrepo
  $ hg files -R subrepo
  [1]
  $ hg files -R subrepo -r '.'
  subrepo/b

  $ hg bookmark -r tip @other
  $ echo xyz > subrepo/c
  $ hg ci -SAm 'add c'
  adding subrepo/c
  committing subrepository subrepo
  created new head
  $ rm subrepo/c

Merge sees deleted subrepo files as an uncommitted change

  $ hg merge @other
  abort: uncommitted changes in subrepository "subrepo"
  [255]
