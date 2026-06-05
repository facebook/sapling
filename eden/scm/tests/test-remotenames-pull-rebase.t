#chg-compatible

  $ eagerepo

  $ mkcommit()
  > {
  >    echo $1 > $1
  >    sl add $1
  >    sl ci -m "$1" -q
  >    export $1=$(sl log -T '{node}' -r .)
  > }

  $ printdag()
  > {
  >   sl log -G --template '{desc} | {bookmarks} | {remotebookmarks}'
  > }

Test sl pull --rebase degrades gracefully if rebase extension is not loaded
  $ sl init remoterepo
  $ cd remoterepo
  $ mkcommit root
  $ sl book bookmarkonremote

  $ newclientrepo localrepo remoterepo bookmarkonremote

Make sure to enable tracking
  $ sl book bmtrackingremote --track remote/bookmarkonremote
  $ sl pull --rebase > /dev/null
  abort: missing rebase destination - supply --dest / -d
  [255]

Tests 'sl pull --rebase' rebases from the active tracking bookmark onto the appropriate remote changes.
  $ enable rebase
  $ cd ../remoterepo

Create remote changes
  $ mkcommit trackedremotecommit
  $ sl up -q -r 'desc(root)'
  $ mkcommit untrackedremotecommit
  $ printdag
  @  untrackedremotecommit |  |
  │
  │ o  trackedremotecommit | bookmarkonremote |
  ├─╯
  o  root |  |
  

Create local changes and checkout tracking bookmark
  $ cd ../localrepo
  $ sl up -q bmtrackingremote
  $ mkcommit localcommit
  $ printdag
  @  localcommit | bmtrackingremote |
  │
  o  root |  | remote/bookmarkonremote
  
Pull remote changes and rebase local changes with tracked bookmark onto them
  $ sl pull -q --rebase -r $untrackedremotecommit
  $ printdag
  @  localcommit | bmtrackingremote |
  │
  │ o  untrackedremotecommit |  |
  │ │
  o │  trackedremotecommit |  | remote/bookmarkonremote
  ├─╯
  o  root |  |
  
Tests 'sl pull --rebase' defaults to original (rebase->pullrebase) behaviour when using non-tracking bookmark
  $ sl debugstrip -q -r 'desc(localcommit)' -r 7a820e70c81fe1bccfa7c7e3b9a863b4426402b0
  $ sl book -d bmtrackingremote
  $ sl book bmnottracking
  $ mkcommit localcommit
  $ printdag
  o  untrackedremotecommit |  |
  │
  │ @  localcommit | bmnottracking |
  ├─╯
  o  root |  | remote/bookmarkonremote
  
  $ sl pull --rebase -d .
  pulling from test:remoterepo
  searching for changes
  nothing to rebase - working directory parent is also destination
  $ sl rebase -d 'desc(untrackedremotecommit)'
  rebasing 6a7c7fb59c1e "localcommit" (bmnottracking)
  $ printdag
  @  localcommit | bmnottracking |
  │
  o  untrackedremotecommit |  |
  │
  │ o  trackedremotecommit |  | remote/bookmarkonremote
  ├─╯
  o  root |  |
  
Tests the behavior of a pull followed by a pull --rebase
  $ cd ../remoterepo
  $ sl up bookmarkonremote -q
  $ echo foo > foo
  $ sl add foo -q
  $ sl commit -m foo -q
  $ cd ../localrepo
  $ sl book -t remote/bookmarkonremote tracking
  $ sl pull
  pulling from test:remoterepo
  searching for changes
  $ sl debugmakepublic 4557926d2166

Tests that there are no race condition between pulling changesets and remote bookmarks
  $ cd ..
  $ cat > hangpull.py << EOF
  > """A small extension that makes pull hang for 5 sec, for testing"""
  > from sapling import extensions, exchange
  > def _pullremotenames(orig, repo, remote, *args, **opts):
  >     import time
  >     time.sleep(5)
  >     return orig(repo, remote, *args, **opts)
  > def extsetup(ui):
  >     remotenames = extensions.find('remotenames')
  >     extensions.wrapfunction(remotenames, 'pullremotenames', _pullremotenames)
  > EOF
  $ cd localrepo
  $ sl --config="extensions.hangpull=$TESTTMP/hangpull.py" -q pull &
  $ sleep 1
  $ cd ../remoterepo
  $ sl up bookmarkonremote -q
  $ mkcommit between_pull
  $ wait
  $ sl log -l 1 --template="{desc}\n"
  between_pull
  $ cd ../localrepo
  $ sl up tracking -q
  $ sl log -l 1 --template="{desc} {remotenames}\n"
  foo remote/bookmarkonremote
  $ sl -q pull
  $ sl log -l 1 --template="{desc} {remotenames}\n"
  between_pull remote/bookmarkonremote

Test pull with --rebase and --tool
  $ cd ../remoterepo
  $ sl up bookmarkonremote -q
  $ echo remotechanges > editedbyboth
  $ sl add editedbyboth
  $ mkcommit remotecommit
  $ cd ../localrepo
  $ sl book -t remote/bookmarkonremote -r remote/bookmarkonremote tracking2
  $ sl goto tracking2 -q
  $ echo localchanges > editedbyboth
  $ sl add editedbyboth
  $ mkcommit somelocalchanges
  $ printdag
  @  somelocalchanges | tracking2 |
  │
  o  between_pull |  | remote/bookmarkonremote
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
  
  $ sl pull --rebase --tool internal:union
  pulling from test:remoterepo
  searching for changes
  rebasing 1d01e32a0efb "somelocalchanges" (tracking2)
  merging editedbyboth
  $ printdag
  @  somelocalchanges | tracking2 |
  │
  o  remotecommit |  | remote/bookmarkonremote
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
