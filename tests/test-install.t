hg debuginstall
  $ hg debuginstall
  checking encoding (ascii)...
  checking Python lib (*lib*)... (glob)
  checking installed modules (*mercurial)... (glob)
  checking templates (*mercurial?templates)... (glob)
  checking commit editor...
  checking username...
  no problems detected

hg debuginstall with no username
  $ HGUSER= hg debuginstall
  checking encoding (ascii)...
  checking Python lib (*lib*)... (glob)
  checking installed modules (*mercurial)... (glob)
  checking templates (*mercurial?templates)... (glob)
  checking commit editor...
  checking username...
   no username supplied (see "hg help config")
   (specify a username in your configuration file)
  1 problems detected, please check your install!
  [1]
