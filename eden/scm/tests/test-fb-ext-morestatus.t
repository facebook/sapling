#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ configure mutation-norecord
  $ enable morestatus fbhistedit histedit rebase reset
  $ setconfig morestatus.show=true
  $ cat >> $TESTTMP/breakupdate.py << EOF
  > import sys
  > from edenscm import merge
  > def extsetup(ui):
  >     merge.applyupdates = lambda *args, **kwargs: sys.exit()
  > EOF
  $ breakupdate() {
  >   setconfig extensions.breakupdate="$TESTTMP/breakupdate.py"
  > }
  $ unbreakupdate() {
  >   disable breakupdate
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
  # Current bisect state: 1 good commit(s), 0 bad commit(s), 0 skip commit(s)
  # To mark the changeset good:    hg bisect --good
  # To mark the changeset bad:     hg bisect --bad
  # To abort:                      hg bisect --reset


Verify that suppressing a morestatus state warning works with the config knob:
  $ hg status --config morestatus.skipstates=bisect

Test hg status is normal after bisect reset
  $ hg bisect --reset
  $ hg status

Test graft state
  $ hg up -q -r 'max(desc(a))'
  $ echo '' > a
  $ hg commit -q -m 'remove content'

  $ hg up -q -r 'max(desc(a))'
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
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)


Test hg status is normal after graft abort
  $ hg up --clean -q
  $ hg status
  ? a.orig
  $ rm a.orig

Test unshelve state
  $ enable shelve
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
  $ hg up -r 0efcea34f18aa8f87dc63b4c37b7c494bc778b03 -q
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
  # 
  # Rebasing from 2977a57ce863 (remove content)
  #            to 79361b8cdbb5 (add content)


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
  # 
  # Rebasing from 2977a57ce863 (remove content)
  #            to 79361b8cdbb5 (add content)


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
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)


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
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)

  $ cd ../..

Test hg status is normal after merge abort
  $ hg goto --clean -q
  $ hg status
  ? a.orig
  $ rm a.orig

Test non-conflicted merge state
  $ hg up -r 0efcea34f18aa8f87dc63b4c37b7c494bc778b03 -q
  $ touch z
  $ hg add z
  $ hg commit -m 'a commit that will merge without conflicts' -q
  $ hg merge -r 79361b8cdbb -q
  $ hg status
  M a
  
  # The repository is in an unfinished *merge* state.
  # To continue:                hg commit
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)


Test hg status is normal after merge commit (no output)
  $ hg commit -m 'merge commit' -q
  $ hg status

Test interrupted update state, without active bookmark and REV is a hash
  $ breakupdate
  $ hg goto -C 2977a57ce863
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg goto -C 2977a57ce863
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)


Test interrupted update state, with active bookmark and REV is a bookmark
  $ hg bookmark b1
  $ hg bookmark -r 79361b8cdbb5 b2
  $ hg goto b2
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg goto b2
  # To abort:                   hg goto --clean b1    (warning: this will discard uncommitted changes)


Test update state can be reset using bookmark
  $ hg goto b1 -q
  $ hg bookmark -d b1 -q
  $ hg status

Test interrupted update state, without active bookmark and REV is specified using date
  $ echo a >> a
  $ hg commit --date "1234567890 0" -m m -q
  $ hg goto --date 1970-1-1 -q
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg goto --date 1970-1-1 -q
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)


  $ unbreakupdate

Test update state can be reset using .
  $ hg goto . -q
  $ hg status

Test args escaping in continue command
  $ breakupdate
  $ hg bookmark b1
  $ hg --config extensions.fsmonitor=! --config ui.ssh="ssh -oControlMaster=no" update -C 2977a57ce863
  $ hg status
  
  # The repository is in an unfinished *update* state.
  # To continue:                hg --config 'extensions.fsmonitor=!' --config 'ui.ssh=ssh -oControlMaster=no' update -C 2977a57ce863
  # To abort:                   hg goto --clean b1    (warning: this will discard uncommitted changes)


  $ unbreakupdate
  $ hg goto --clean b1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status

Test bisect search status (after cleaning up previous setup)
  $ echo 'z' > z
  $ hg commit -Am 'z' -q
  $ hg bisect --bad
  $ hg bisect --good 0efcea34f18a
  Testing changeset 69a19f24e505 (5 changesets remaining, ~2 tests)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  
  # The repository is in an unfinished *bisect* state.
  # Current bisect state: 1 good commit(s), 1 bad commit(s), 0 skip commit(s)
  # 
  # Current Tracker: bad commit     current        good commit
  #                  547e426ae373...69a19f24e505...0efcea34f18a
  # Commits remaining:           5
  # Estimated bisects remaining: 3
  # To mark the changeset good:    hg bisect --good
  # To mark the changeset bad:     hg bisect --bad
  # To abort:                      hg bisect --reset


Test hg status is normal after bisect reset
  $ hg bisect --reset
  $ hg status
