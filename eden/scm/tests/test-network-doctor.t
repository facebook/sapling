#chg-compatible

  $ touch $TESTTMP/stub

  $ setconfig experimental.network-doctor=True paths.default=mononoke://169.254.1.2/foo
Set up fake cert paths so we don't hit "missing certs" error.
  $ setconfig auth.test.cert=$TESTTMP/stub auth.test.key=$TESTTMP/stub auth.test.priority=1 auth.test.prefix=mononoke://*

  $ hg init repo && cd repo

  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed
  pulling from mononoke://169.254.1.2/foo
  abort: command failed due to network error (see * for details) (glob)
  
  Please check your VPN connection (internet okay, but can't reach corp).
  [1]


  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed --verbose
  pulling from mononoke://169.254.1.2/foo
  abort: command failed due to network error (see * for details) (glob)
  
  Please check your VPN connection (internet okay, but can't reach corp).
    no corp connectivity: TCP error: test
  [1]


  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed --debug
  pulling from mononoke://169.254.1.2/foo
  abort: command failed due to network error (see * for details) (glob)
  
  Please check your VPN connection (internet okay, but can't reach corp).
    no corp connectivity: TCP error: test
  
  Original error:
  failed to connect to 169.254.1.2:443
   reason: * (glob)
   cn:     169.254.1.2
   cert:   $TESTTMP/stub
   key:    $TESTTMP/stub
  
  [1]


Works for native rust commands as well.
  $ hg clone fb://banana --config commands.force-rust=clone --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed
  Cloning reponame-default into $TESTTMP/repo/reponame-default
  abort: command failed due to network error
  
  Please check your VPN connection (internet okay, but can't reach corp).
  
  Details:
  
  NoCorp(TCP(Custom { kind: Other, error: "test" }))
  
  Original error:
  
  Network Error: [6] Couldn't resolve host name (Could not resolve host: test_fail)
  
  Caused by:
      [6] Couldn't resolve host name (Could not resolve host: test_fail)
  
  [255]

