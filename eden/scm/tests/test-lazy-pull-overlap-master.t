#chg-compatible
#debugruntest-compatible

This test checks the pull works when:

1. Reassigning non-master group happens.
2. Master moved. Need to resolve a lazy vertex, and the server returns the new master.

And the client does not limit itself to only be able to resolve ancestors of
the old master in that case.

  $ configure modern
  $ setconfig paths.default=test:e1 ui.ssh=false
  $ shorttraceback

Reduce discovery sample size so we avoid triggering resolving lazy hashes to expose issues.

  $ setconfig discovery.full-sample-size=2 discovery.initial-sample-size=1

Test that the local lazy repo has commits in the "non-master" group that
overlaps with the "master" group.

Prepare Repo:

  $ newremoterepo repo1
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > H
  > |
  > G
  > |
  > F
  > |
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ hg push -r $E --to master --create -q

Clone the lazy repo (up to master):

  $ hg clone -U --shallow test:e1 --config remotefilelog.reponame=x $TESTTMP/cloned1 -q

Move server-side bookmarks forward:

  $ hg push -r $H --to master -q

Create commits in the client repo. Make them overlap with the server-side master group:

  $ cd $TESTTMP/cloned1
  $ DAG_SKIP_FLUSH_VERTEXES=$B LOG=dag::cache=info hg debugdrawdag << EOS
  > F       Z
  > |       |
  > master  $B
  > EOS
   INFO dag::cache: skip flushing 112478962961147124edd43549aedd1a335e44bf-1 to IdMap set by DAG_SKIP_FLUSH_VERTEXES
   INFO dag::cache: skip flushing 112478962961147124edd43549aedd1a335e44bf-1 to IdMap set by DAG_SKIP_FLUSH_VERTEXES

Pull:

  $ hg pull -B master
  pulling from test:e1
  searching for changes
