hg debuginstall
  $ hg debuginstall
  Checking encoding (ascii)...
  Checking installed modules (*/mercurial)... (glob)
  Checking templates (*/mercurial/templates)... (glob)
  Checking commit editor...
  Checking username...
  No problems detected

hg debuginstall with no username
  $ HGUSER= hg debuginstall
  Checking encoding (ascii)...
  Checking installed modules (*/mercurial)... (glob)
  Checking templates (*/mercurial/templates)... (glob)
  Checking commit editor...
  Checking username...
   no username supplied (see "hg help config")
   (specify a username in your configuration file)
  1 problems detected, please check your install!
  [1]
