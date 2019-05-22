Turn manifest verification on and off:
  $ hg init repo1
  $ cd repo1
  $ hg debugdrawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ hg verify --config verify.skipmanifests=0
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions
  $ echo "[verify]" >> $HGRCPATH
  $ echo "skipmanifests=1" >> $HGRCPATH
  $ hg verify
  checking changesets
  verify.skipmanifests is enabled; skipping verification of manifests
  0 files, 3 changesets, 0 total revisions
