  $ HGLEGACY= hg debugshell -c 'print(type(ui)); print(m.ui); print(__import__("mercurial.ui").ui)'
  <class 'mercurial.ui.ui'>
  <module 'mercurial.ui' from '*'> (glob)
  <module 'mercurial.ui' from '*'> (glob)

  $ HGLEGACY=ui hg debugshell -c 'print(type(ui)); print(m.ui); print(__import__("mercurial.ui").ui)'
  <class 'mercurial.legacyui.ui'>
  <module 'mercurial.legacyui' from '*'> (glob)
  <module 'mercurial.legacyui' from '*'> (glob)
