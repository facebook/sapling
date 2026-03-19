
#require eden no-windows

#testcases rustcheckout pythoncheckout

#if rustcheckout
  $ setconfig checkout.use-rust=true
#endif

#if pythoncheckout
  $ setconfig checkout.use-rust=false
#endif

Test that checkout handles untracked files that collide with tracked
directories in the destination commit.

  $ newserver server
  $ drawdag <<EOS
  > B # B/ignored/link/file = content\n
  > | # B/ignored/regular/file = content\n
  > | # B/unignored/file = content\n
  > |
  > A # A/.gitignore = ignored/\n
  > EOS

  $ cd
  $ newclientrepo client server
  $ hg go -q $A

Create ignored untracked files - a symlink and a regular file:
  $ mkdir $TESTTMP/target ignored
  $ echo file > $TESTTMP/target/file
  $ ln -s $TESTTMP/target ignored/link
  $ echo regular > ignored/regular

Create unignored untracked file:
  $ echo unignored > unignored

Verify they are ignored:
  $ hg status
  ? unignored

Checkout to commit B where the untracked files have all become directories.
The ignored files should be silently replaced, but the unignored file should
cause a conflict:

  $ hg go -q $B
  abort: unignored: local file conflicts with a directory in the destination commit
  [255]

Remove the unignored file and retry:
  $ rm unignored
  $ hg go -q $B
  $ hg st
  $ cat ignored/link/file
  content
  $ cat ignored/regular/file
  content
