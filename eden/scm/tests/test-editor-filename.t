#chg-compatible
#debugruntest-compatible

Test temp file used with an editor has the expected suffix.

  $ hg init repo
  $ cd repo

Create an editor that writes its arguments to stdout and set it to $HGEDITOR.

  $ cat > editor.sh << EOF
  > echo "\$@"
  > exit 1
  > EOF
  $ hg add editor.sh
  $ HGEDITOR='sh "$TESTTMP/repo/editor.sh"'
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

Test ui.edit API for weird filenames that needs escape.

#if windows
('"' is not allowed in Windows filename)
  $ newrepo 'a && b'
#else
FIXME: Fails (in ui.loadrepoconfig) if path includes something like $HOME.
  $ newrepo 'a && " b"'
#endif

  $ pwd
  $TESTTMP/a && " b"
  $ cat >> edit.py << 'EOF'
  > import sys
  > paths = sys.argv[1:]
  > print(f"editor got {len(paths)} path(s)")
  > for path in paths:
  >     with open(path) as f:
  >         print(f"content: {f.read()}")
  > EOF

  $ HGEDITOR='python edit.py' hg debugshell << 'EOS'
  > repopath = repo.svfs.join("")
  > text = "<message for editor>"
  > user = "Foo Bar <foo@bar.com>"
  > ui.edit(text=text, user=user, repopath=repopath, action="test")
  > EOS
  editor got 1 path(s)
  content: <message for editor>

