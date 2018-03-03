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

  $ cat A
  S
  S
  X
  S
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

  $ cat A
  S
  S
  S
  Y
  X
  Y
  S
  S
  S
