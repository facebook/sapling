#chg-compatible

  $ configure evolution
  $ enable amend
  $ disable treemanifest

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Setup the first client
  $ hg clone ssh://user@dummy/repo first_client -q

Setup the second client
  $ hg clone ssh://user@dummy/repo second_client -q
  $ cd second_client
  $ mkcommit commit
  $ hg log -r . -T '{node}\n'
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  $ mkcommit commit2
  $ hg cloud backup -q
  $ mkcommit commitwithbook
  $ hg push -r . --to scratch/commit --create -q
  $ hg up -q null
  $ mkcommit onemorecommit
  $ hg log -r . -T '{node}\n'
  94f1e8b68592fbdd8e8606b6426bbd075a59c94c
  $ hg up -q null
  $ mkcommit commitonemorebookmark
  $ hg push -r . --to scratch/commit2 --create -q
  $ mkcommit commitfullhash
  $ FULLHASH="$(hg log -r . -T '{node}')"
  $ echo "$FULLHASH"
  d15d0da9f84a9bebe6744eba3ec1dd86e2d46818
  $ hg cloud backup -q
  $ cd ..

Change the paths: 'default' path should be incorrect, but 'infinitepush' path should be correct
`hg up` should nevertheless succeed
  $ cd first_client
  $ hg paths
  default = ssh://user@dummy/repo
  $ cat << EOF >> .hg/hgrc
  > [paths]
  > default=ssh://user@dummy/broken
  > infinitepush=ssh://user@dummy/repo
  > EOF
  $ hg paths
  default = ssh://user@dummy/broken
  infinitepush = ssh://user@dummy/repo
  $ hg up 7e6a6fd9c7c8c8 -q
  '7e6a6fd9c7c8c8' does not exist locally - looking for it remotely...
  '7e6a6fd9c7c8c8' found remotely
  pull finished in * sec (glob)

Same goes for updating to a bookmark
  $ hg up scratch/commit -q
  'scratch/commit' does not exist locally - looking for it remotely...
  'scratch/commit' found remotely
  pull finished in * sec (glob)

Now try to pull it
  $ hg pull -r 94f1e8b68592 -q

Now change the paths again try pull with no parameters. It should use default path
  $ cat << EOF >> .hg/hgrc
  > [paths]
  > default=ssh://user@dummy/repo
  > infinitepush=ssh://user@dummy/broken
  > EOF
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found

Now try infinitepushbookmark path
  $ cat << EOF >> .hg/hgrc
  > [paths]
  > default=ssh://user@dummy/broken
  > infinitepush=ssh://user@dummy/broken
  > infinitepushbookmark=ssh://user@dummy/repo
  > EOF
  $ hg pull -B scratch/commit2
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Update by full hash - infinitepush path should be used
  $ cat << EOF >> .hg/hgrc
  > [paths]
  > default=ssh://user@dummy/broken
  > infinitepush=ssh://user@dummy/repo
  > infinitepushbookmark=ssh://user@dummy/broken
  > EOF
  $ hg update "$FULLHASH"
  'd15d0da9f84a9bebe6744eba3ec1dd86e2d46818' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  'd15d0da9f84a9bebe6744eba3ec1dd86e2d46818' found remotely
  pull finished in * sec (glob)
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (leaving bookmark scratch/commit)
