
#require no-eden

  $ setconfig scmstore.fetch-tree-aux-data=true
  $ setconfig scmstore.tree-metadata-mode=always
  $ setconfig experimental.restricted-tree-mode=enforced
  $ setconfig slacl.server-acl-enforcement=true
  $ enable rebase

  $ newserver server
  $ drawdag << 'EOS'
  > A  # A/regular/file.txt = regular content
  >    # A/restricted/.slacl = acl config
  >    # A/restricted/secret.txt = secret content
  > EOS

  $ sl clone --config clone.use-rust=True --config format.use-eager-repo=false --config format.use-remotefilelog=true --config remotefilelog.reponame=client -q "test:server" "$TESTTMP/client"
  $ cd "$TESTTMP/client"

  $ sl debugmanifestdirs -qr $A
  19d1f9c4aa6e6b299fa6a863b253889df872ae0f restricted
  7336b5d3a2867d97ff2b64af2b848b76ac7e7f39 regular
  7ebc6a0e1746ead2f3778301c440cde7eec58620 /
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

Don't attempt to fetch 19d1f9c4 - it is restricted
  $ LOG=tree_fetches=trace sl go -q $A
  TRACE tree_fetches: attrs=["content"] keys=["@7ebc6a0e"]
  TRACE tree_fetches: attrs=["content"] keys=["@7336b5d3"]
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

  $ find .
  ./A
  ./regular
  ./regular/file.txt

Give a specific message when referencing a restricted file:
  $ sl cat restricted/secret.txt
  restricted/secret.txt: restricted path
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

Doesn't having warning since it uses dirstate to walk.
  $ sl files restricted/secret.txt
  restricted/secret.txt: restricted path
  [1]

  $ sl files -r . restricted/secret.txt
  restricted/secret.txt: restricted path
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

Experimental fallback can treat matching Python manifest lookups as not found:
  $ cat > check_manifest_get.py <<'PY'
  > from sapling import error, hg, ui as uimod
  > repo = hg.repository(uimod.ui.load(), ".")
  > try:
  >     print(repo["."].manifest().get("restricted/secret.txt", b"missing"))
  > except error.PermissionDeniedError:
  >     print("permission denied")
  > PY
  $ sl debugpython -- check_manifest_get.py
  permission denied
  $ setconfig experimental.slacl-ignore-permission-denied-regex=check_manifest_get.py
  $ sl debugpython -- check_manifest_get.py
  b'missing'

Make sure root tree has acl indices populated in cache
  $ sl debugscmstore -r $A '' --mode=tree | grep -A 4 acl_children_indices
                          acl_children_indices: Some(
                              [
                                  2,
                              ],
                          ),

After committing a change outside the restricted directory, the new root
tree should still have acl_children_indices for the unchanged restricted directory:
  $ echo modified > regular/file.txt
  $ sl commit -m 'modify regular file'
  $ sl debugscmstore -r . '' --mode=tree | grep -A 4 acl_children_indices
                          acl_children_indices: Some(
                              [
                                  2,
                              ],
                          ),

Rust commands also warn about restricted paths:
  $ sl grep --config grep.use-rust=true -r $A 'content'
  regular/file.txt:regular content
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

Matcher-scoped BFS should not check ACLs under directories it will not visit:

  $ newserver server2
  $ drawdag << 'EOS'
  > A  # A/some_dir/public.txt = public content
  >    # A/some_dir/secret/.slacl = acl config
  >    # A/some_dir/secret/private.txt = private content
  > EOS

  $ cd
  $ newclientrepo client2 server2

No permission check is needed for `some_dir/secret` when only listing `some_dir/public.txt`.
  $ SL_LOG=eagerepo::api=debug sl files -r $A some_dir/public.txt 2>&1 | grep check_manifest_permission || true

Diff stat from restricted to unrestricted currently aborts instead of filtering
the restricted side.

  $ newserver server3
  $ drawdag << 'EOS'
  > B  # B/public.txt = public v2
  >    # B/restricted/.slacl = acl config
  >    # B/restricted/secret.txt = secret v2
  > |
  > A  # A/public.txt = public v1
  >    # A/restricted/secret.txt = secret v1
  > EOS

  $ cd
  $ newclientrepo client3 server3
  $ sl go -q $A

  $ sl diff --stat -r $A -r $B
  pulling '9ca8bdfec14241f36077c849d46b93e38afd340c' from 'test:server3'
   A                     |  1 -
   B                     |  1 +
   public.txt            |  2 +-
   restricted/secret.txt |  1 -
   4 files changed, 2 insertions(+), 3 deletions(-)
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl diff --stat -r $B -r $A
   A                     |  1 +
   B                     |  1 -
   public.txt            |  2 +-
   restricted/secret.txt |  1 +
   4 files changed, 3 insertions(+), 2 deletions(-)
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

Backout of a commit that touches restricted and unrestricted paths:

  $ newserver server4
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  >   # C/restricted/.slacl = acl config
  >   # B/public.txt = public v2
  >   # B/restricted/secret.txt = secret v2
  >   # A/public.txt = public v1
  >   # A/restricted/secret.txt = secret v1
  > EOS

  $ cd
  $ newclientrepo client4 server4
  $ sl go -q $C
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

  $ sl backout -r $B --config ui.interactive=True << 'EOF'
  > u
  > EOF
  other changed restricted/secret.txt which is restricted in local
  (d)elete/drop this file, input (m)oved path, or leave (u)nresolved? u
  1 files updated, 0 files merged, 1 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl status
  M public.txt
  R B
  ! restricted/secret.txt
  $ sl resolve --tool internal:local restricted/secret.txt
  (no more unresolved files)
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl cat public.txt
  public v2 (no-eol)

Restricted-path conflicts can be resolved by dropping the inaccessible path:

  $ cd
  $ newclientrepo client5 server4
  $ sl go -q $C
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl backout -r $B --config ui.interactive=True << 'EOF'
  > d
  > EOF
  other changed restricted/secret.txt which is restricted in local
  (d)elete/drop this file, input (m)oved path, or leave (u)nresolved? d
  1 files updated, 1 files merged, 1 files removed, 0 files unresolved
  changeset * backs out changeset * (glob)
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl status
  $ sl resolve -l
  $ sl cat public.txt
  public v1 (no-eol)

Restricted-path conflicts can be resolved by moving the file to a visible path:

  $ cd
  $ newclientrepo client6 server4
  $ sl go -q $C
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl backout -r $B --config ui.interactive=True << 'EOF'
  > m
  > moved-secret.txt
  > EOF
  other changed restricted/secret.txt which is restricted in local
  (d)elete/drop this file, input (m)oved path, or leave (u)nresolved? m
  move path 'restricted/secret.txt' to [what path relative to repo root] ? moved-secret.txt
  1 files updated, 1 files merged, 1 files removed, 0 files unresolved
  changeset * backs out changeset * (glob)
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]
  $ sl status
  $ sl cat moved-secret.txt
  secret v1 (no-eol)
  $ sl resolve -l

Backout of a commit that touches restricted paths the user never had access to:

  $ newserver server5
  $ drawdag << 'EOS'
  > B
  > |
  > A
  >   # A/public.txt = public v1
  >   # A/restricted/.slacl = acl config
  >   # A/restricted/secret.txt = secret v1
  >   # B/public.txt = public v2
  >   # B/restricted/secret.txt = secret v2
  > EOS

  $ cd
  $ newclientrepo client7 server5
  $ sl go -q $B
  warning: results may be incomplete due to path ACLs
    'restricted' is restricted by ACL 'some-acl'
  [1]

FIXME: should avoid leaving a partial backout. There is no per-file conflict to
present here because the restricted file names are not visible.
  $ sl backout -r $B
  removing B
  reverting public.txt
  abort: path 'restricted' is restricted by ACL 'some-acl'
  [255]
  $ sl status
  M public.txt
  R B
  $ sl cat public.txt
  public v2 (no-eol)
