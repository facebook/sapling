#require killdaemons

Create an extension to test bundle2 remote-changegroup parts

  $ cat > bundle2.py << EOF
  > """A small extension to test bundle2 remote-changegroup parts.
  > 
  > Current bundle2 implementation doesn't provide a way to generate those
  > parts, so they must be created by extensions.
  > """
  > from mercurial import bundle2, changegroup, exchange, util
  > 
  > def _getbundlechangegrouppart(bundler, repo, source, bundlecaps=None,
  >                               b2caps=None, heads=None, common=None,
  >                               **kwargs):
  >     """this function replaces the changegroup part handler for getbundle.
  >     It allows to create a set of arbitrary parts containing changegroups
  >     and remote-changegroups, as described in a bundle2maker file in the
  >     repository .hg/ directory.
  > 
  >     Each line of that bundle2maker file contain a description of the
  >     part to add:
  >       - changegroup common_revset heads_revset
  >           Creates a changegroup part based, using common_revset and
  >           heads_revset for changegroup.getchangegroup.
  >       - remote-changegroup url file
  >           Creates a remote-changegroup part for a bundle at the given
  >           url. Size and digest, as required by the client, are computed
  >           from the given file.
  >       - raw-remote-changegroup <python expression>
  >           Creates a remote-changegroup part with the data given in the
  >           python expression as parameters. The python expression is
  >           evaluated with eval, and is expected to be a dict.
  >     """
  >     def newpart(name, data=''):
  >         """wrapper around bundler.newpart adding an extra part making the
  >         client output information about each processed part"""
  >         bundler.newpart('output', data=name)
  >         part = bundler.newpart(name, data=data)
  >         return part
  > 
  >     for line in open(repo.join('bundle2maker'), 'r'):
  >         line = line.strip()
  >         try:
  >             verb, args = line.split(None, 1)
  >         except ValueError:
  >             verb, args = line, ''
  >         if verb == 'remote-changegroup':
  >            url, file = args.split()
  >            bundledata = open(file, 'rb').read()
  >            digest = util.digester.preferred(b2caps['digests'])
  >            d = util.digester([digest], bundledata)
  >            part = newpart('remote-changegroup')
  >            part.addparam('url', url)
  >            part.addparam('size', str(len(bundledata)))
  >            part.addparam('digests', digest)
  >            part.addparam('digest:%s' % digest, d[digest])
  >         elif verb == 'raw-remote-changegroup':
  >            part = newpart('remote-changegroup')
  >            for k, v in eval(args).items():
  >                part.addparam(k, str(v))
  >         elif verb == 'changegroup':
  >             _common, heads = args.split()
  >             common.extend(repo.lookup(r) for r in repo.revs(_common))
  >             heads = [repo.lookup(r) for r in repo.revs(heads)]
  >             cg = changegroup.getchangegroup(repo, 'changegroup',
  >                 heads=heads, common=common)
  >             newpart('changegroup', cg.getchunks())
  >         else:
  >             raise Exception('unknown verb')
  > 
  > exchange.getbundle2partsmapping['changegroup'] = _getbundlechangegrouppart
  > EOF

Start a simple HTTP server to serve bundles

  $ python "$TESTDIR/dumbhttp.py" -p $HGPORT --pid dumb.pid
  $ cat dumb.pid >> $DAEMON_PIDS

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > bundle2-exp=True
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > logtemplate={rev}:{node|short} {phase} {author} {bookmarks} {desc|firstline}
  > EOF

  $ hg init repo

  $ hg -R repo unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg -R repo log -G
  o  7:02de42196ebe draft Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  6:eea13746799a draft Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  5:24b6387c8c8c draft Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  4:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | o  3:32af7686d403 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | o  2:5fddd98957c8 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  1:42ccdea3bb16 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  0:cd010b8cd998 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg clone repo orig
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat > repo/.hg/hgrc << EOF
  > [extensions]
  > bundle2=$TESTTMP/bundle2.py
  > EOF

Test a pull with an remote-changegroup

  $ hg bundle -R repo --base '0:4' -r '5:7' bundle.hg
  3 changesets found
  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/bundle.hg bundle.hg
  > EOF
  $ hg clone orig clone -r 3 -r 4
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files (+1 heads)
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R clone log -G
  o  7:02de42196ebe public Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  6:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  5:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  4:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | @  3:32af7686d403 public Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | o  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ rm -rf clone

Test a pull with an remote-changegroup and a following changegroup

  $ hg bundle -R repo --base 2 -r '3:4' bundle2.hg
  2 changesets found
  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/bundle2.hg bundle2.hg
  > changegroup 0:4 5:7
  > EOF
  $ hg clone orig clone -r 2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  remote: changegroup
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R clone log -G
  o  7:02de42196ebe public Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  6:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  5:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  4:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | o  3:32af7686d403 public Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | @  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ rm -rf clone

Test a pull with a changegroup followed by an remote-changegroup

  $ hg bundle -R repo --base '0:4' -r '5:7' bundle3.hg
  3 changesets found
  $ cat > repo/.hg/bundle2maker << EOF
  > changegroup 000000000000 :4
  > remote-changegroup http://localhost:$HGPORT/bundle3.hg bundle3.hg
  > EOF
  $ hg clone orig clone -r 2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R clone log -G
  o  7:02de42196ebe public Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  6:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  5:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  4:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | o  3:32af7686d403 public Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | @  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ rm -rf clone

Test a pull with two remote-changegroups and a changegroup

  $ hg bundle -R repo --base 2 -r '3:4' bundle4.hg
  2 changesets found
  $ hg bundle -R repo --base '3:4' -r '5:6' bundle5.hg
  2 changesets found
  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/bundle4.hg bundle4.hg
  > remote-changegroup http://localhost:$HGPORT/bundle5.hg bundle5.hg
  > changegroup 0:6 7
  > EOF
  $ hg clone orig clone -r 2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  remote: changegroup
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R clone log -G
  o  7:02de42196ebe public Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  6:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  5:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  4:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | o  3:32af7686d403 public Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | @  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ rm -rf clone

Hash digest tests

  $ hg bundle -R repo -a bundle6.hg
  8 changesets found

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'sha1', 'digest:sha1': '2c880cfec23cff7d8f80c2f12958d1563cbdaba6'}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf clone

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'md5', 'digest:md5': 'e22172c2907ef88794b7bea6642c2394'}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf clone

Hash digest mismatch throws an error

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'sha1', 'digest:sha1': '0' * 40}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  transaction abort!
  rollback completed
  abort: bundle at http://localhost:$HGPORT/bundle6.hg is corrupted:
  sha1 mismatch: expected 0000000000000000000000000000000000000000, got 2c880cfec23cff7d8f80c2f12958d1563cbdaba6
  [255]

Multiple hash digests can be given

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'md5 sha1', 'digest:md5': 'e22172c2907ef88794b7bea6642c2394', 'digest:sha1': '2c880cfec23cff7d8f80c2f12958d1563cbdaba6'}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf clone

If either of the multiple hash digests mismatches, an error is thrown

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'md5 sha1', 'digest:md5': '0' * 32, 'digest:sha1': '2c880cfec23cff7d8f80c2f12958d1563cbdaba6'}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  transaction abort!
  rollback completed
  abort: bundle at http://localhost:$HGPORT/bundle6.hg is corrupted:
  md5 mismatch: expected 00000000000000000000000000000000, got e22172c2907ef88794b7bea6642c2394
  [255]

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle6.hg', 'size': 1663, 'digests': 'md5 sha1', 'digest:md5': 'e22172c2907ef88794b7bea6642c2394', 'digest:sha1': '0' * 40}
  > EOF
  $ hg clone ssh://user@dummy/repo clone
  requesting all changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  transaction abort!
  rollback completed
  abort: bundle at http://localhost:$HGPORT/bundle6.hg is corrupted:
  sha1 mismatch: expected 0000000000000000000000000000000000000000, got 2c880cfec23cff7d8f80c2f12958d1563cbdaba6
  [255]

Corruption tests

  $ hg clone orig clone -r 2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/bundle4.hg bundle4.hg
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle5.hg', 'size': 578, 'digests': 'sha1', 'digest:sha1': '0' * 40}
  > changegroup 0:6 7
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  transaction abort!
  rollback completed
  abort: bundle at http://localhost:$HGPORT/bundle5.hg is corrupted:
  sha1 mismatch: expected 0000000000000000000000000000000000000000, got f29485d6bfd37db99983cfc95ecb52f8ca396106
  [255]

The entire transaction has been rolled back in the pull above

  $ hg -R clone log -G
  @  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  |
  o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  

No params

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: remote-changegroup: missing "url" param
  [255]

Missing size

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle4.hg'}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: remote-changegroup: missing "size" param
  [255]

Invalid size

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle4.hg', 'size': 'foo'}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: remote-changegroup: invalid value for param "size"
  [255]

Size mismatch

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle4.hg', 'size': 42}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  transaction abort!
  rollback completed
  abort: bundle at http://localhost:$HGPORT/bundle4.hg is corrupted:
  size mismatch: expected 42, got 581
  [255]

Unknown digest

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle4.hg', 'size': 581, 'digests': 'foo', 'digest:foo': 'bar'}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: missing support for remote-changegroup - digest:foo
  [255]

Missing digest

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'http://localhost:$HGPORT/bundle4.hg', 'size': 581, 'digests': 'sha1'}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: remote-changegroup: missing "digest:sha1" param
  [255]

Not an HTTP url

  $ cat > repo/.hg/bundle2maker << EOF
  > raw-remote-changegroup {'url': 'ssh://localhost:$HGPORT/bundle4.hg', 'size': 581}
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: remote-changegroup does not support ssh urls
  [255]

Not a bundle

  $ cat > notbundle.hg << EOF
  > foo
  > EOF
  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/notbundle.hg notbundle.hg
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: http://localhost:$HGPORT/notbundle.hg: not a Mercurial bundle
  [255]

Not a bundle 1.0

  $ cat > notbundle10.hg << EOF
  > HG20
  > EOF
  $ cat > repo/.hg/bundle2maker << EOF
  > remote-changegroup http://localhost:$HGPORT/notbundle10.hg notbundle10.hg
  > EOF
  $ hg pull -R clone ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  remote: remote-changegroup
  abort: http://localhost:$HGPORT/notbundle10.hg: not a bundle version 1.0
  [255]

  $ hg -R clone log -G
  @  2:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  |
  o  1:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ rm -rf clone

  $ killdaemons.py $DAEMON_PIDS
