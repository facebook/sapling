  $ enable commitcloud rebase
  $ setconfig infinitepush.branchpattern="re:scratch/.+"

  $ newclientrepo
  $ drawdag <<EOS
  > B C
  > |/
  > A
  > EOS

Uses SLAPI even with push.edenapi=false:
  $ hg push -r $A --to scratch/test --create --config push.edenapi=false
  pushing rev 426bada5c675 to destination eager:$TESTTMP/repo1_server bookmark scratch/test
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  creating remote bookmark scratch/test

Fast forward - okay:
  $ hg push -r $B --to scratch/test
  pushing rev 112478962961 to destination eager:$TESTTMP/repo1_server bookmark scratch/test
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark scratch/test from 426bada5c675 to 112478962961

Don't try push rebase:
  $ hg push -r $C --to scratch/test
  pushing rev dc0947a82db8 to destination eager:$TESTTMP/repo1_server bookmark scratch/test
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  abort: non-fast-forward push to remote bookmark scratch/test from 112478962961 to dc0947a82db8
  (add '--force' or set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  [255]

  $ hg push -r $C --to scratch/test --force
  pushing rev dc0947a82db8 to destination eager:$TESTTMP/repo1_server bookmark scratch/test
  moving remote bookmark scratch/test from 112478962961 to dc0947a82db8

  $ log() {
  > hg log -G -T "{node|short} '{desc|firstline}' {remotenames} {join(mutations % '(Rewritten using {operation} into {join(successors % \'{node|short}\', \', \')})', ' ')}" "$@"
  > }

  $ log
  o  dc0947a82db8 'C' remote/scratch/test
  │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'

  $ hg rebase -qr $C -d $B

  $ log
  o  bbfdd6cb49aa 'C'
  │
  │ x  dc0947a82db8 'C' remote/scratch/test (Rewritten using rebase into bbfdd6cb49aa)
  │ │
  o │  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'

  $ hg push -q -r bbfdd6cb49aa --to scratch/test --force

dc0947a82db8 should not be visible anymore:
  $ log
  o  bbfdd6cb49aa 'C' remote/scratch/test
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'

  $ hg push --delete scratch/test
  deleting remote bookmark scratch/test
