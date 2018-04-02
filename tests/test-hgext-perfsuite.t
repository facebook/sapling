#require symlink execbit
  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > amend=
  > perfsuite=
  > rebase=
  > [perfsuite]
  > rebase.masterdistance=1
  > immrebase.masterdistance=0
  > [remotefilelog]
  > reponame=test
  > EOF

  $ hg init repo1
  $ hg -R repo1 debugdrawdag <<'EOS'
  > d
  > |
  > c
  > |
  > b
  > |
  > a
  > EOS
  $ hg book -R repo1 -r d master
  $ hg clone repo1 repo2
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo2 perftestsuite --seed 1 --traceback
  ran 'commit' in * sec (glob)
  ran 'amend' in * sec (glob)
  ran 'status' in * sec (glob)
  ran 'revert' in * sec (glob)
  ran 'rebase' in * sec (glob)
  ran 'immrebase' in * sec (glob)
  ran 'pull' in * sec (glob)

--print
  $ hg clone repo1 repo3
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo3 perftestsuite --seed 1 --print
  stdout: M a
  M b
  M c
  
  ran 'commit' in * sec (glob)
  stdout: M a
  M b
  M c
  ! d
  
  stdout: removing d
  
  stdout: saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/78cee9739c2b-81996245-amend.hg
  
  ran 'amend' in * sec (glob)
  stdout: M a
  M b
  M c
  ? d
  
  ran 'status' in * sec (glob)
  stdout: reverting repo3/a
  reverting repo3/b
  reverting repo3/c
  
  ran 'revert' in * sec (glob)
  stdout: rebasing 4:40cde51ade58 "test commit" (tip)
  saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/40cde51ade58-07bec616-rebase.hg
  
  ran 'rebase' in * sec (glob)
  stdout: 4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)
  
  stdout: rebasing 4:45a785d41f50 "test commit" (tip)
  saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/45a785d41f50-016652a0-rebase.hg
  
  ran 'immrebase' in * sec (glob)
  stdout: pulling from $TESTTMP/repo1
  searching for changes
  no changes found
  
  ran 'pull' in * sec (glob)

--profile
  $ hg clone repo1 repo4
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo4 perftestsuite --seed 1 --use-profile --print 2>&1 | grep "Sample count"
  Sample count: * (glob)
  Sample count: * (glob)
  Sample count: * (glob)
  Sample count: * (glob)
  Sample count: * (glob)
  Sample count: * (glob)
  Sample count: * (glob)
