Test functionality is present

  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTTMP/fbamend.py
  > EOF

  $ hg help commit | grep -- --fixup
      --fixup               (with --amend) rebase children commits from a
  $ hg help commit | grep -- --rebase
      --rebase              (with --amend) rebases children commits after the
  $ hg help amend
  hg amend [OPTION]...
  
  amend the current commit with more changes
  
  options ([+] can be repeated):
  
   -e --edit                prompt to edit the commit message
      --rebase              rebases children commits after the amend
      --fixup               rebase children commits from a previous amend
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
  
  (some details hidden, use --verbose to show complete help)

Test basic functions

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg commit -m 'a'
  $ echo b >> b
  $ hg add b
  $ hg commit -m 'b'
  $ hg up .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ hg amend
  warning: the commit's children were left behind (use hg amend --fixup to rebase them)
  $ hg amend --fixup
  rebasing the children of bbb36c6acd42(preamend)
  rebasing 1:d2ae7f538514 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d2ae7f538514-2953539b-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/cb9a9f314b8b-cc5ccb0b-preamend-backup.hg (glob)
  $ echo a >> a
  $ hg amend --rebase
  rebasing the children of a4365b3108cc(preamend)
  rebasing 1:dfec26c56fa2 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/dfec26c56fa2-aff347bb-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/bbb36c6acd42-b715c760-preamend-backup.hg (glob)

Test that the extension disables itself when evolution is enabled

  $ cat > ${TESTTMP}/obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

noisy warning

  $ hg version 2>&1
  fbamend and evolve extension are imcompatible, fbamend deactivated.
  You can either disable it globally:
  - type `hg config --edit`
  - drop the `fbamend=` line from the `[extensions]` section
  or disable it for a specific repo:
  - type `hg config --local --edit`
  - add a `fbamend=!$TESTTMP/fbamend.py` line in the `[extensions]` section
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-2014 Matt Mackall and others
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

commit has no new flags

  $ hg help commit 2> /dev/null | grep -- --fixup
  [1]
  $ hg help commit 2> /dev/null | grep -- --rebase
  [1]

The amend command is missing

  $ hg help amend
  fbamend and evolve extension are imcompatible, fbamend deactivated.
  You can either disable it globally:
  - type `hg config --edit`
  - drop the `fbamend=` line from the `[extensions]` section
  or disable it for a specific repo:
  - type `hg config --local --edit`
  - add a `fbamend=!$TESTTMP/fbamend.py` line in the `[extensions]` section
  abort: no such help topic: amend
  (try "hg help --keyword amend")
  [255]
