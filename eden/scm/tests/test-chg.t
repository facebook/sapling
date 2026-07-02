#require chg linux no-eden

  $ newclientrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl go -q $B
  $ CHGDEBUG=1 sl files 2>&1 | grep 'request runcommand'
  chg: debug: *request runcommand* (glob)
