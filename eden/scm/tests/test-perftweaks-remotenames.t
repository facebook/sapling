#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True


  $ configure dummyssh
  $ enable tweakdefaults remotenames rebase

  $ hg init master
  $ cd master
  $ echo a >> a && hg ci -Aqm a
  $ hg book master
  $ hg book -i
  $ echo b >> b && hg ci -Aqm b
  $ hg book foo

  $ cd ..
  $ hg clone ssh://user@dummy/master client -r 0
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Verify pulling only some commits does not cause errors from the unpulled
remotenames
  $ cd client
  $ hg pull -r 0
  pulling from ssh://user@dummy/master
  no changes found
  $ hg book --remote
     default/master            cb9a9f314b8b
  $ hg dbsh -c 'ui.write(repo.svfs.readutf8("remotenames"))'
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b bookmarks default/master

  $ hg pull --rebase -d master
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  nothing to rebase - working directory parent is also destination
