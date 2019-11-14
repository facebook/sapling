Push merge commits from a treeonly shallow repo to a hybrid treemanifest server

  $ setconfig remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh"
  $ setconfig format.aggressivemergedeltas=True
  $ setconfig treemanifest.flatcompat=False

  $ newrepo server
  $ setconfig treemanifest.server=True
  $ enable pushrebase treemanifest

  $ newrepo client
  $ echo remotefilelog >> .hg/requires
  $ enable treemanifest remotefilelog pushrebase remotenames
  $ setconfig treemanifest.sendtrees=True treemanifest.treeonly=True
  $ setconfig paths.default=ssh://user@dummy/server
  $ drawdag <<'EOS'
  > D
  > |\
  > B E   # E/F2 = F (renamed from F)
  > | |   # B/A2 = A (renamed from A)
  > A F
  > EOS

  $ hg push --to foo --create -r $D -f  ssh://user@dummy/server
  pushing rev 5a587c09248a to destination ssh://user@dummy/server bookmark foo
  searching for changes
  remote: pushing 5 changesets:
  remote:     426bada5c675  A
  remote:     a6661b868de9  F
  remote:     9f93d39c36cf  B
  remote:     fc0baf5da824  E
  remote:     5a587c09248a  D
  exporting bookmark foo

Verify the renames are preserved (commit hashes did not change)

  $ cd $TESTTMP/server
  $ hg log -r "::$D" -G -T "{desc} {bookmarks}"
  o    D foo
  |\
  | o  E
  | |
  o |  B
  | |
  | o  F
  |
  o  A
  
  $ setconfig treemanifest.treeonly=True

Push a commit that client1 doesnt have
  $ cd ..
  $ newrepo client2
  $ echo remotefilelog >> .hg/requires
  $ enable treemanifest remotefilelog pushrebase remotenames
  $ setconfig treemanifest.sendtrees=True treemanifest.treeonly=True
  $ setconfig paths.default=ssh://user@dummy/server
  $ hg pull
  pulling from ssh://user@dummy/server
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 6 files
  new changesets 426bada5c675:5a587c09248a
  $ hg up -q tip
- Add a bunch of files, to force it to choose to make a delta
  $ echo >> file1
  $ echo >> file2
  $ echo >> file3
  $ echo >> file4
  $ echo >> file5
  $ mkdir mydir
  $ echo >> mydir/fileX
  $ echo >> mydir/fileY
  $ hg commit -Aqm "Add mydir/fileX & mydir/fileY"
  $ hg push --to foo
  pushing rev 0cfa18081ea6 to destination ssh://user@dummy/server bookmark foo
  searching for changes
  remote: pushing 1 changeset:
  remote:     0cfa18081ea6  Add mydir/fileX & mydir/fileY
  updating bookmark foo

Push treeonly merge commit to a treeonly server
  $ cd $TESTTMP/client
  $ hg up -q tip
  $ mkdir -p mydir/subdir
  $ echo X >> mydir/subdir/file
  $ hg commit -Aqm "Edit file"
  $ hg up -q 'tip^'
  $ mkdir -p mydir/subdir
  $ echo X >> mydir/subdir/file2
  $ hg commit -Aqm "Edit file2"
  $ hg merge -q 5
  $ hg commit -m "Merge 2"
  $ hg push --to foo ssh://user@dummy/server 2>&1
  pushing rev b634a5228cef to destination ssh://user@dummy/server bookmark foo
  searching for changes
  remote: pushing 3 changesets:
  remote:     a1d68bae23ee  Edit file
  remote:     54deb28e5abb  Edit file2
  remote:     b634a5228cef  Merge 2
  remote: 4 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 9 changes to 9 files
  3 new obsolescence markers
  updating bookmark foo
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved
  obsoleted 3 changesets
