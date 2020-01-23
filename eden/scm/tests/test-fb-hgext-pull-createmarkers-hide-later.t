#chg-compatible

  $ disable treemanifest

Setup

  $ configure evolution dummyssh
  $ enable amend pullcreatemarkers pushrebase rebase remotenames
  $ setconfig ui.username="nobody <no.reply@fb.com>" experimental.rebaseskipobsolete=true
  $ setconfig remotenames.allownonfastforward=true

Test that hg pull creates obsolescence markers for landed diffs
  $ hg init server
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    [ -z "$2" ] || echo "Differential Revision: https://phabricator.fb.com/D$2" >> msg
  >    hg ci -l msg
  > }
  $ land_amend() {
  >    hg log -r. -T'{desc}\n' > msg
  >    echo "Reviewed By: someone" >> msg
  >    hg ci --amend -l msg
  > }

Set up server repository

  $ cd server
  $ mkcommit initial
  $ mkcommit secondcommit
  $ hg book master
  $ cd ..

Set up a client repository, and work on 3 diffs

  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ mkcommit b 123 # 123 is the phabricator rev number (see function above)
  $ mkcommit c 124
  $ mkcommit d 131
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}' -r 'all()'
  @  4 "add d
  |
  |  Differential Revision: https://phabricator.fb.com/D131"
  o  3 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D124"
  o  2 "add b
  |
  |  Differential Revision: https://phabricator.fb.com/D123"
  o  1 "add secondcommit" default/master
  |
  o  0 "add initial"
  

Now land the first two diff, but with amended commit messages, as would happen
when a diff is landed with landcastle.

  $ hg update -r 1
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg graft -r 2
  grafting 948715751816 "add b"
  $ land_amend
  $ hg graft -r 3
  grafting 0e229072f723 "add c"
  $ land_amend
  $ hg push -r . --to master
  pushing rev cc68f5e5f8d6 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 2 changesets:
  remote:     e0672eeeb97c  add b
  remote:     cc68f5e5f8d6  add c
  updating bookmark master

Strip the commits we just landed.

  $ hg update -r 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg debugstrip -r 6
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/e0672eeeb97c-d1aa7ddd-backup.hg (glob)

Here pull should now detect commits 2 and 3 as landed, but it won't be able to
hide them since there is a non-hidden successor.

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 2 files
  obsoleted 3 changesets
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}' -r 'all()'
  o  7 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D124
  |  Reviewed By: someone" default/master
  o  6 "add b
  |
  |  Differential Revision: https://phabricator.fb.com/D123
  |  Reviewed By: someone"
  | o  4 "add d
  | |
  | |  Differential Revision: https://phabricator.fb.com/D131"
  | x  3 "add c
  | |
  | |  Differential Revision: https://phabricator.fb.com/D124"
  | x  2 "add b
  |/
  |    Differential Revision: https://phabricator.fb.com/D123"
  @  1 "add secondcommit"
  |
  o  0 "add initial"
  
  $ hg log -T '{rev}\n' -r 'allsuccessors(2)'
  6
  $ hg log -T '{rev}\n' -r 'allsuccessors(3)'
  7

Now land the last diff.

  $ hg update -r 7
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg graft -r 4
  grafting e4b5974890c0 "add d"
  $ land_amend
  $ hg push -r . --to master
  pushing rev 296f9d37d5c1 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     296f9d37d5c1  add d
  updating bookmark master

And strip the commit we just landed.

  $ hg update -r 7
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugstrip -r 9
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/296f9d37d5c1-9c9e6ffd-backup.hg (glob)

Here pull should now detect commit 4 has been landed.  It should hide this
commit, and should also hide 3 and 2, which were previously landed, but up
until now had non-hidden successors.

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  obsoleted 2 changesets
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}' -r 'all()'
  o  9 "add d
  |
  |  Differential Revision: https://phabricator.fb.com/D131
  |  Reviewed By: someone" default/master
  @  7 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D124
  |  Reviewed By: someone"
  o  6 "add b
  |
  |  Differential Revision: https://phabricator.fb.com/D123
  |  Reviewed By: someone"
  o  1 "add secondcommit"
  |
  o  0 "add initial"
  
