Simple commands:

  $ echo foo
  foo
  $ echo 'bar\nbaz' | cat
  bar
  baz

Multi-line command:

  $ foo() {
  >     echo bar
  > }
  $ foo
  bar

Regular expressions:

  $ echo foobarbaz
  foobar.* (re)
  $ echo barbazquux
  .*quux.* (re)

Globs:

  $ echo '* \\foobarbaz {10}'
  \* \\fo?bar* {10} (glob)

Literal match ending in " (re)":

  $ echo 'foo (re)'
  foo (re)

Exit code:

  $ false
  [1]
