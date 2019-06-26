#require fsmonitor

(Run this test using HGFSMONITOR_TESTS=1)

  $ newrepo

  $ hg debugdrawdag << EOS
  > C   # C/A=0\n1\n
  > |   # B/A=1\n2\n
  > | B # C/2=1\n
  > |/  # B/2=2\n
  > A   # A/A=1\n
  > EOS

  $ enable fsmonitor rebase hgevents
  $ setconfig blackbox.track=merge_resolve,watchman blackbox.logsource=true

  $ hg rebase -s B -d C --tool=false
  rebasing 1:65f3e88a53bc "B" (B)
  merging 2
  merging A
  merging 2 failed!
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"watchman"}}' | egrep '(watchman.*state.*completed|resolving)' | python $RUNTESTDIR/sortdictfilter.py
  [legacy][watchman] command ('state-enter', '$TESTTMP/repo1', {'metadata': {'distance': 3, 'merge': False, 'partial': False, 'rev': '0000000000000000000000000000000000000000', 'status': 'ok'}, 'name': 'hg.update'}) completed in 0.00 seconds
  [legacy][watchman] command ('state-leave', '$TESTTMP/repo1', {'metadata': {'distance': 3, 'merge': False, 'partial': False, 'rev': '2e2f27616b65209eecd4710c454df0f678f271d9', 'status': 'ok'}, 'name': 'hg.update'}) completed in 0.00 seconds
  [legacy][watchman] command ('state-enter', '$TESTTMP/repo1', {'metadata': {'distance': 3, 'merge': True, 'partial': False, 'rev': '2e2f27616b65209eecd4710c454df0f678f271d9', 'status': 'ok'}, 'name': 'hg.update'}) completed in 0.00 seconds
  [legacy][watchman] command ('state-enter', '$TESTTMP/repo1', {'metadata': {'path': '2'}, 'name': 'hg.filemerge'}) completed in 0.00 seconds
  [legacy][watchman] command ('state-leave', '$TESTTMP/repo1', {'metadata': {'path': '2'}, 'name': 'hg.filemerge'}) completed in 0.00 seconds
  [legacy][watchman] command ('state-leave', '$TESTTMP/repo1', {'metadata': {'distance': 3, 'merge': True, 'partial': False, 'rev': '65f3e88a53bc0f5183deea0cdbc46738777ec005', 'status': 'ok'}, 'name': 'hg.update'}) completed in 0.00 seconds
