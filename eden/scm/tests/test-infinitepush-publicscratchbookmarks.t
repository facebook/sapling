#require no-eden


  $ enable commitcloud
  $ disable infinitepush
  $ setconfig remotenames.autopullhoistpattern='re:.*'
  $ setconfig infinitepush.branchpattern="re:scratch/.+"

  $ newserver server
  $ echo base > base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm commit1
  $ hg book master

  $ newclientrepo client1 test:server
  $ newclientrepo client2 test:server
  $ cd ../client1

Attempt to push a public commit to a scratch bookmark.  There is no scratch
data to push, but the bookmark should be accepted.

  $ hg push -q --to scratch/public --create -r . --traceback

Pull this bookmark in the other client
  $ cd ../client2
  $ hg up -q scratch/public
  $ hg log -r . -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" remote/master remote/scratch/public
  $ cd ../client1

Attempt to push a public commit to a real remote bookmark.  This should also
be accepted.

  $ hg push -q --to real-public --create -r .

Attempt to push a draft commit to a scratch bookmark.  This should still work.

  $ echo 2 > file
  $ hg commit -Aqm commit2
  $ hg push -q --to scratch/draft --create -r .

Check the server data is correct.

  $ hg bookmarks --cwd $TESTTMP/server
   * master                    e6c779c67aa9
     real-public               e6c779c67aa9
     scratch/draft             3f2e32144a89
     scratch/public            e6c779c67aa9

Make another public scratch bookmark on an older commit.

  $ hg up -q 'desc(base)'
  $ hg push -q --to scratch/other --create -r .

Make a new draft commit here, and push it to the other scratch bookmark.  This
works because the old commit is an ancestor of the new commit.

  $ echo a > other
  $ hg commit -Aqm other1
  $ hg push -q --to scratch/other -r . --force

  $ hg -R ../server book
   * master                    e6c779c67aa9
     real-public               e6c779c67aa9
     scratch/draft             3f2e32144a89
     scratch/other             8bebbb8c3ae7
     scratch/public            e6c779c67aa9

Try again with --non-forward-move.

  $ hg push -q --to scratch/public --force -r .

  $ hg -R ../server book
   * master                    e6c779c67aa9
     real-public               e6c779c67aa9
     scratch/draft             3f2e32144a89
     scratch/other             8bebbb8c3ae7
     scratch/public            8bebbb8c3ae7

Move the two bookmarks back to a public commit.

  $ hg push -q --to scratch/public --force -r 'desc(base)'
  $ hg push -q --to scratch/other --force -r 'desc(commit1)'

Update the public scratch bookmarks in the other client, using both -r and -B.

  $ cd ../client2
  $ hg log -r scratch/public -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" remote/master remote/scratch/public
  $ hg pull -qB scratch/public
  $ hg log -r scratch/public -T '{node|short} "{desc}" {remotebookmarks}\n'
  d20a80d4def3 "base" remote/scratch/public
  $ hg pull -qB scratch/other
  $ hg log -r scratch/other -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" remote/master remote/scratch/other
