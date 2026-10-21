The nth first-parent ancestor `x~n` for an n beyond the dag binding's u64
range crashes with an uncaught OverflowError. The correct result is the
empty set, as for any n larger than the chain (e.g. `.~5` below).

  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg up -q $C

Control: in-range n, including the largest u64 and an n larger than the chain.

  $ hg log -r '.~1' -T '{desc}\n'
  B
  $ hg log -r '.~2' -T '{desc}\n'
  A
  $ hg log -r '.~0' -T '{desc}\n'
  C
  $ hg log -r '.~5' -T '{desc}\n'
  $ hg log -r '.~18446744073709551615' -T '{desc}\n'
  $ hg log -r '.~-5' -T '{desc}\n'

Bug: n = 2**64 (and anything larger) crashes instead of returning the empty set.

  $ hg log -r '.~18446744073709551616' -T '{desc}\n' 2>&1 | tail -1
  OverflowError: int too big to convert
  $ hg log -r '.~6646139978924579364519035301401722880000000' -T '{desc}\n' 2>&1 | tail -1
  OverflowError: int too big to convert
  $ hg log -r '(. + .^)~18446744073709551616' -T '{desc}\n' 2>&1 | tail -1
  OverflowError: int too big to convert
  $ hg log -r 'all() - .~18446744073709551616' -T '{desc}\n' 2>&1 | tail -1
  OverflowError: int too big to convert

Errors in the base set are still reported first, whatever n is.

  $ hg log -r 'nonexistent~1' -T '{desc}\n'
  abort: unknown revision 'nonexistent'!
  [255]
  $ hg log -r 'nonexistent~18446744073709551616' -T '{desc}\n'
  abort: unknown revision 'nonexistent'!
  [255]

The negative (children) branch has no fixed-width binding and does not crash.

  $ hg log -r '.~-18446744073709551616' -T '{desc}\n'
  $ hg log -r "$A~-1" -T '{desc}\n'
  B
  $ hg log -r "$A~-18446744073709551616" -T '{desc}\n'
