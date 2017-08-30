Test temp file used with an editor has the expected suffix.

  $ hg init

Create an editor that writes its arguments to stdout and set it to $HGEDITOR.

  $ cat > editor.sh << EOF
  > #!/bin/bash
  > echo "\$@"
  > exit 1
  > EOF
  $ chmod +x editor.sh
  $ hg add editor.sh
  $ HGEDITOR=$TESTTMP/editor.sh
  $ export HGEDITOR

Verify that the path for a commit editor has the expected suffix.

  $ hg commit
  *.commit.hg.txt (glob)
  abort: edit failed: editor.sh exited with status 1
  [255]

Verify that the path for a histedit editor has the expected suffix.

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > histedit=
  > EOF
  $ hg commit --message 'At least one commit for histedit.'
  $ hg histedit
  *.histedit.hg.txt (glob)
  abort: edit failed: editor.sh exited with status 1
  [255]
