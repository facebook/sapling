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

Literal match ending in " (re)":

  $ echo 'foo (re)'
  foo (re)

Exit code:

  $ false
  [1]
