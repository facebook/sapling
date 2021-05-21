#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1 ui.traceback=1
  $ setconfig treemanifest.flatcompat=0
  $ setconfig infinitepush.httpbookmarks=1
  $ setconfig pull.httpbookmarks=1
  $ export LOG=pull::httpbookmarks=debug

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > B C  # C/T/A=2
  > |/
  > A    # A/T/A=1
  > EOS

Push:

  $ hg push -r $C --to master --create
  pushing rev 178c10ffbc2f to destination test:e1 bookmark master
  searching for changes
  exporting bookmark master
  $ hg book --list-remote master
     master                    178c10ffbc2f92d5407c14478ae9d9dea81f232e

Pull:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull -B master
  pulling from test:e1
   DEBUG pull::httpbookmarks: edenapi fetched bookmarks: {'master': '178c10ffbc2f92d5407c14478ae9d9dea81f232e'}
  $ hg book --list-subscriptions
     remote/master             178c10ffbc2f
