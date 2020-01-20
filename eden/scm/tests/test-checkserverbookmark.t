#chg-compatible

  $ . "$TESTDIR/hgsql/library.sh"

Do some initial setup
  $ configure dummyssh
  $ enable checkserverbookmark smartlog
  $ setconfig ui.user="nobody <no.reply@fb.com>"

Setup helpers
  $ log() {
  >   hg sl -T "{desc} [{phase};rev={rev};{node}] {bookmarks}" "$@"
  > }

Setup a server repo
  $ hg init server
  $ cd server
  $ echo a > a && hg ci -qAm a && hg book -i book1
  $ echo b > b && hg ci -qAm b
  $ echo c > c && hg ci -qAm c && hg book -i book2
  $ log -r "all()"
  @  c [draft;rev=2;177f92b773850b59254aa5e923436f921b55483b] book2
  |
  o  b [draft;rev=1;d2ae7f538514cd87c17547b0de4cea71fe1af9fb]
  |
  o  a [draft;rev=0;cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b] book1
  

Verify bookmark locations while not being in a repo
  $ cd $TESTTMP
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book1 --hash cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b --deleted
  abort: can't use `--hash` and `--deleted`
  [255]
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book1
  abort: either `--hash` or `--deleted` should be used
  [255]
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book1 --hash cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server has expected bookmark location. book: book1, hash: cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book2 --hash cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: book2, server: 177f92b773850b59254aa5e923436f921b55483b; expected cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  [1]
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book1 --hash d2ae7f538514cd87c17547b0de4cea71fe1af9fb
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: book1, server: cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b; expected d2ae7f538514cd87c17547b0de4cea71fe1af9fb
  [1]
  $ hg checkserverbookmark --path ssh://user@dummy/server --name nope --hash d2ae7f538514cd87c17547b0de4cea71fe1af9fb
  creating a peer took: * (glob)
  running lookup took: * (glob)
  abort: unknown revision 'nope'!
  [255]
  $ hg checkserverbookmark --path ssh://user@dummy/server --name nope --deleted
  creating a peer took: * (glob)
  running listkeys took: * (glob)
  hg server expectedly does not have a bookmark: nope
  $ hg checkserverbookmark --path ssh://user@dummy/server --name book1 --deleted
  creating a peer took: * (glob)
  running listkeys took: * (glob)
  hg server has bookmark, which is expected to have been deleted: book1
  [1]
