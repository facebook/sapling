
#require no-eden

#inprocess-hg-incompatible

  $ enable rebase commitextras megarepo
  $ setconfig megarepo.lossy-commit-action=abort

  $ newclientrepo

  $ touch A
  $ sl commit -Aqm A
  $ sl go -q null
  $ touch B
  $ sl commit -Aqm B
  $ touch C
  $ sl commit -Aqm C --extra created_by_lossy_conversion=

  $ sl backout -r .
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ sl backout -r . --config megarepo.lossy-commit-action=ignore
  removing C
  changeset 21d06d5633a6 backs out changeset 57c4b16efbb2

  $ sl log -G -T '{node|short} {desc} {join(extras, ",")}'
  @  21d06d5633a6 Back out "C"
  │
  │  Original commit changeset: 57c4b16efbb2 branch=default
  o  57c4b16efbb2 C branch=default,created_by_lossy_conversion=
  │
  o  8ee0aac3fbd0 B branch=default
  
  o  a24b40a3340f A branch=default


  $ sl rebase -r 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 -d a24b40a3340fbcaa7e652fbc03d3f2a6958db4c7
  rebasing 57c4b16efbb2 "C"
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ sl rebase --abort
  rebase aborted

  $ sl rebase -r 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 -d a24b40a3340fbcaa7e652fbc03d3f2a6958db4c7 --config megarepo.lossy-commit-action=warn
  rebasing 57c4b16efbb2 "C"
  warning: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8

  $ sl log -G -T '{node|short} {desc} {join(extras, ",")}'
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


  $ sl go -q --config commit.reject-modifying-obsolete=false 57c4b16efbb2
  $ sl amend -m nope
  warning: changing an old version of a commit will diverge your stack:
  - 57c4b16efbb2 -> ccf2db2c8709 (rebase)
  proceed with amend (Yn)?  y
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

  $ sl go -q a24b40a3340f
  $ sl graft 57c4b16efbb2
  grafting 57c4b16efbb2 "C"
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]


  $ sl metaedit -r 57c4b16efbb2 -m nope
  abort: operating on lossily synced commit 57c4b16efbb23b68cbef2f5748e20688a1ebb5f8 disallowed by default
  (perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass)
  [255]

No infinite loop with autopull + titles namespace

  $ sl commit -m 'remote/foo commit' --config ui.allowemptycommit=1
  $ sl log -r remote/foo
  pulling 'foo' from 'test:repo1_server'
  abort: unknown revision 'remote/foo'!
  [255]
