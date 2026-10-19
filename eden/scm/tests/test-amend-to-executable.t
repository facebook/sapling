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

Amend an executable-bit change across a descendant content change.
FIXME: Native merge must combine the mode and content changes without panicking.
  $ newclientrepo
  $ setconfig diff.git=true
  $ echo one > file
  $ sl ci -Aqm A
  $ echo two >> file
  $ sl ci -m B
  $ chmod +x file
  $ sl amend --to 'desc(A)' > $TESTTMP/amend-error 2>&1
  [1]
  $ tail -1 $TESTTMP/amend-error
  SystemError: Rust panic: not implemented
  $ sl status
  M file
  $ sl manifest -v -r 'desc(A)'
  644   file
  $ sl manifest -v
  644   file
  $ sl cat -r . file
  one
  two

Amend a content change across a descendant clearing the executable bit.
FIXME: Native merge must combine the content and mode changes without panicking.
  $ newclientrepo
  $ setconfig diff.git=true
  $ echo one > file
  $ chmod +x file
  $ sl ci -Aqm A
  $ chmod -x file
  $ sl ci -m B
  $ echo two >> file
  $ sl amend --to 'desc(A)' > $TESTTMP/amend-error 2>&1
  [1]
  $ tail -1 $TESTTMP/amend-error
  SystemError: Rust panic: not implemented
  $ sl status
  M file
  $ sl manifest -v -r 'desc(A)'
  755 * file
  $ sl manifest -v
  644   file
  $ sl cat -r . file
  one

Internal amend patches preserve permissions, paths, and whitespace despite diff
preferences.
FIXME: User diff settings must not silently discard mode and whitespace changes.
  $ newclientrepo
  $ setconfig diff.git=false diff.noprefix=true diff.ignorews=true
  $ echo 'one two' > file
  $ sl ci -Aqm A
  $ chmod +x file
  $ echo 'one  two' > file
  $ sl amend --to .
  $ sl status
  M file
  $ sl manifest -v
  644   file
  $ sl cat -r . file
  one two
