  $ . $TESTDIR/library.sh

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

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport --blobstore files --linknodes repo-hg repo

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

Pull from Mononoke
  $ cd repo2
  $ hgmn pull -q

Make sure that cache is empty
  $ [[ -a $TESTTMP/cachepath/repo/packs/manifests ]]
  [1]

Small extension to call gettreepack method with a few nodes. At the time of writing this test
hg prefetch failed for treeonly repos. We can use it instead when it's fixed
  $ cat >> $TESTTMP/gettreepack.py <<EOF
  > from mercurial import registrar
  > from mercurial import (bundle2, extensions)
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('gettreepack', [
  >     ('r', 'rev', [], 'specify the revision', 'REV'),
  >     ('', 'baserev', [], 'specify the base revision', 'REV'),
  > ], '[-r REV]')
  > def _gettreepack(ui, repo, **opts):
  >     treemanifestext = extensions.find('treemanifest')
  >     fallbackpath = treemanifestext.getfallbackpath(repo)
  >     ctxs = [repo[r] for r in opts.get('rev')]
  >     basectxs = [repo[r] for r in opts.get('baserev')]
  >     with repo.connectionpool.get(fallbackpath) as conn:
  >         remote = conn.peer
  >         mfnodes = [ctx.manifestnode() for ctx in ctxs]
  >         basemfnodes = [ctx.manifestnode() for ctx in basectxs]
  >         bundle = remote.gettreepack('', mfnodes, basemfnodes, [])
  >         bundle2.processbundle(repo, bundle, None)
  > EOF

  $ hgmn --config extensions.gettreepack=$TESTTMP/gettreepack.py gettreepack -r 0 -r 1
  $ hgmn --config extensions.gettreepack=$TESTTMP/gettreepack.py gettreepack -r 2 --baserev 1 --baserev 0

Make sure that new entries were downloaded
  $ [[ -a $TESTTMP/cachepath/repo/packs/manifests ]]
  $ ls $TESTTMP/cachepath/repo/packs/manifests
  14d5757b02d7849c29f5330a9784cde6ea705b10.dataidx
  14d5757b02d7849c29f5330a9784cde6ea705b10.datapack
  19f8819363e37aac879d4b9d262a6f159c2f499e.histidx
  19f8819363e37aac879d4b9d262a6f159c2f499e.histpack
  2b552b0b3dee8707b0d8e59440602314c30e7c48.histidx
  2b552b0b3dee8707b0d8e59440602314c30e7c48.histpack
  bd34d6c34ee406323fec52573c480997f38ae742.dataidx
  bd34d6c34ee406323fec52573c480997f38ae742.datapack
