  $ setconfig extensions.treemanifest=!

  $ setup() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > amend=
  > [experimental]
  > evolution=createmarkers
  > EOF
  > }
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
  $ cd first_client
  $ setup
  $ cd ..

Setup the second client
  $ hg clone ssh://user@dummy/repo second_client -q
  $ cd second_client
  $ setup
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
