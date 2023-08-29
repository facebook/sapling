#debugruntest-compatible
#inprocess-hg-incompatible

  $ enable rebase commitextras megarepo
  $ setconfig megarepo.lossy-commit-action=abort

  $ configure modernclient
  $ newclientrepo

  $ touch A
  $ hg commit -Aqm A
  $ hg go -q null
  $ touch B
  $ hg commit -Aqm B
  $ touch C
  $ hg commit -Aqm C --extra created_by_lossy_conversion=

  $ hg backout -r .
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ hg backout -r . --config megarepo.lossy-commit-action=ignore
  removing C
  changeset 21d06d5633a6 backs out changeset 57c4b16efbb2

  $ hg log -G -T '{node|short} {desc} {join(extras, ",")}'
  @  21d06d5633a6 Back out "C"
  │
  │  Original commit changeset: 57c4b16efbb2 branch=default
  o  57c4b16efbb2 C branch=default,created_by_lossy_conversion=
  │
  o  8ee0aac3fbd0 B branch=default
  
  o  a24b40a3340f A branch=default


  $ hg rebase -r 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 -d a24b40a3340fbcaa7e652fbc03d3f2a6958db4c7
  rebasing 57c4b16efbb2 "C"
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ hg rebase --abort
  rebase aborted

  $ hg rebase -r 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 -d a24b40a3340fbcaa7e652fbc03d3f2a6958db4c7 --config megarepo.lossy-commit-action=warn
  rebasing 57c4b16efbb2 "C"
  warning: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8

  $ hg log -G -T '{node|short} {desc} {join(extras, ",")}'
  o  ccf2db2c8709 C branch=default,rebase_source=57c4b16efbb23b68cbef2f5748e20688a1ebb5f8
  │
  │ @  21d06d5633a6 Back out "C"
  │ │
  │ │  Original commit changeset: 57c4b16efbb2 branch=default
  │ x  57c4b16efbb2 C branch=default,created_by_lossy_conversion=
  │ │
  │ o  8ee0aac3fbd0 B branch=default
  │
  o  a24b40a3340f A branch=default


  $ hg go -q 57c4b16efbb2
  $ hg amend -m nope
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ hg go -q a24b40a3340f
  $ hg graft 57c4b16efbb2
  grafting 57c4b16efbb2 "C"
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]


  $ hg metaedit -r 57c4b16efbb2 -m nope
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]
