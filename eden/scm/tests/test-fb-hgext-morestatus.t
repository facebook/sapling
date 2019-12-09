#chg-compatible

  $ enable obsstore
  $ cat >> $HGRCPATH << EOF
  > [morestatus]
  > show=True
  > [extensions]
  > morestatus=
  > fbhistedit=
  > histedit=
  > rebase=
  > reset=
  > EOF
  $ cat >> $TESTTMP/breakupdate.py << EOF
  > import sys
  > from edenscm.mercurial import merge
  > def extsetup(ui):
  >     merge.applyupdates = lambda *args, **kwargs: sys.exit()
  > EOF
  $ breakupdate() {
  >   cat >> .hg/hgrc <<EOF
  > [extensions]
  > breakupdate=$TESTTMP/breakupdate.py
  > EOF
  > }
  $ unbreakupdate() {
  >   cat >> .hg/hgrc <<EOF
  > [extensions]
  > breakupdate=!
  > EOF
  > }

Test An empty repo should return no extra output
  $ hg init repo
  $ cd repo
  $ hg status

Test status on histedit stop
  $ echo 'a' > a
  $ hg commit -Am 'a' -q
  $ hg histedit -q --commands - . 2> /dev/null << EOF
  > stop cb9a9f314b8b a
  > EOF
  [1]
  $ hg status
  
  # The repository is in an unfinished *histedit* state.
  # To continue:                hg histedit --continue
  # To abort:                   hg histedit --abort

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
  # To mark the changeset good:    hg bisect --good
  # To mark the changeset bad:     hg bisect --bad
  # To abort:                      hg bisect --reset

Verify that suppressing a morestatus state warning works with the config knob:
  $ hg status --config morestatus.skipstates=bisect

Test hg status is normal after bisect reset
  $ hg bisect --reset
  $ hg status

Test graft state
  $ hg up -q -r 1
  $ echo '' > a
  $ hg commit -q -m 'remove content'

  $ hg up -q -r 1
  $ echo 'ab' > a
  $ hg commit -q -m 'add content'
  $ hg graft -q 2977a57
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *graft* state.
  # Unresolved merge conflicts:
  # 
  #     a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg graft --continue
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)

Test hg status is normal after graft abort
  $ hg up --clean -q
  $ hg status
  ? a.orig
  $ rm a.orig

Test unshelve state
  $ echo "shelve=" >> $HGRCPATH
  $ hg reset ".^" -q
  $ hg shelve -q
  $ hg up -r 2977a57 -q
  $ hg unshelve -q
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *unshelve* state.
  # Unresolved merge conflicts:
  # 
  #     a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg unshelve --continue
  # To abort:                   hg unshelve --abort

Test hg status is normal after unshelve abort
  $ hg unshelve --abort
  rebase aborted
  unshelve of 'default' aborted
  $ hg status
  ? a.orig
  $ rm a.orig

Test rebase state
  $ echo "rebase=" >> $HGRCPATH
  $ hg up -r 1 -q
  $ echo 'ab' > a
  $ hg commit -q -m 'add content'
  $ hg rebase -s 2977a57 -d . -q
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *rebase* state.
  # Unresolved merge conflicts:
  # 
  #     a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort

Test status in rebase state with resolved files
  $ hg resolve --mark a
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *rebase* state.
  # No unresolved merge conflicts.
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort

Test hg status is normal after rebase abort
  $ hg rebase --abort -q
  rebase aborted
  $ hg status
  ? a.orig
  $ rm a.orig

Test rebase with an interrupted update:
  $ breakupdate
  $ hg rebase -s 2977a57ce863 -d 79361b8cdbb5 -q
  $ unbreakupdate
  $ hg status
  
  # The repository is in an unfinished *rebase* state.
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort
  $ hg rebase --abort -q
  rebase aborted

Test conflicted merge state
  $ hg merge -q
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *merge* state.
  # Unresolved merge conflicts:
  # 
  #     a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)

Test if listed files have a relative path to current location
  $ mkdir -p b/c
  $ cd b/c
  $ hg status
  M a
  ? a.orig
  
  # The repository is in an unfinished *merge* state.
  # Unresolved merge conflicts:
  # 
  #     ../../a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  $ cd ../..

Test hg status is normal after merge abort
  $ hg update --clean -q
  $ hg status
  ? a.orig
  $ rm a.orig

Test non-conflicted merge state
  $ hg up -r 1 -q
  $ touch z
  $ hg add z
  $ hg commit -m 'a commit that will merge without conflicts' -q
  $ hg merge -r 79361b8cdbb -q
  $ hg status
  M a
  
  # The repository is in an unfinished *merge* state.
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)

Test hg status is normal after merge commit (no output)
  $ hg commit -m 'merge commit' -q
  $ hg status

Test interrupted update state, without active bookmark and REV is a hash
  $ breakupdate
  $ hg update -C 2977a57ce863
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg update -C 2977a57ce863
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)

Test interrupted update state, with active bookmark and REV is a bookmark
  $ hg bookmark b1
  $ hg bookmark -r 79361b8cdbb5 b2
  $ hg update b2
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg update b2
  # To abort:                   hg update --clean b1    (warning: this will discard uncommitted changes)

Test update state can be reset using bookmark
  $ hg update b1 -q
  $ hg bookmark -d b1 -q
  $ hg status

Test interrupted update state, without active bookmark and REV is specified using date
  $ echo a >> a
  $ hg commit --date "1234567890 0" -m m -q
  $ hg update --date 1970-1-1 -q
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg update --date 1970-1-1 -q
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)

  $ unbreakupdate

Test update state can be reset using .
  $ hg update . -q
  $ hg status

Test args escaping in continue command
  $ breakupdate
  $ hg bookmark b1
  $ hg --config extensions.fsmonitor=! --config ui.ssh="ssh -oControlMaster=no" update -C 2977a57ce863
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg --config 'extensions.fsmonitor=!' --config 'ui.ssh=ssh -oControlMaster=no' update -C 2977a57ce863
  # To abort:                   hg update --clean b1    (warning: this will discard uncommitted changes)

