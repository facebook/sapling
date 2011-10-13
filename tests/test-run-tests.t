Simple commands:

  $ echo foo
  foo
  $ printf 'oh no'
  oh no (no-eol)
  $ printf 'bar\nbaz\n' | cat
  bar
  baz

Multi-line command:

  $ foo() {
  >     echo bar
  > }
  $ foo
  bar

Doctest commands:

  >>> print 'foo'
  foo
  $ echo interleaved
  interleaved
  >>> for c in 'xyz':
  ...     print c
  x
  y
  z
  >>> print
  <BLANKLINE>

Regular expressions:

  $ echo foobarbaz
  foobar.* (re)
  $ echo barbazquux
  .*quux.* (re)

Globs:

  $ printf '* \\foobarbaz {10}\n'
  \* \\fo?bar* {10} (glob)

Literal match ending in " (re)":

  $ echo 'foo (re)'
  foo (re)

Exit code:

  $ (exit 1) 
  [1]
