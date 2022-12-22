#chg-compatible
#debugruntest-compatible

  $ enable remotenames

Init a repo

  $ hg init pathsrepo
  $ cd pathsrepo

Check that a new path can be added

  $ hg paths -a yellowbrickroad yellowbrickroad
  $ hg paths -a stairwaytoheaven stairwaytoheaven
  $ hg paths
  stairwaytoheaven = $TESTTMP/pathsrepo/stairwaytoheaven
  yellowbrickroad = $TESTTMP/pathsrepo/yellowbrickroad

Check that a repo can be deleted

  $ hg paths -d yellowbrickroad
  $ hg paths
  stairwaytoheaven = $TESTTMP/pathsrepo/stairwaytoheaven

Delete .hg/hgrc fil

  $ rm .hg/hgrc

Non-existence of config file does not change behavior:

  $ hg paths -d stairwaytoheaven

Check that a path can be added when no .hg/hgrc file exists

  $ hg paths -a yellowbrickroad yellowbrickroad
  $ hg paths
  yellowbrickroad = $TESTTMP/pathsrepo/yellowbrickroad

Helpful error with wrong args:

  $ hg paths -a banana
  abort: invalid URL - invoke as 'hg paths -a NAME URL'
  [255]

  $ hg paths -a banana too many
  abort: invalid URL - invoke as 'hg paths -a NAME URL'
  [255]
