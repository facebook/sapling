#modern-config-incompatible

#require no-eden

#inprocess-hg-incompatible
  $ setconfig devel.segmented-changelog-rev-compat=true

  $ configure dummyssh
This test tries to exercise the ssh functionality with a dummy script

creating 'remote' repo

  $ hg init remote
  $ cd remote
  $ hg unbundle "$TESTDIR/bundles/remote.hg"
  adding changesets
  adding manifests
  adding file changes
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

clone remote via stream

  $ for i in 0 1 2 3 4 5 6 7 8; do
  >    hg clone --stream -r "$i" ssh://user@dummy/remote test-"$i"
  >    if cd test-"$i"; then
  >       hg verify
  >       cd ..
  >    fi
  > done
  adding changesets
  adding manifests
  adding file changes
  updating to bfaf4b5cbf0118bb4af9af4a814f69938aba779d
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to 21f32785131faada60c836287c02be2897cd236b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to 4ce51a113780a3de8b48c9f6fe2cd277498316c8
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to 93ee6ab32777cd430e07da694794fb6a4f917712
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to c70afb1ee98533b7b5660ecff09b6b5f7f582089
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to f03ae5a9b9793707225f01c702f21f1fc4bd338f
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to 095cb14b1b4d2c1dd4adcd0a4850288728a62af0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to faa2e4234c7af3618f18f052cdd849dd96cc0d01
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to 916f1afdef9056d85a9da7c863112473923434a1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  $ cd test-8
  $ hg pull ../test-7
  pulling from ../test-7
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ cd ..
  $ cd test-1
  $ hg pull -r 4 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ hg pull ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ cd ..
  $ cd test-2
  $ hg pull -r 5 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ hg pull ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cd ..
