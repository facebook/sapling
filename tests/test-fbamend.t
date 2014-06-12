#
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
  
  options:
  
   -e --edit                prompt to edit the commit message
      --rebase              rebases children commits after the amend
      --fixup               rebase children commits from a previous amend
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
  
  [+] marked option can be specified multiple times
  
  use "hg -v help amend" to show the global options

Test that the extension disable itself when evolution is enabled

  $ cat > ./obs.py << EOF
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
  Mercurial Distributed SCM (version 3.0.1+46-c00822e0b8ea)
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
