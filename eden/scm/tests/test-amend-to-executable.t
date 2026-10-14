#require no-windows execbit

Test replaying an executable file added and then modified in descendant commits
  $ newclientrepo
  $ echo target > target
  $ sl ci -m "target" -Aq
  $ echo one > executable
  $ chmod +x executable
  $ sl ci -m "add executable" -Aq
  $ echo two >> executable
  $ sl ci -m "modify executable" -Aq
  $ echo amended >> target
  $ sl amend --to "desc(target)"
  $ test -x executable
  $ cat executable
  one
  two
  $ sl status
