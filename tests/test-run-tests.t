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

Return codes before inline python:

  $ sh -c 'exit 1'
  [1]

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

testing hghave

  $ "$TESTDIR/hghave" true
  $ "$TESTDIR/hghave" false
  skipped: missing feature: nail clipper
  [1]
  $ "$TESTDIR/hghave" no-true
  skipped: system supports yak shaving
  [1]
  $ "$TESTDIR/hghave" no-false

Conditional sections based on hghave:

#if true
  $ echo tested
  tested
#else
  $ echo skipped
#endif

#if false
  $ echo skipped
#else
  $ echo tested
  tested
#endif

Exit code:

  $ (exit 1) 
  [1]
