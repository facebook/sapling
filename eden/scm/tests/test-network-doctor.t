#chg-compatible

  $ touch $TESTTMP/stub

  $ setconfig experimental.network-doctor=True paths.default=mononoke://169.254.1.2/foo
Set up fake cert paths so we don't hit "missing certs" error.
  $ setconfig auth.test.cert=$TESTTMP/stub auth.test.key=$TESTTMP/stub auth.test.priority=1 auth.test.prefix=mononoke://*

  $ hg init repo && cd repo

  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed
  pulling from mononoke://169.254.1.2/foo
  
  Please check your VPN connection (internet okay, but can't reach corp).
  [1]


  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed --verbose
  pulling from mononoke://169.254.1.2/foo
  
  Please check your VPN connection (internet okay, but can't reach corp).
    no corp connectivity: TCP error: test
  [1]


  $ hg pull --config edenapi.url=https://test_fail/foo --config doctor.external-host-check-url=https://test_succeed --debug
  pulling from mononoke://169.254.1.2/foo
  
  Please check your VPN connection (internet okay, but can't reach corp).
    no corp connectivity: TCP error: test
  
  Original error:
  failed to connect to 169.254.1.2:443
   reason: * (glob)
   cn:     169.254.1.2
   cert:   $TESTTMP/stub
   key:    $TESTTMP/stub
  
  [1]

