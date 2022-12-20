#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ enable remotenames

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
  (use 'hg pull -h' to get help)
  [255]

Tests 'hg pull --rebase' rebases from the active tracking bookmark onto the appropriate remote changes.
  $ setglobalconfig extensions.rebase=
  $ cd ../remoterepo

Create remote changes
  $ mkcommit trackedremotecommit
  $ hg up -q -r 'desc(root)'
  $ mkcommit untrackedremotecommit
  $ printdag
  @  untrackedremotecommit |  |
  │
  │ o  trackedremotecommit | bookmarkonremote |
  ├─╯
  o  root |  |
  

Create local changes and checkout tracking bookmark
  $ cd ../localrepo
  $ hg up -q bmtrackingremote
  $ mkcommit localcommit
  $ printdag
  @  localcommit | bmtrackingremote |
  │
  o  root |  | default/bookmarkonremote
  
Pull remote changes and rebase local changes with tracked bookmark onto them
  $ hg pull -q --rebase
  $ printdag
  @  localcommit | bmtrackingremote |
  │
  │ o  untrackedremotecommit |  |
  │ │
  o │  trackedremotecommit |  | default/bookmarkonremote
  ├─╯
  o  root |  |
  
Tests 'hg pull --rebase' defaults to original (rebase->pullrebase) behaviour when using non-tracking bookmark
  $ hg debugstrip -q -r 'desc(localcommit)' -r 7a820e70c81fe1bccfa7c7e3b9a863b4426402b0
  $ hg book -d bmtrackingremote
  $ hg book bmnottracking
  $ mkcommit localcommit
  $ printdag
  o  untrackedremotecommit |  |
  │
  │ @  localcommit | bmnottracking |
  ├─╯
  o  root |  |
  
  $ hg pull --rebase
  pulling from $TESTTMP/remoterepo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating to active bookmark bmnottracking
  nothing to rebase
  $ hg rebase -d 'desc(untrackedremotecommit)'
  rebasing 6a7c7fb59c1e "localcommit" (bmnottracking)
  $ printdag
  @  localcommit | bmnottracking |
  │
  o  untrackedremotecommit |  |
  │
  │ o  trackedremotecommit |  | default/bookmarkonremote
  ├─╯
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
  $ hg debugmakepublic 4557926d2166

Tests that there are no race condition between pulling changesets and remote bookmarks
  $ cd ..
  $ cat > hangpull.py << EOF
  > """A small extension that makes pull hang for 5 sec, for testing"""
  > from edenscm import extensions, exchange
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
  foo default/bookmarkonremote
  $ hg -q pull
  $ hg log -l 1 --template="{desc} {remotenames}\n"
  between_pull default/bookmarkonremote

Test pull with --rebase and --tool
  $ cd ../remoterepo
  $ hg up bookmarkonremote -q
  $ echo remotechanges > editedbyboth
  $ hg add editedbyboth
  $ mkcommit remotecommit
  $ cd ../localrepo
  $ hg book -t default/bookmarkonremote -r default/bookmarkonremote tracking2
  $ hg goto tracking2 -q
  $ echo localchanges > editedbyboth
  $ hg add editedbyboth
  $ mkcommit somelocalchanges
  $ printdag
  @  somelocalchanges | tracking2 |
  │
  o  between_pull |  | default/bookmarkonremote
  │
  o  foo |  |
  │
  │ o  localcommit | bmnottracking tracking |
  │ │
  │ o  untrackedremotecommit |  | public/4557926d216642d06949776e29c30bb2a54e7b6d
  │ │
  o │  trackedremotecommit |  |
  ├─╯
  o  root |  |
  
  $ hg pull --rebase --tool internal:union
  pulling from $TESTTMP/remoterepo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  rebasing 1d01e32a0efb "somelocalchanges" (tracking2)
  merging editedbyboth
  $ printdag
  @  somelocalchanges | tracking2 |
  │
  o  remotecommit |  | default/bookmarkonremote
  │
  o  between_pull |  |
  │
  o  foo |  |
  │
  │ o  localcommit | bmnottracking tracking |
  │ │
  │ o  untrackedremotecommit |  | public/4557926d216642d06949776e29c30bb2a54e7b6d
  │ │
  o │  trackedremotecommit |  |
  ├─╯
  o  root |  |
  
  $ cat editedbyboth
  remotechanges
  localchanges
