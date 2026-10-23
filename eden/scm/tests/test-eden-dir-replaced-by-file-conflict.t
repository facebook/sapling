#require eden no-windows

#testcases rustcheckout pythoncheckout

#if rustcheckout
  $ setconfig checkout.use-rust=true
#endif

#if pythoncheckout
  $ setconfig checkout.use-rust=false
#endif

Test that goto treats a tracked directory replaced locally by a file as a
changed/deleted conflict when the destination commit removes the directory.

  $ setconfig ui.origbackuppath=$TESTTMP/origbackups
  $ newserver server
  $ drawdag <<EOS
  > B # B/d/x.txt = (removed)
  > |
  > A # A/d/x.txt = x\n
  > EOS

  $ cd
  $ newclientrepo client server
  $ sl go -q $A
  $ sl pull -q -r $B

Replace the tracked directory with a file:
  $ rm -r d
  $ echo local > d
  $ sl status
  ! d/x.txt
  ? d

Plain goto refuses to proceed:
#if rustcheckout
  $ sl go $B
  abort: 1 conflicting file changes:
   d
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]
#endif
#if pythoncheckout
  $ sl go $B
  abort: conflicting changes:
    d
  (commit or goto --clean to discard changes)
  [255]
#endif

Merge and leave the conflict unresolved:
  $ sl go --merge $B --config ui.interactive=true <<EOF
  > u
  > EOF
  local [working copy] changed d which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges
  [1]
  $ sl resolve --list
  U d
  $ cat d
  local

Resolve by keeping the local file:
  $ sl resolve --tool :local d
  (no more unresolved files)
  continue: sl goto --continue
  $ sl resolve --list
  R d
  $ sl status
  A d
  $ cat d
  local

Start over and resolve by deleting the file:
  $ sl go -qC $A
  $ rm -r d
  $ echo local > d
  $ sl go --merge $B --config ui.interactive=true <<EOF
  > u
  > EOF
  local [working copy] changed d which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges
  [1]
  $ sl resolve --tool :other d
  (no more unresolved files)
  continue: sl goto --continue
  $ sl resolve --list
  R d
  $ sl status
  $ ls
  A
  B

Answer the prompt by keeping the local file:
  $ sl go -qC $A
  $ rm -r d
  $ echo local > d
  $ sl go --merge $B --config ui.interactive=true <<EOF
  > c
  > EOF
  local [working copy] changed d which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ sl status
  A d
  $ cat d
  local

Answer the prompt by deleting the file:
  $ sl go -qC $A
  $ rm -r d
  $ echo local > d
  $ sl go --merge $B --config ui.interactive=true <<EOF
  > d
  > EOF
  local [working copy] changed d which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? d
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ sl status
  $ ls
  A
  B
