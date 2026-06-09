
#require no-eden

  $ newserver server
  $ drawdag << 'EOS'
  > A  # A/regular/file.txt = regular content
  >    # A/restricted/.slacl = acl config
  >    # A/restricted/secret.txt = secret content
  > EOS

  $ sl clone --config clone.use-rust=True --config format.use-eager-repo=false --config format.use-remotefilelog=true --config remotefilelog.reponame=client -q "test:server" "$TESTTMP/client"
  $ cd "$TESTTMP/client"
  $ setconfig scmstore.fetch-tree-aux-data=true
  $ setconfig scmstore.tree-metadata-mode=always
  $ setconfig experimental.restricted-tree-mode=enforced
  $ setconfig slacl.server-acl-enforcement=true

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
  $ setconfig scmstore.fetch-tree-aux-data=true
  $ setconfig scmstore.tree-metadata-mode=always
  $ setconfig experimental.restricted-tree-mode=enforced
  $ setconfig slacl.server-acl-enforcement=true

FIXME: this should not check permissions for `some_dir/secret` when only listing `some_dir/public.txt`.
  $ SL_LOG=eagerepo::api=debug sl files -r $A some_dir/public.txt 2>&1 | grep check_manifest_permission || true
  DEBUG eagerepo::api: check_manifest_permission e447ed9c329f28d36d5bfef61352650580015dc3
