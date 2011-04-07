  $ hg init a
  $ cd a
  $ hg init b
  $ echo x > b/x

Should print nothing:

  $ hg add b
  $ hg st

Should fail:

  $ hg st b/x
  abort: path 'b/x' is inside nested repo 'b'
  [255]
  $ hg add b/x
  abort: path 'b/x' is inside nested repo 'b'
  [255]

Should fail:

  $ hg add b b/x
  abort: path 'b/x' is inside nested repo 'b'
  [255]
  $ hg st

Should arguably print nothing:

  $ hg st b

  $ echo a > a
  $ hg ci -Ama a

Should fail:

  $ hg mv a b
  abort: path 'b/a' is inside nested repo 'b'
  [255]
  $ hg st

