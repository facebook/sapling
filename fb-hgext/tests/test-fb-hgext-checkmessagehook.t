  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > checkmessagehook = $TESTDIR/../hgext3rd/checkmessagehook.py
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -A -l $TESTDIR/ctrlchar-msg.txt
  adding a
  non-printable characters in commit message
  Line 5: 'This has a sneaky ctrl-A: \x01'
  Line 6: 'And this has esc: \x1b'
  transaction abort!
  rollback completed
  abort: pretxncommit.checkmessage hook failed
  [255]
  $ hg commit -A -l $TESTDIR/perfectlyok-msg.txt
  adding a
  $ hg log -r .
  changeset:   0:d9cf9881be7b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     This commit message is perfectly OK, and has no sneaky control characters.
  
