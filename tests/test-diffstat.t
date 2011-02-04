  $ hg init repo
  $ cd repo
  $ i=0; while [ "$i" -lt 213 ]; do echo a >> a; i=`expr $i + 1`; done
  $ hg add a

Wide diffstat:

  $ hg diff --stat
   a |  213 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
   1 files changed, 213 insertions(+), 0 deletions(-)

diffstat width:

  $ COLUMNS=24 hg diff --config ui.interactive=true --stat
   a |  213 ++++++++++++++
   1 files changed, 213 insertions(+), 0 deletions(-)

  $ hg ci -m adda

  $ cat >> a <<EOF
  > a
  > a
  > a
  > EOF

Narrow diffstat:

  $ hg diff --stat
   a |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)

  $ hg ci -m appenda

  $ printf '\0' > b
  $ hg add b

Binary diffstat:

  $ hg diff --stat
   b |    0 
   1 files changed, 0 insertions(+), 0 deletions(-)

Binary git diffstat:

  $ hg diff --stat --git
   b |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

  $ hg ci -m createb

  $ printf '\0' > "file with spaces"
  $ hg add "file with spaces"

Filename with spaces diffstat:

  $ hg diff --stat
   file with spaces |    0 
   1 files changed, 0 insertions(+), 0 deletions(-)

Filename with spaces git diffstat:

  $ hg diff --stat --git
   file with spaces |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)
	
