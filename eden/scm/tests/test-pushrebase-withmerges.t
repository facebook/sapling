#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest

Setup

  $ configure mutation-norecord dummyssh
  $ enable remotenames pushrebase
  $ setconfig ui.username="nobody <no.reply@fb.com>"

  $ commit() {
  >   echo $1 > $1
  >   hg add $1
  >   hg commit -m "$1"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
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
  $ hg merge 0fcb170b6d8413eccdcba882f30260c80a99ad19
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge alpha and beta"
  $ log
  @    merge alpha and beta [draft:b41b83f633d8]
  ├─╮
  │ o    merge beta [draft:45a8d60c53ab]
  │ ├─╮
  │ │ o  beta [draft:4f90fdc3a1aa]
  │ │
  o │  merge alpha [draft:0fcb170b6d84]
  ├─╮
  o │  alpha [draft:c85f9ce7b342]
    │
    o  base [public:d20a80d4def3]
  

Add a commit in the server

  $ cd ../server
  $ commit other
  $ log
  @  other [draft:7fd651906bb3] @
  │
  o  base [draft:d20a80d4def3]
  

Push in from the client.

  $ cd ../client
  $ hg push --to @
  pushing rev b41b83f633d8 to destination ssh://user@dummy/server bookmark @
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark @
  remote: pushing 5 changesets:
  remote:     c85f9ce7b342  alpha
  remote:     0fcb170b6d84  merge alpha
  remote:     4f90fdc3a1aa  beta
  remote:     45a8d60c53ab  merge beta
  remote:     b41b83f633d8  merge alpha and beta
  remote: 6 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @    merge alpha and beta [public:8c1abab9fd04]
  ├─╮
  │ o    merge alpha [public:a9138cc95bb3]
  │ ├─╮
  o │ │  merge beta [public:f71e1c3a925c]
  ├───╮
  │ │ o  other [public:7fd651906bb3]
  │ │ │
  o │ │  beta [public:4f90fdc3a1aa]
    │ │
    o │  alpha [public:c85f9ce7b342]
      │
      o  base [public:d20a80d4def3]
  
  $ test -f other
