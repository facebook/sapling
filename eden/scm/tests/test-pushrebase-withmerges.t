#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ . helpers-usechg.sh

Setup

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > remotenames =
  > pushrebase =
  > [experimental]
  > evolution = createmarkers
  > EOF

  $ commit() {
  >   echo $1 > $1
  >   hg add $1
  >   hg commit -m "$1"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{rev}:{node|short}] {bookmarks}" "$@"
  > }

Set up server repository

  $ hg init server
  $ cd server
  $ commit base
  $ hg book @

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client

Build commit graph to push in

  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ commit alpha
  $ hg merge @
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge alpha"
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ commit beta
  $ hg merge @
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge beta"
  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge alpha and beta"
  $ log
  @    merge alpha and beta [draft:5:b41b83f633d8]
  |\
  | o    merge beta [draft:4:45a8d60c53ab]
  | |\
  | | o  beta [draft:3:4f90fdc3a1aa]
  | |
  o |  merge alpha [draft:2:0fcb170b6d84]
  |\|
  o |  alpha [draft:1:c85f9ce7b342]
   /
  o  base [public:0:d20a80d4def3]
  

Add a commit in the server

  $ cd ../server
  $ commit other
  $ log
  @  other [draft:1:7fd651906bb3] @
  |
  o  base [draft:0:d20a80d4def3]
  

Push in from the client.

  $ cd ../client
  $ hg push --to @
  pushing rev b41b83f633d8 to destination ssh://user@dummy/server bookmark @
  searching for changes
  remote: pushing 5 changesets:
  remote:     c85f9ce7b342  alpha
  remote:     0fcb170b6d84  merge alpha
  remote:     4f90fdc3a1aa  beta
  remote:     45a8d60c53ab  merge beta
  remote:     b41b83f633d8  merge alpha and beta
  remote: 6 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 1 changes to 3 files
  3 new obsolescence markers
  updating bookmark @
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  obsoleted 3 changesets
  $ log
  @    merge alpha and beta [public:9:8c1abab9fd04]
  |\
  | o    merge beta [public:8:f71e1c3a925c]
  | |\
  o---+  merge alpha [public:7:a9138cc95bb3]
  | | |
  | | o  other [public:6:7fd651906bb3]
  | | |
  | o |  beta [public:3:4f90fdc3a1aa]
  |  /
  o /  alpha [public:1:c85f9ce7b342]
   /
  o  base [public:0:d20a80d4def3]
  
  $ test -f other
