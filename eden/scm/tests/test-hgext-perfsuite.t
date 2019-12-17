#chg-compatible

  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
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
  stdout: ? i/s/o/aae
  ? t/y/c/aag
  ? u/r/l/aaa
  ? v/f/r/aab
  ? v/f/r/aaf
  
  stdout: adding i/s/o/aae
  adding t/y/c/aag
  adding u/r/l/aaa
  adding v/f/r/aab
  adding v/f/r/aaf
  
  ran 'commit' in * sec (glob)
  stdout: M v/f/r/aab
  ? m/h/f/aag
  ? u/r/l/aag
  ? v/f/r/aaa
  ? v/f/r/aac
  ? z/y/x/aae
  
  stdout: adding m/h/f/aag
  adding u/r/l/aag
  adding v/f/r/aaa
  adding v/f/r/aac
  adding z/y/x/aae
  
  stdout: saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/20b2121f9781-5b3ae32a-amend.hg
  
  ran 'amend' in * sec (glob)
  stdout: ! u/r/l/aaa
  ? h/o/v/aag
  ? i/s/o/aag
  ? t/y/c/aab
  ? t/y/c/aaf
  
  ran 'status' in * sec (glob)
  stdout: reverting repo3/u/r/l/aaa
  
  ran 'revert' in * sec (glob)
  stdout: rebasing b17a0147d61c "test commit"
  saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/b17a0147d61c-bd5f50f4-rebase.hg
  
  ran 'rebase' in * sec (glob)
  stdout: 1 files updated, 0 files merged, 10 files removed, 0 files unresolved
  (activating bookmark master)
  
  stdout: rebasing f8d4b0697695 "test commit"
  saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/f8d4b0697695-7388783a-rebase.hg
  
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
