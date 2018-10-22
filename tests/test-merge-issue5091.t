  $ hg init

Base

  $ cat << EOF > A
  > S
  > S
  > S
  > S
  > S
  > EOF

  $ hg ci -m Base -q -A A

Other

  $ cat << EOF > A
  > S
  > S
  > X
  > S
  > S
  > EOF

  $ hg ci -m Other -q
  $ hg bookmark -qir. other

Local

  $ hg up '.^' -q

  $ cat << EOF > A
  > S
  > S
  > S
  > X
  > S
  > S
  > S
  > EOF

  $ hg ci -m Local -q

If the diff algorithm tries to group multiple hunks into one. It will cause a
merge conflict in the middle.

  $ hg merge other -q -t :merge3
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  [1]

  $ cat A
  S
  S
  <<<<<<< working copy: 14ce9a1fcd1e - test: Local
  S
  X
  S
  ||||||| base
  S
  =======
  X
  >>>>>>> merge rev:    4171d1cf524c other - test: Other
  S
  S

In a more complex case, where hunks cannot be grouped together, the result will
look weird in xdiff's case but okay in bdiff's case where there is no conflict,
and everything gets auto resolved reasonably.

  $ rm -rf .hg
  $ hg init

  $ cat << EOF > A
  > S
  > S
  > Y
  > S
  > Y
  > S
  > S
  > EOF

  $ hg ci -m Base -q -A A

  $ cat << EOF > A
  > S
  > S
  > Y
  > X
  > Y
  > S
  > S
  > EOF

  $ hg ci -m Other -q
  $ hg bookmark -qir. other

  $ hg up '.^' -q

  $ cat << EOF > A
  > S
  > S
  > S
  > Y
  > X
  > Y
  > S
  > S
  > S
  > EOF

  $ hg ci -m Local -q

  $ hg merge other -q -t :merge3
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  [1]

  $ cat A
  S
  S
  <<<<<<< working copy: 057fc5d1a99c - test: Local
  S
  ||||||| base
  Y
  S
  =======
  Y
  X
  >>>>>>> merge rev:    f0ba17ae43c9 other - test: Other
  Y
  X
  Y
  S
  S
  S
