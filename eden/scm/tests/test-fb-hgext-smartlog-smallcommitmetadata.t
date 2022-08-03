  $ setconfig workingcopy.ruststatus=False
  $ configure modern
  $ enable smartlog
  $ newserver master
  $ cat >> .hg/hgrc <<EOF
  > [alias]
  > sl = smartlog -T '{sl}'
  > [templatealias]
  > sl_stablecommit = "{label('sl.stablecommit', smallcommitmeta('arcpull_stable'))}"
  > sl_hash_minlen = 8
  > sl_phase_label = "{ifeq(phase, 'public', 'sl.public', 'sl.draft')}"
  > sl_node = "{label(sl_phase_label, shortest(node, sl_hash_minlen))}"
  > sl = "{label('sl.label', separate('\n', sl_node, sl_stablecommit, '\n'))}"
  > EOF
  $ hg debugsmallcommitmetadata
  Found the following entries:
  $ echo "a" > a ; hg add a ; hg commit -qAm a
  $ echo "b" > b ; hg add b ; hg commit -qAm b
  $ echo "c" > c ; hg add c ; hg commit -qAm c

Add some metadata
  $ hg debugsmallcommitmetadata -r cb9a9f314b8b -c arcpull_stable stable
  $ hg debugsmallcommitmetadata -r d2ae7f538514 -c bcategory bvalue
  $ hg debugsmallcommitmetadata -r 177f92b77385 -c ccategory cvalue
  $ hg debugsmallcommitmetadata
  Found the following entries:
  cb9a9f314b8b arcpull_stable: 'stable'
  d2ae7f538514 bcategory: 'bvalue'
  177f92b77385 ccategory: 'cvalue'

Verify smartlog shows only the configured data
  $ hg debugsmallcommitmetadata
  Found the following entries:
  cb9a9f314b8b arcpull_stable: 'stable'
  d2ae7f538514 bcategory: 'bvalue'
  177f92b77385 ccategory: 'cvalue'
  $ hg sl
  @  177f92b7
  │
  o  d2ae7f53
  │
  o  cb9a9f31
     stable
  
  note: background backup is currently disabled so your commits are not being backed up.
