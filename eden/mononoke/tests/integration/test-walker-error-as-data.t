# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Base case, check can walk fine
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Delete all gitsha1 aliases so that we get errors
  $ ls blobstore/blobs/* | wc -l
  30
  $ rm blobstore/blobs/*.alias.gitsha1.*
  $ ls blobstore/blobs/* | wc -l
  27

Check we get an error due to the missing aliases
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Execution error: Could not step to OutgoingEdge { label: FileContentMetadataToGitSha1Alias, target: AliasContentMapping(GitSha1(GitSha1 { sha1: "96d80cd6c4e7158dbebd0849f4fb7ce513e5828c", ty: "blob", size: 1 })) }
  * (glob)
  Caused by:
      Blob is missing: alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c
  Error: Execution failed

Check error as data fails if not in readonly-storage mode
  $ mononoke_walker --storage-id=blobstore scrub --error-as-data-node-type AliasContentMapping -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Execution error: Error as data could mean internal state is invalid, run with --readonly-storage to ensure no risk of persisting it
  Error: Execution failed

Check counts with error-as-data-node-type
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub --error-as-data-node-type AliasContentMapping -I deep -q --bookmark master_bookmark --scuba-log-file=scuba.json 2>&1 | strip_glog | sed -re 's/^(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  1 Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types []
  3 Could not step to
  1 Final count: (37, 34)
  1 Walked* (glob)

Check scuba data
  $ wc -l < scuba.json
  3
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .edge_type, .node_key, .node_type, .repo, .walk_type ] | @csv' < scuba.json | sort
  1,"step","FileContentMetadataToGitSha1Alias","alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54","AliasContentMapping","repo","scrub"
  1,"step","FileContentMetadataToGitSha1Alias","alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220","AliasContentMapping","repo","scrub"
  1,"step","FileContentMetadataToGitSha1Alias","alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","AliasContentMapping","repo","scrub"

Check error-as-data-edge-type, should get an error on FileContentMetadataToGitSha1Alias as have not converted its errors to data
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataToSha1Alias -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataToSha1Alias]
  Execution error: Could not step to OutgoingEdge { label: FileContentMetadataToGitSha1Alias, target: AliasContentMapping(GitSha1(GitSha1 { sha1: "96d80cd6c4e7158dbebd0849f4fb7ce513e5828c", ty: "blob", size: 1 })) }
  * (glob)
  Caused by:
      Blob is missing: alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c
  Error: Execution failed

Check error-as-data-edge-type, should get no errors as FileContentMetadataToGitSha1Alias has its errors converted to data
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataToGitSha1Alias -I deep -q --bookmark master_bookmark 2>&1 | strip_glog | sed -re 's/^(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  1 Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataToGitSha1Alias]
  3 Could not step to
  1 Final count: (37, 34)
  1 Walked* (glob)
