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
