#modern-config-incompatible

#require no-eden

#inprocess-hg-incompatible


  $ configure dummyssh
  $ enable rebase

  $ hg init master
  $ cd master
  $ echo a >> a && hg ci -Aqm a
  $ hg book master
  $ hg book -i
  $ echo b >> b && hg ci -Aqm b
  $ hg book foo

  $ cd ..
  $ hg clone -q ssh://user@dummy/master client -r 0

Verify pulling only some commits does not cause errors from the unpulled
remotenames
  $ cd client
  $ hg pull -r 0
  pulling from ssh://user@dummy/master
  $ hg book --remote
     remote/foo                       d2ae7f538514cd87c17547b0de4cea71fe1af9fb
     remote/master                    cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  $ hg dbsh -c 'ui.write(repo.svfs.readutf8("remotenames"))'
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b bookmarks remote/master

  $ hg pull --rebase -d master
  pulling from ssh://user@dummy/master
  nothing to rebase - working directory parent is also destination
