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
   0.0  hg up 26805aba1e600a82e93661149f2313866a221a7b --config 'tracing.stderr=True' (11.0s)
   1.0    Repo Setup (1.0s; local)
   3.0    Status (1.0s)
     :      * A/M/R Files: 0
   5.0    Calculate Updates (3.0s)
   6.0      Check Unknown Files (1.0s)
   9.0    Apply Updates (1.0s)
     :      * Actions: 3
     :      * Deleted Files: 0
     :      * Disk Writes: 3.0B (*) (glob)
     :      * Written Files: 3
  11.0
  
  $ cat .hg/blackbox.log
  1970/01/01 00:00:00 * @26805aba1e600a82e93661149f2313866a221a7b (*)> Trace: (glob)
   0.0  hg up 26805aba1e600a82e93661149f2313866a221a7b --config 'tracing.stderr=True' (11.0s)
   1.0    Repo Setup (1.0s; local)
   3.0    Status (1.0s)
     :      * A/M/R Files: 0
   5.0    Calculate Updates (3.0s)
   6.0      Check Unknown Files (1.0s)
   9.0    Apply Updates (1.0s)
     :      * Actions: 3
     :      * Deleted Files: 0
     :      * Disk Writes: 3.0B (*) (glob)
     :      * Written Files: 3
  11.0
  
