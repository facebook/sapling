#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh
  $ disable treemanifest
  $ enable infinitepush
  $ setconfig remotenames.hoist=default 'remotenames.autopullhoistpattern=re:.*'
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ scratchnodes() {
  >    for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
  >        echo ${node##*/}
  >    done
  > }
  $ scratchbookmarks() {
  >    for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
  >        echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  >    done
  > }

Create server repo with one commit and one remote bookmark
  $ hg init repo
  $ cd repo
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk infinitepush.storetype=disk
  $ mkcommit servercommit
Let's make server bookmark to match scratch pattern and
check that it won't be handled like scratch bookmark
  $ hg book scratch/serverbook
  $ cd ..

Clone server and enable remotenames
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client -q
  $ cd client
  $ enable remotenames
  $ hg book --remote
     default/scratch/serverbook ac312cb08db5

Push scratch commit and scratch bookmark
  $ mkcommit scratchcommitwithremotenames
  $ hg push --config extensions.remotenames= -r . --to scratch/mybranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     620472ff5c0c  scratchcommitwithremotenames
  $ hg book --remote
     default/scratch/mybranch  620472ff5c0c
     default/scratch/serverbook ac312cb08db5
  $ hg book
  no bookmarks set
  $ hg -R ../repo log -G
  @  commit:      ac312cb08db5
     bookmark:    scratch/serverbook
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     servercommit
  
  $ scratchnodes
  620472ff5c0c4a560a3ffd98c07f0c9ecad33f64
  $ scratchbookmarks
  scratch/mybranch 620472ff5c0c4a560a3ffd98c07f0c9ecad33f64
  $ cd ..

Clone server one more time and pull scratch bookmark. Make sure it is remote
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client2 -q
  $ cd client2
  $ enable remotenames
  $ hg book --remote
     default/scratch/serverbook ac312cb08db5
  $ hg pull -B scratch/mybranch
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg book --remote
     default/scratch/mybranch  620472ff5c0c
     default/scratch/serverbook ac312cb08db5
  $ hg book
  no bookmarks set

Make sure that next non-scratch pull doesn't override remote scratch bookmarks
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  $ hg book --remote
     default/scratch/mybranch  620472ff5c0c
     default/scratch/serverbook ac312cb08db5
  $ cd ..

Create one more branch head on the server
  $ cd repo
  $ mkcommit head1
  $ hg up ac312cb08db5
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark scratch/serverbook)
  $ mkcommit head2
  $ hg log -G
  @  commit:      dc4b2ecb723b
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     head2
  │
  │ o  commit:      64d557aa86fd
  ├─╯  bookmark:    scratch/serverbook
  │    user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     head1
  │
  o  commit:      ac312cb08db5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     servercommit
  
  $ cd ..

Go back to client, make pull and make sure that we pulled remote branches
  $ cd client
  $ hg dbsh -c 'ui.write("".join(sorted(repo.svfs.readutf8("remotenames").splitlines(True))))'
  620472ff5c0c4a560a3ffd98c07f0c9ecad33f64 bookmarks default/scratch/mybranch
  ac312cb08db5366e622a01fd001e583917eb9f1c bookmarks default/scratch/serverbook
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg dbsh -c 'ui.write("".join(sorted(repo.svfs.readutf8("remotenames").splitlines(True))))'
  620472ff5c0c4a560a3ffd98c07f0c9ecad33f64 bookmarks default/scratch/mybranch
  64d557aa86fdc42384b193f7eab99059da84f1f0 bookmarks default/scratch/serverbook
  $ cd ..

Push from another client, make sure that push updates other remote bookmarks as well (like "serverbook")
  $ cd client2
  $ hg up scratch/serverbook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit newscratch
  $ hg push -r . --to scratch/secondbranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     36667a3f76e4  newscratch
  $ hg book --remote
     default/scratch/mybranch  620472ff5c0c
     default/scratch/secondbranch 36667a3f76e4
     default/scratch/serverbook 64d557aa86fd
  $ hg book
  no bookmarks set

Try to push with remotebookmarks disabled
  $ hg push --config remotenames.bookmarks=False -r . --to scratch/secondbranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     36667a3f76e4  newscratch
  $ hg book
  no bookmarks set

Create new bookmark and try to pull it
  $ mkcommit newcommittoupdate1
  $ hg push -q -r . --to scratch/branchtoupdateto1 --create
  $ hg up -q ".^"
  $ mkcommit newcommittoupdate2
  $ hg push -q -r . --to scratch/branchtoupdateto2 --create
  $ hg up -q ".^"
  $ mkcommit newcommittopull
  $ hg push -q -r . --to scratch/branchtopull --create
  $ cd ../client
  $ hg up default/scratch/branchtoupdateto1
  pulling 'default/scratch/branchtoupdateto1', 'scratch/branchtoupdateto1' from 'ssh://user@dummy/repo'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ cat >> $HGRCPATH << EOF
  > [remotenames]
  > hoist=remote
  > rename.default=remote
  > EOF

  $ hg up remote/scratch/branchtoupdateto2
  pulling 'remote/scratch/branchtoupdateto2', 'scratch/branchtoupdateto2' from 'ssh://user@dummy/repo'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test hiding of a bookmark
  $ enable amend
  $ setconfig remotenames.selectivepull=True
  $ hg book --list-subscriptions
     default/scratch/branchtoupdateto1 2885148f6198
     default/scratch/mybranch  620472ff5c0c
     default/scratch/serverbook 64d557aa86fd
     remote/scratch/branchtoupdateto2 1f558bd20eaa
  $ hg hide .
  hiding commit 1f558bd20eaa "newcommittoupdate2"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 36667a3f76e4
  1 changeset hidden
  unsubscribing remote bookmark "remote/scratch/branchtoupdateto2"
  1 remote bookmark unsubscribed
  $ hg pull -B scratch/branchtoupdateto2
  pulling from ssh://user@dummy/repo
  $ hg hide -B remote/scratch/branchtoupdateto2
  hiding commit 1f558bd20eaa "newcommittoupdate2"
  hiding commit 36667a3f76e4 "newscratch"
  hiding commit ac312cb08db5 "servercommit"
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  working directory now at 000000000000
  3 changesets hidden
  unsubscribing remote bookmark "remote/scratch/branchtoupdateto2"
  1 remote bookmark unsubscribed
  $ hg book --list-subscriptions
     default/scratch/branchtoupdateto1 2885148f6198
     default/scratch/mybranch  620472ff5c0c
     default/scratch/serverbook 64d557aa86fd
