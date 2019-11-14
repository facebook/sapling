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
  rebasing 65f3e88a53bc "B" (B)
  merging 2
  merging A
  merging 2 failed!
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg blackbox --no-timestamp --no-sid --pattern '{"watchman":"_"}' | egrep '(watchman.*state.*)'
  [watchman] command ["state-enter",{"metadata":{"distance":3,"merge":false,"partial":false,"rev":"0000000000000000000000000000000000000000","status":"ok"},"name":"hg.update"}] finished in 0 ms
  [watchman] command ["state-leave",{"metadata":{"distance":3,"merge":false,"partial":false,"rev":"2e2f27616b65209eecd4710c454df0f678f271d9","status":"ok"},"name":"hg.update"}] finished in 0 ms
  [watchman] command ["state-enter",{"metadata":{"distance":3,"merge":true,"partial":false,"rev":"2e2f27616b65209eecd4710c454df0f678f271d9","status":"ok"},"name":"hg.update"}] finished in 0 ms
  [watchman] command ["state-enter",{"metadata":{"path":"2"},"name":"hg.filemerge"}] finished in 0 ms
  [watchman] command ["state-leave",{"metadata":{"path":"2"},"name":"hg.filemerge"}] finished in 0 ms
  [watchman] command ["state-leave",{"metadata":{"distance":3,"merge":true,"partial":false,"rev":"65f3e88a53bc0f5183deea0cdbc46738777ec005","status":"ok"},"name":"hg.update"}] finished in 0 ms
