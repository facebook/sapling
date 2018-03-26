#require symlink execbit
  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > amend=
  > perfsuite=
  > rebase=
  > [perfsuite]
  > rebasedistance=1
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
  $ cd repo2
  $ hg up -Cq tip
  $ hg perftestsuite --seed 1 --traceback
  ran 'commit' in * sec (glob)
  ran 'amend' in * sec (glob)
  ran 'status' in * sec (glob)
  ran 'revert' in * sec (glob)
  ran 'rebase' in * sec (glob)
  ran 'pull' in * sec (glob)
