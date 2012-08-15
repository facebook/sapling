  $ fileset() {
  >   hg debugfileset "$@"
  > }

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ echo a > a2
  $ echo b > b1
  $ hg ci -Am addfiles
  adding a1
  adding a2
  adding b1

Test operators and basic patterns

  $ fileset a1
  a1
  $ fileset 'a*'
  a1
  a2
  $ fileset '"re:a\d"'
  a1
  a2
  $ fileset 'a1 or a2'
  a1
  a2
  $ fileset 'a1 | a2'
  a1
  a2
  $ fileset 'a* and "*1"'
  a1
  $ fileset 'a* & "*1"'
  a1
  $ fileset 'not (r"a*")'
  b1
  $ fileset '! ("a*")'
  b1
  $ fileset 'a* - a1'
  a2

