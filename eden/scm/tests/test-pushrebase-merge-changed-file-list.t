#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . $RUNTESTDIR/library.sh

Setup

  $ configure dummyssh
  $ enable remotenames
  $ setconfig ui.username="nobody <no.reply@fb.com>"

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {remotenames}" "$@"
  > }

Set up server repository

  $ newserver server
  $ setconfig extensions.pushrebase=
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book -r . master

Clone client repository
  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  1 trees fetched over 0.00s
  $ cd client
  $ setconfig extensions.pushrebase=
  $ setconfig extensions.remotenames=

Add new commit
  $ cd ../server
  $ hg up -q master
  $ echo 'bar' > a
  $ hg commit -Am 'a => bar'

Create a merge commit that merges executable file in
  $ cd ../client
  $ hg up -q tip
  $ log -r .
  @  initial [public:2bb9d20e471c] default/master
  
  $ hg up -q null
  $ echo ex > ex
  $ chmod +x ex
  $ hg ci -Aqm tomerge
  $ log -r .
  @  tomerge [draft:db9ca4f4d8f9]
  
  $ hg up -q 2bb9d20e471c
  $ hg merge -q db9ca4f4d8f9
  $ hg ci -m merge
  $ hg push -r . --to master -q
  1 trees fetched over 0.00s

Check that file list contains no changed files, because a file were just merged in
  $ hg up -q tip
  $ hg log -r . -T '{files}'

Create a merge commit that merges a file and then makes it executable
  $ cd ../server
  $ hg up -q master
  $ mkcommit randomservercommit

  $ cd ../client
  $ hg up -q null
  $ echo no_exec > no_exec
  $ hg ci -Aqm tomerge_no_exec
  $ hg log -r . -T '{node}'
  38806fbf9b2d528b5e65b29edbb249ace57ca52e (no-eol)

  $ hg up -q 2bb9d20e471c
  $ hg merge -q 38806fbf9b2d
  $ chmod +x no_exec
  $ hg ci -m merge

  $ hg push -r . --to master -q
  1 trees fetched over 0.00s
  $ hg up -q tip
  $ hg log -r . -T '{files}'
  no_exec (no-eol)

Create a merge commit that merges executable and non-executable files.
File list should be empty because we are keeping p1 flags
  $ cd ../server
  $ hg up -q master
  $ mkcommit randomservercommit2

  $ cd ../client
  $ hg up -q null
  $ echo no_exec_2 > no_exec_2
  $ hg ci -Aqm tomerge_no_exec

  $ hg log -r . -T '{node}'
  9c093b936a3cf120f340f16111bd80331029fd5c (no-eol)
  $ hg up -q master
  $ echo no_exec_2 > no_exec_2
  $ chmod +x no_exec_2
  $ hg commit -Aqm 'exec commit'
  $ hg merge -q 9c093b936a3cf120f340f16111bd80331029fd5c
  warning: cannot merge flags for no_exec_2 without common ancestor - keeping local flags

  $ hg ci -m merge
  $ hg push -q -r . --to master
  1 trees fetched over 0.00s
  $ hg up -q tip
  $ hg log -r . -T '{files}'

Create a merge commit that merges executable and non-executable files.
File list should be non-empty because we are keeping p2 flags
  $ cd ../server
  $ hg up -q master
  $ mkcommit randomservercommit3

  $ cd ../client
  $ hg up -q null
  $ echo no_exec_3 > no_exec_3
  $ hg ci -Aqm tomerge_no_exec

  $ hg log -r . -T '{node}'
  84cb2313f1da1968f526b51ea263f81a6a9b9b1c (no-eol)
  $ hg up -q master
  $ echo no_exec_3 > no_exec_3
  $ chmod +x no_exec_3
  $ hg commit -Aqm 'exec commit'
  $ hg merge -q 84cb2313f1da1968f526b51ea263f81a6a9b9b1c
  warning: cannot merge flags for no_exec_3 without common ancestor - keeping local flags
  $ chmod -x no_exec_3

  $ hg ci -m merge
  $ hg push -q -r . --to master
  1 trees fetched over 0.00s
  $ hg up -q tip
  $ hg log -r . -T '{files}'
  no_exec_3 (no-eol)
