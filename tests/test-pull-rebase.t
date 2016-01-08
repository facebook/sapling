  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=`dirname $TESTDIR`/remotenames.py" >> $HGRCPATH

  $ mkcommit()
  > {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "$1" -q
  > }

  $ printdag()
  > {
  >   hg log -G --template '{desc} | {bookmarks} | {remotebookmarks}'
  > }

Test hg pull --rebase degrades gracefully if rebase extension is not loaded
  $ hg init remoterepo
  $ cd remoterepo
  $ mkcommit root
  $ hg book bookmarkonremote

  $ cd ..
  $ hg clone -q remoterepo localrepo
  $ cd localrepo

Make sure to enable tracking
  $ hg book bmtrackingremote --track default/bookmarkonremote
  $ hg pull --rebase > /dev/null
  hg pull: option --rebase not recognized
  [255]

Tests 'hg pull --rebase' rebases from the active tracking bookmark onto the appropriate remote changes.
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "rebase=" >> $HGRCPATH
  $ cd ../remoterepo

Create remote changes
  $ mkcommit trackedremotecommit
  $ hg up -q -r 0
  $ mkcommit untrackedremotecommit
  $ printdag
  @  untrackedremotecommit |  |
  |
  | o  trackedremotecommit | bookmarkonremote |
  |/
  o  root |  |
  

Create local changes and checkout tracking bookmark
  $ cd ../localrepo
  $ hg up -q bmtrackingremote
  $ mkcommit localcommit
  $ printdag
  @  localcommit | bmtrackingremote |
  |
  o  root |  | default/bookmarkonremote
  
Pull remote changes and rebase local changes with tracked bookmark onto them
  $ hg pull -q --rebase
  $ printdag
  @  localcommit | bmtrackingremote |
  |
  | o  untrackedremotecommit |  |
  | |
  o |  trackedremotecommit |  | default/bookmarkonremote
  |/
  o  root |  |
  
Tests 'hg pull --rebase' defaults to original (rebase->pullrebase) behaviour when using non-tracking bookmark
  $ echo "strip=" >> $HGRCPATH
  $ hg strip -q -r 3 -r 2 -r 1
  $ hg book -d bmtrackingremote
  $ hg book bmnottracking
  $ mkcommit localcommit
  $ printdag
  @  localcommit | bmnottracking |
  |
  o  root |  |
  
  $ hg pull --rebase -q
  $ printdag
  @  localcommit | bmnottracking |
  |
  o  untrackedremotecommit |  |
  |
  | o  trackedremotecommit |  | default/bookmarkonremote
  |/
  o  root |  |
  
Tests the behavior of a pull followed by a pull --rebase
  $ cd ../remoterepo
  $ hg up bookmarkonremote -q
  $ echo foo > foo
  $ hg add foo -q
  $ hg commit -m foo -q
  $ cd ../localrepo
  $ hg book -t default/bookmarkonremote tracking
  $ hg pull
  pulling from $TESTTMP/remoterepo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg pull --rebase
  pulling from $TESTTMP/remoterepo (glob)
  searching for changes
  no changes found
  abort: can't rebase public changeset 4557926d2166
  (see "hg help phases" for details)
  [255]

Tests that there are no race condition between pulling changesets and remote bookmarks
  $ cd ..
  $ cat > hangpull.py << EOF
  > """A small extension that makes pull hang for 5 sec, for testing"""
  > from mercurial import extensions, exchange
  > def _pullremotenames(orig, repo, remote, *args, **opts):
  >     import time
  >     time.sleep(5)
  >     return orig(repo, remote, *args, **opts)
  > def extsetup(ui):
  >     remotenames = extensions.find('remotenames')
  >     extensions.wrapfunction(remotenames, 'pullremotenames', _pullremotenames)
  > EOF
  $ cd localrepo
  $ hg --config="extensions.hangpull=$TESTTMP/hangpull.py" -q pull &
  $ sleep 1
  $ cd ../remoterepo
  $ hg up bookmarkonremote -q
  $ mkcommit between_pull
  $ wait
  $ hg log -l 1 --template="{desc}\n"
  between_pull
  $ cd ../localrepo
  $ hg up tracking -q
  $ hg log -l 1 --template="{desc} {remotenames}\n"
  foo default/bookmarkonremote default/default
  $ hg -q pull
  $ hg log -l 1 --template="{desc} {remotenames}\n"
  between_pull default/bookmarkonremote default/default
