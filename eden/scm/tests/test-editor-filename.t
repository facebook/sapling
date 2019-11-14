Test temp file used with an editor has the expected suffix.

  $ hg init

Create an editor that writes its arguments to stdout and set it to $HGEDITOR.

  $ cat > editor.sh << EOF
  > echo "\$@"
  > exit 1
  > EOF
  $ hg add editor.sh
  $ HGEDITOR="sh $TESTTMP/editor.sh"
  $ export HGEDITOR

Verify that the path for a commit editor has the expected suffix.

  $ hg commit
  *.commit.hg.txt (glob)
  abort: edit failed: sh exited with status 1
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
  abort: edit failed: sh exited with status 1
  [255]

Verify that when performing an action that has the side-effect of creating an
editor for a diff, the file ends in .diff.

  $ echo 1 > one
  $ echo 2 > two
  $ hg add
  adding one
  adding two
  $ hg commit --interactive --config ui.interactive=true --config ui.interface=text << EOF
  > y
  > e
  > q
  > EOF
  diff --git a/one b/one
  new file mode 100644
  examine changes to 'one'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +1
  record change 1/2 to 'one'? [Ynesfdaq?] e
  
  *.diff (glob)
  editor exited with exit code 1
  record change 1/2 to 'one'? [Ynesfdaq?] q
  
  abort: user quit
  [255]
