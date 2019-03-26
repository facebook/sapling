  $ setconfig blackbox.track=perftrace tracing.threshold=0
  $ newrepo
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ enable blackbox

  $ hg up $C --config tracing.stderr=True
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0.0  hg up 26805aba1e600a82e93661149f2313866a221a7b --config 'tracing.stderr=True' (*) (glob)
  1.0
  
  $ cat .hg/blackbox.log
  1970/01/01 00:00:00 * @26805aba1e600a82e93661149f2313866a221a7b (*)> Trace: (glob)
  0.0  hg up 26805aba1e600a82e93661149f2313866a221a7b --config 'tracing.stderr=True' (*) (glob)
  1.0
  
