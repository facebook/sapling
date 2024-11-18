  $ enable commitcloud
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

  $ hg push --delete scratch/test
  deleting remote bookmark scratch/test
