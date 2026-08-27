#require eden no-windows

  $ configure modernclient

Create a local snapshot restore command so the test does not depend on a
snapshot service. The empty file change list is enough to exercise the parent
update and its hook while the snapshot transaction is open.

  $ cat > snapshotrestore.py << 'EOF'
  > from sapling import registrar
  > from sapling.ext.snapshot import update as snapshot_update
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command("debugsnapshotrestore", [], "REV")
  > def debugsnapshotrestore(ui, repo, rev):
  >     snapshot_update.fetchsnapshot = lambda repo, csid: {
  >         "hg_parents": repo[rev].node(),
  >         "file_changes": [],
  >     }
  >     snapshot_update._download_files_and_fix_status = lambda ui, repo, snapshot: None
  >     snapshot_update.update(ui, repo, "00" * 20)
  > EOF
  $ cat > $TESTTMP/update-hook.sh << 'EOF'
  > #!/bin/sh
  > printf 'hook parent: '
  > sl log -r . -T '{desc}\n'
  > sl status
  > echo 'hook status: ok'
  > EOF

  $ setconfig extensions.snapshotrestore="$TESTTMP/snapshotrestore.py"

  $ newclientrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ B=$(sl log -r 'desc(B)' -T '{node}')
  $ A=$(sl log -r 'desc(A)' -T '{node}')
  $ sl goto -q "$A"
  $ setconfig hooks.update="sh $TESTTMP/update-hook.sh"

The hook reads the pending dirstate parent while the snapshot transaction is
open.

  $ sl debugsnapshotrestore "$B"
  snapshot: will restore snapshot 0000000000000000000000000000000000000000
  snapshot: updating to parent * (glob)
  hook parent: B
  hook status: ok
  update complete
  snapshot: updated to parent * in * seconds (glob)
  snapshot: restored snapshot in * seconds (glob)
  $ sl log -r . -T '{desc}\n'
  B
