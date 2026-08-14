#debugruntest-incompatible
#require eden no-windows

Disable Eden's in-memory tree cache so checkout asks Sapling for both trees.
Give Sapling a short config reload interval.

  $ cat > $TESTTMP/.edenrc <<EOF
  > [treecache]
  > enable-in-memory-tree-caching = false
  > EOF
  $ setconfig backingstore.reload-check-interval-secs=0
  $ setconfig backingstore.reload-interval-secs=1
  $ setconfig experimental.restricted-tree-mode=disabled
  $ setconfig slacl.server-acl-enforcement=false

Create two commits where the same ACL-protected directory changes.

  $ newserver server
  $ drawdag << 'EOS'
  > B  # B/restricted/.slacl = acl config
  >    # B/restricted/file.txt = target
  > |
  > A  # A/restricted/.slacl = acl config
  >    # A/restricted/file.txt = base
  > EOS

The user starts with access, materializes the directory, and edits its tracked
file without committing the edit.

  $ newclientrepo client server
  $ sl go -q $A
  $ cat restricted/file.txt
  base (no-eol)
  $ echo local > restricted/file.txt
  $ cat restricted/file.txt
  local

Revoke access while Eden is still running. The short reload interval makes the
running Sapling backing store observe the config change before the next tree
fetch.

  $ setconfig slacl.server-acl-enforcement=true
  $ sleep 2

BUG: checkout treats the newly opaque old tree as empty, reports success, and
silently removes the user's local edit before installing the restricted tree.
If Eden still had the real old tree, this modified tracked file would produce a
checkout conflict instead of disappearing.

  $ sl go $B
  $ test ! -e restricted/file.txt
