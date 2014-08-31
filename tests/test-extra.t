Test that extra metadata (renames, copies, and other extra metadata) roundtrips
across from hg to git
  $ . "$TESTDIR/testutil"

  $ git init -q gitrepo
  $ cd gitrepo
  $ touch a
  $ git add a
  $ fn_git_commit -ma
  $ git checkout -b not-master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "not-master"

  $ cd ..
  $ hg clone -q gitrepo hgrepo
  $ cd hgrepo
  $ hg mv a b
  $ fn_hg_commit -mb
  $ hg up 0 | egrep -v '^\(leaving bookmark master\)$'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch c
  $ hg add c
  $ fn_hg_commit -mc

Rebase will add a rebase_source
  $ hg --config extensions.rebase= rebase -s 1 -d 2
  saved backup bundle to $TESTTMP/*.hg (glob)
  $ hg up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Add a commit with multiple extra fields
  $ touch d
  $ hg add d
  $ fn_hg_commitextra --field zzzzzzz=datazzz --field aaaaaaa=dataaaa
  $ hg log --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n"
  @  3 f15e01c73845392d86a5ed10fb0753d09bca13d3
  |  aaaaaaa=dataaaa branch=default zzzzzzz=datazzz
  |
  o  2 dcec77c6ae3cff594c4435e5820bec4ec9e57440 b
  |  branch=default rebase_source=bb8ddb1031b5d9afd7caa5aa9d24c735222e3636
  |
  o  1 003b36e9c3993ac4319eeebd5f77a1d5306ba706 c
  |  branch=default
  |
  o  0 ab83abcbf5717f738191aa2d42f52a7100ce06a8 a
     branch=default
  

  $ hg bookmark b1
  $ hg push -r b1
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 3 commits with 3 trees and 0 blobs
  adding reference refs/heads/b1

  $ cd ../gitrepo
  $ git cat-file commit b1
  tree 1b773a2eb70f29397356f8069c285394835ff85a
  parent 99316cce06b9b5aa9e5a3f4df124939583791dda
  author test <none@none> 1167609613 +0000
  committer test <none@none> 1167609613 +0000
  
  
  
  --HG--
  extra : aaaaaaa : dataaaa
  extra : zzzzzzz : datazzz

  $ cd ..
  $ hg clone -q gitrepo hgrepo2
  $ cd hgrepo2
  $ hg log --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n"
  @  3 f15e01c73845392d86a5ed10fb0753d09bca13d3
  |  aaaaaaa=dataaaa branch=default zzzzzzz=datazzz
  |
  o  2 dcec77c6ae3cff594c4435e5820bec4ec9e57440 b
  |  branch=default rebase_source=bb8ddb1031b5d9afd7caa5aa9d24c735222e3636
  |
  o  1 003b36e9c3993ac4319eeebd5f77a1d5306ba706 c
  |  branch=default
  |
  o  0 ab83abcbf5717f738191aa2d42f52a7100ce06a8 a
     branch=default
  
