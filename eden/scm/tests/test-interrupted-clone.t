#debugruntest-incompatible
#require no-windows

  $ setconfig clone.use-rust=true
  $ export LOG=cmdclone=trace,atexit=debug,warn
  $ FAILPOINTS='run::clone=sleep(5000)' hg clone -Uq test:e1 failure &>output &
Wait for clone to hit failpoint.
  $ while ! grep -q 'performing rust clone' output; do sleep 0.1; done
  $ kill %1
  $ wait
Make sure repo was cleaned up.
  $ find failure && cat output
  find: *: $ENOENT$ (glob)
  [1]
