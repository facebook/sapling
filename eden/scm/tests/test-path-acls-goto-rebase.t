#require no-eden

  $ setconfig scmstore.fetch-tree-aux-data=true
  $ setconfig scmstore.tree-metadata-mode=always
  $ setconfig experimental.restricted-tree-mode=enforced
  $ setconfig slacl.server-acl-enforcement=true

  $ enable rebase

Goto: check out a commit where a previously-unrestricted file becomes restricted

  $ newserver server1
  $ drawdag << 'EOS'
  > B  # B/dir/.slacl = acl config
  >    # B/dir/file.txt = content v2
  > |
  > A  # A/dir/file.txt = content v1
  > EOS

  $ cd
  $ newclientrepo client1 server1
  $ sl go -q $A
  $ sl go -q $B
  warning: results may be incomplete due to path ACLs
    'dir' is restricted by ACL 'some-acl'
  [1]

Goto: local modifications to a file that becomes restricted

  $ newserver server2
  $ drawdag << 'EOS'
  > B  # B/dir/.slacl = acl config
  >    # B/dir/file.txt = content v2
  > |
  > A  # A/dir/file.txt = content v1
  > EOS

  $ cd
  $ newclientrepo client2 server2
  $ sl go -q $A
  $ echo 'local change' > dir/file.txt
  $ sl go -q $B
  abort: 1 conflicting file changes:
   dir/file.txt
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  warning: results may be incomplete due to path ACLs
    'dir' is restricted by ACL 'some-acl'
  [255]

Rebase: commit modifies a file that is restricted in destination

  $ newserver server3
  $ drawdag << 'EOS'
  > C  # C/dir/.slacl = acl config
  >    # C/dir/file.txt = content v2
  > |
  > | B  # B/dir/file.txt = modified by user
  > |/
  > A  # A/dir/file.txt = content v1
  > EOS

  $ cd
  $ newclientrepo client3 server3
  $ sl go -q $B
  $ sl rebase -r $B -d $C
  pulling '3af88752c97bb3f6651d0a57a3d16a696f28de48' from 'test:server3'
  rebasing c416137c0b61 "B"
  abort: path 'dir' is restricted by ACL 'some-acl'
  [255]

Rebase: commit adds a file under a path that is restricted in destination

  $ newserver server4
  $ drawdag << 'EOS'
  > C  # C/restricted/.slacl = acl config
  >    # C/restricted/existing.txt = existing
  > |
  > | B  # B/restricted/new.txt = new file
  > |/
  > A  # A/dummy = dummy
  > EOS

  $ cd
  $ newclientrepo client4 server4
  $ sl go -q $B
  $ sl rebase -q -r $B -d $C
  abort: path 'restricted' is restricted by ACL 'some-acl'
  [255]

Rebase: two commits where only the second touches a restricted path

  $ newserver server5
  $ drawdag << 'EOS'
  > D  # D/dir/.slacl = acl config
  >    # D/dir/file.txt = dest content
  >    # D/other.txt = original
  > |
  > | C  # C/dir/file.txt = restricted change
  > | |
  > | B  # B/other.txt = safe change
  > |/
  > A  # A/dir/file.txt = original
  >    # A/other.txt = original
  > EOS

  $ cd
  $ newclientrepo client5 server5
  $ sl go -q $C
  $ sl rebase -r $B::$C -d $D
  pulling '16e6c5ae0beee858c20c00828646da495a094d26' from 'test:server5'
  rebasing 01f209e23a69 "B"
  rebasing 5f76ba0bb512 "C"
  abort: path 'dir' is restricted by ACL 'some-acl'
  [255]

Rebase: ACL checks are repeated for the same restricted tree

  $ newserver server6
  $ drawdag << 'EOS'
  > E  # E/dir/.slacl = acl config
  >    # E/dir/file.txt = destination content
  >    # E/other.txt = original
  > |
  > | D  # D/other.txt = stack change 3
  > | |
  > | C  # C/other.txt = stack change 2
  > | |
  > | B  # B/other.txt = stack change 1
  > |/
  > A  # A/dir/file.txt = original
  >    # A/other.txt = original
  > EOS

  $ cd
  $ newclientrepo client6 server6
  $ sl go -q $D

FIXME: this should only check permissions for the restricted tree once.
  $ SL_LOG=eagerepo::api=debug sl rebase -r $B::$D -d $E 2>&1 | $PYTHON -c 'import sys; print("".join(line for line in sys.stdin if "check_manifest_permission" in line), end="")'
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
  DEBUG eagerepo::api: check_manifest_permission d4ef899346f65d1984b2a14db0f44f42df35d2d4
