  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/morestatus.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [morestatus]
  > show=True
  > [extensions]
  > morestatus=$TESTTMP/morestatus.py
  > EOF

Test An empty repo should return no extra output
  $ hg init repo
  $ cd repo
  $ hg status

Test status on histedit stop
  $ echo "histedit=" >> $HGRCPATH
  $ echo "fbhistedit=$(echo $(dirname $TESTDIR))/fbhistedit.py" >> $HGRCPATH
  $ echo 'a' > a
  $ hg commit -Am 'a' -q
  $ hg histedit -q --commands - . 2> /dev/null << EOF
  > stop cb9a9f314b8b a
  > EOF
  [1]
  $ hg status
  
  # The repository is in an unfinished *histedit* state.

Test disabling output. Nothing should be shown
  $ hg status --config morestatus.show=False
  $ HGPLAIN=1 hg status
  $ hg histedit -q --continue

Test no output on normal state
  $ hg status

Test bisect state
  $ hg bisect --good
  $ hg status
  
  # The repository is in an unfinished *bisect* state.
  $ hg bisect --reset

Test graft state
  $ hg up -q -r 0
  $ echo '' > a
  $ hg commit -q -m 'remove content'

  $ hg up -q -r 0
  $ echo 'ab' > a
  $ hg commit -q -m 'add content'
  $ hg graft -q 2977a57
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *graft* state.

Get out of graft state
  $ hg up --clean -q
  $ hg status
  ? a.orig
  $ rm a.orig

Test unshelve state
  $ echo "reset=" >> $HGRCPATH
  $ echo "shelve=" >> $HGRCPATH
  $ hg reset .^ -q
  reseting without an active bookmark
  devel-warn: transaction with no lock at: /data/users/cdelahousse/hg-repo/mercurial/repair.py:161 (strip)
  $ hg shelve -q
  $ hg up -r 2977a57 -q
  $ hg unshelve -q
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *unshelve* state.
  $ hg unshelve --abort
  rebase aborted
  unshelve of 'default' aborted

  $ rm a.orig

Test rebase state
  $ echo "rebase=" >> $HGRCPATH
  $ hg up -r 0 -q
  $ echo 'ab' > a
  $ hg commit -q -m 'add content'
  $ hg rebase -s 2977a57 -d . -q
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *rebase* state.
