# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Base case, check can walk fine
  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Seen,Loaded: 40,40
  Bytes/s,* (glob)
  Walked* (glob)

Delete a gitsha1 alias so that we get errors
  $ ls blobstore/blobs/* | wc -l
  30
  $ rm blobstore/blobs/*.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c*
  $ ls blobstore/blobs/* | wc -l
  29

Check we get an error due to the missing aliases
  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Execution error: Could not step to OutgoingEdge { label: FileContentMetadataToGitSha1Alias, target: AliasContentMapping(AliasKey(GitSha1(GitSha1(96d80cd6c4e7158dbebd0849f4fb7ce513e5828c)))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c is missing
  Error: Execution failed

Check error as data fails if not in readonly-storage mode
  $ mononoke_walker --with-readonly-storage=false scrub --error-as-data-node-type AliasContentMapping -I deep -q -b master_bookmark 2>&1 | strip_glog
  Execution error: Error as data could mean internal state is invalid, run with --with-readonly-storage=true to ensure no risk of persisting it
  Error: Execution failed

Check counts with error-as-data-node-type
  $ mononoke_walker --scuba-dataset file://scuba.json -l loaded scrub -q --error-as-data-node-type AliasContentMapping -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types []
  1 Could not step to
  1 Seen,Loaded: 40,39

Check scuba data
  $ wc -l < scuba.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .edge_type, .node_key, .node_type, .repo, .walk_type ] | @csv' < scuba.json | sort
  1,"missing","FileContentMetadataToGitSha1Alias","alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","AliasContentMapping","repo","scrub"

Check error-as-data-edge-type, should get an error on FileContentMetadataToGitSha1Alias as have not converted its errors to data
  $ mononoke_walker -l loaded scrub -q --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataToSha1Alias -I deep -b master_bookmark 2>&1 | strip_glog
  Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataToSha1Alias]
  Execution error: Could not step to OutgoingEdge { label: FileContentMetadataToGitSha1Alias, target: AliasContentMapping(AliasKey(GitSha1(GitSha1(96d80cd6c4e7158dbebd0849f4fb7ce513e5828c)))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c is missing
  Error: Execution failed

Check error-as-data-edge-type, should get no errors as FileContentMetadataToGitSha1Alias has its errors converted to data
  $ mononoke_walker -l loaded scrub -q --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataToGitSha1Alias -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataToGitSha1Alias]
  1 Could not step to
  1 Seen,Loaded: 40,39
