setup
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

create master bookmark
  $ hg bookmark master_bookmark -r tip

setup data
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

setup config
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > clienttelemetry=
  > [clienttelemetry]
  > announceremotehostname=true
  > EOF

set up the local repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg local -q
  $ cd local
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  connected to * (glob)
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  $ hgmn pull --config clienttelemetry.announceremotehostname=False
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
