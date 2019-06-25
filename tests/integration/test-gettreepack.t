  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hg init repo-hg

setup hg server repo
  $ cd repo-hg
  $ setup_hg_server
  $ cd $TESTTMP

setup client repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate -q
  $ cd repo2
  $ setup_hg_client

make a few commits on the server
  $ cd $TESTTMP/repo-hg
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Pull from Mononoke
  $ cd repo2
  $ hgmn pull -q
  warning: stream clone requested but client is missing requirements: lz4revlog
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)

Make sure that cache is empty
  $ [[ -a $TESTTMP/cachepath/repo/packs/manifests ]]
  [1]

  $ hgmn prefetch -r 0 -r1
  $ hgmn prefetch -r 2

Make sure that new entries were downloaded
  $ [[ -a $TESTTMP/cachepath/repo/packs/manifests ]]
  $ ls $TESTTMP/cachepath/repo/packs/manifests | wc -l
  8

Update to the revisions. Change the path to make sure that gettreepack command is
not sent because we've already downloaded all the trees
  $ hgmn up 2 --config paths.default=ssh://brokenpath -q
  $ ls
  A
  B
  C

Change the path to make sure that no wireproto commands should be sent at all,
because everything has been already downloaded.
  $ hgmn up 1 --config paths.default=ssh://brokenpath -q
  $ ls
  A
  B

  $ cat >> $TESTTMP/gettreepack.py <<EOF
  > from edenscm.mercurial import registrar
  > from edenscm.mercurial.node import bin
  > from edenscm.mercurial import (bundle2, extensions)
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('gettreepack', [
  >     ('', 'mfnode', [], 'specify the manifest revisions', 'REV'),
  > ], '[-r REV]')
  > def _gettreepack(ui, repo, **opts):
  >     treemanifestext = extensions.find('treemanifest')
  >     fallbackpath = treemanifestext.getfallbackpath(repo)
  >     with repo.connectionpool.get(fallbackpath) as conn:
  >         remote = conn.peer
  >         depth = 100
  >         bundle = remote.gettreepack('', [bin(mfnode) for mfnode in opts.get('mfnode')], [], [], depth)
  >         bundle2.processbundle(repo, bundle, None)
  > EOF

  $ hgmn --config extensions.gettreepack=$TESTTMP/gettreepack.py gettreepack --mfnode 1111111111111111111111111111111111111111
  remote: Command failed
  remote:   Error:
  remote:     Manifest id 1111111111111111111111111111111111111111 is missing
  remote:   Root cause:
  remote:     ManifestMissing(
  remote:         HgManifestId(
  remote:             HgNodeHash(
  remote:                 Sha1(1111111111111111111111111111111111111111),
  remote:             ),
  remote:         ),
  remote:     )
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
