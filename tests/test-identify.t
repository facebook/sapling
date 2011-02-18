  $ "$TESTDIR/hghave" no-outer-repo || exit 80

no repo

  $ hg id
  abort: there is no Mercurial repository here (.hg not found)
  [255]

create repo

  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a

basic id usage

  $ hg id
  cb9a9f314b8b tip
  $ hg id --debug
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b tip
  $ hg id -q
  cb9a9f314b8b
  $ hg id -v
  cb9a9f314b8b tip

with options

  $ hg id -r.
  cb9a9f314b8b tip
  $ hg id -n
  0
  $ hg id -t
  tip
  $ hg id -b
  default
  $ hg id -i
  cb9a9f314b8b
  $ hg id -n -t -b -i
  cb9a9f314b8b 0 default tip

with modifications

  $ echo b > a
  $ hg id -n -t -b -i
  cb9a9f314b8b+ 0+ default tip

other local repo

  $ cd ..
  $ hg -R test id
  cb9a9f314b8b+ tip
  $ hg id test
  cb9a9f314b8b+ tip

with remote http repo

  $ cd test
  $ hg serve -p $HGPORT1 -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg id http://localhost:$HGPORT1/
  cb9a9f314b8b

remote with rev number?

  $ hg id -n http://localhost:$HGPORT1/
  abort: can't query remote revision number, branch, tags, or bookmarks
  [255]

remote with tags?

  $ hg id -t http://localhost:$HGPORT1/
  abort: can't query remote revision number, branch, tags, or bookmarks
  [255]

remote with branch?

  $ hg id -b http://localhost:$HGPORT1/
  abort: can't query remote revision number, branch, tags, or bookmarks
  [255]

remote with bookmarks?

  $ hg id -B http://localhost:$HGPORT1/
  abort: can't query remote revision number, branch, tags, or bookmarks
  [255]

Make sure we do not obscure unknown requires file entries (issue2649)

  $ echo fake >> .hg/requires
  $ hg id
  abort: requirement 'fake' not supported!
  [255]

  $ cd ..
  $ hg id test
  abort: requirement 'fake' not supported!
  [255]

