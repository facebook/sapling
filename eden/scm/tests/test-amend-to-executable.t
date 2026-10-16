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
  $ sl manifest -v -r 'desc("add executable")'
  755 * executable
  644   target
  $ sl manifest -v
  755 * executable
  644   target
  $ sl cat -r . executable
  one
  two
  $ sl cat -r 'desc("add executable")' executable
  one
  $ sl cat -r 'desc(target)' target
  target
  amended

Replay additions, flag clears, and flag changes on existing files.

#if symlink
  $ newclientrepo
  $ drawdag <<'EOS'
  > C  # C/tail=tail\n
  > |
  > B  # B/exec-set=file\n (executable)
  > |  # B/exec-clear=file\n
  > |  # B/link-add=target (symlink)
  > |  # B/link-set=target (symlink)
  > |  # B/link-clear=target
  > A  # A/exec-set=file\n
  >    # A/exec-clear=file\n (executable)
  >    # A/link-set=target
  >    # A/link-clear=target (symlink)
  >    # A/target=target\n
  >    # drawdag.defaultfiles=false
  > EOS
  $ sl goto -q $C
  $ echo amended >> target
  $ sl amend --to 'desc(A)'
  $ sl status
  $ sl manifest -v -r 'desc(B)'
  644   exec-clear
  755 * exec-set
  644 @ link-add
  644   link-clear
  644 @ link-set
  644   target
  $ sl manifest -v
  644   exec-clear
  755 * exec-set
  644 @ link-add
  644   link-clear
  644 @ link-set
  644   tail
  644   target
  $ sl cat -r . target
  target
  amended
#endif
