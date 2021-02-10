#chg-compatible

  $ configure modern

  $ newserver server

Populate commits

  $ clone server client1
  $ cd client1
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg push -r $B --to master --create -q

Check stream clone "pull"

  $ hg clone --shallow -U "ssh://user@dummy/server" client2 --debug
  running /home/quark/bin/python3 /data/users/quark/fbsource/fbcode/eden/scm/tests/dummyssh 'user@dummy' 'hg -R server serve --stdio'
  sending hello command
  sending between command
  remote: 658
  remote: capabilities: treeonly unbundle=HG10GZ,HG10BZ,HG10UN unbundlereplay streamreqs=generaldelta,lz4revlog,revlogv1 knownnodes pushkey batch listkeyspatterns branchmap bundle2=HG20%0Ab2x%253Ainfinitepush%0Ab2x%253Ainfinitepushmutation%0Ab2x%253Ainfinitepushscratchbookmarks%0Abookmarks%0Achangegroup%3D01%2C02%2C03%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Alistkeys%0Aphases%3Dheads%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps%0Atreemanifest%3DTrue%0Atreemanifestserver%3DTrue%0Atreeonly%3DTrue known unbundlehash lookup getbundle changegroupsubset stream_option gettreepack remotefilelog getflogheads getfile
  remote: 1
  fetching changelog
  running /home/quark/bin/python3 /data/users/quark/fbsource/fbcode/eden/scm/tests/dummyssh 'user@dummy' 'hg -R server serve --stdio'
  sending hello command
  sending between command
  remote: 658
  remote: capabilities: treeonly unbundle=HG10GZ,HG10BZ,HG10UN unbundlereplay streamreqs=generaldelta,lz4revlog,revlogv1 knownnodes pushkey batch listkeyspatterns branchmap bundle2=HG20%0Ab2x%253Ainfinitepush%0Ab2x%253Ainfinitepushmutation%0Ab2x%253Ainfinitepushscratchbookmarks%0Abookmarks%0Achangegroup%3D01%2C02%2C03%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Alistkeys%0Aphases%3Dheads%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps%0Atreemanifest%3DTrue%0Atreemanifestserver%3DTrue%0Atreeonly%3DTrue known unbundlehash lookup getbundle changegroupsubset stream_option gettreepack remotefilelog getflogheads getfile
  remote: 1
  sending stream_out_shallow command
  3 files to transfer, 465 bytes of data
  adding 00manifesttree.i (227 bytes)
  adding 00changelog.i (128 bytes)
  adding 00changelog.d (110 bytes)
  transferred 465 bytes in 0.0 seconds (454 KB/sec)
  fetching selected remote bookmarks
  reusing connection from pool
  preparing listkeys for "bookmarks" with pattern "['master']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 47 bytes
  no changes found
  sending getbundle command
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "bookmarks" supported
  bundle2-input-part: total payload size 28
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size 24
  bundle2-input-bundle: 1 parts total
