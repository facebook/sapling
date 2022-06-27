hg debuginstall
  $ hg debuginstall
  checking encoding (utf-8)...
  checking Python executable (*) (glob)
  checking Python version (*) (glob)
  checking Python lib (*lib*)... (glob)
  checking Python security support (*) (glob)
    TLS 1.2 not supported by Python install; network connections lack modern security (?)
    SNI not supported by Python install; may have connectivity issues with some servers (?)
  checking Mercurial version (*) (glob)
  checking Mercurial custom build (*) (glob)
  checking installed modules (*mercurial)... (glob)
  checking registered compression engines (*zlib*) (glob)
  checking available compression engines (*zlib*) (glob)
  checking available compression engines for wire protocol (*zlib*) (glob)
  checking commit editor (internal:none)
  checking username (test)
  no problems detected

hg debuginstall JSON
  $ hg debuginstall -Tjson | sed 's|\\\\|\\|g'
  [
   {
    "compengines": ["bz2", "bz2truncated", "none", "zlib"*], (glob)
    "compenginesavail": ["bz2", "bz2truncated", "none", "zlib"*], (glob)
    "compenginesserver": [*"zlib"*], (glob)
    "editor": "internal:none",
    "encoding": "utf-8",
    "encodingerror": null,
    "extensionserror": null,
    "hgmodules": "*mercurial", (glob)
    "hgver": "*", (glob)
    "hgverextra": "*", (glob)
    "problems": 0,
    "pythonexe": "*", (glob)
    "pythonlib": "*", (glob)
    "pythonsecurity": [*], (glob)
    "pythonver": "*.*.*", (glob)
    "username": "test",
    "usernameerror": null
   }
  ]

hg debuginstall with no username
  $ HGUSER= hg debuginstall
  checking encoding (utf-8)...
  checking Python executable (*) (glob)
  checking Python version (*) (glob)
  checking Python lib (*lib*)... (glob)
  checking Python security support (*) (glob)
    TLS 1.2 not supported by Python install; network connections lack modern security (?)
    SNI not supported by Python install; may have connectivity issues with some servers (?)
  checking Mercurial version (*) (glob)
  checking Mercurial custom build (*) (glob)
  checking installed modules (*mercurial)... (glob)
  checking registered compression engines (*zlib*) (glob)
  checking available compression engines (*zlib*) (glob)
  checking available compression engines for wire protocol (*zlib*) (glob)
  checking commit editor (internal:none)
  checking username...
   no username supplied
   (specify a username in your configuration file)
  1 problems detected, please check your install!
  [1]

hg debuginstall with invalid encoding
  $ HGENCODING=invalidenc hg debuginstall | grep encoding
  checking encoding (invalidenc)...
   unknown encoding: invalidenc

exception message in JSON

  $ HGENCODING=invalidenc HGUSER= hg debuginstall -Tjson | grep error
    "encodingerror": "unknown encoding: invalidenc",
    "extensionserror": null,
    "usernameerror": "no username supplied"

path variables are expanded (~ is the same as $TESTTMP)
  $ mkdir tools
  $ touch tools/testeditor.exe
#if execbit
  $ chmod 755 tools/testeditor.exe
#endif
  $ hg debuginstall --config ui.editor=~/tools/testeditor.exe
  checking encoding (utf-8)...
  checking Python executable (*) (glob)
  checking Python version (*) (glob)
  checking Python lib (*lib*)... (glob)
  checking Python security support (*) (glob)
    TLS 1.2 not supported by Python install; network connections lack modern security (?)
    SNI not supported by Python install; may have connectivity issues with some servers (?)
  checking Mercurial version (*) (glob)
  checking Mercurial custom build (*) (glob)
  checking installed modules (*mercurial)... (glob)
  checking registered compression engines (*zlib*) (glob)
  checking available compression engines (*zlib*) (glob)
  checking available compression engines for wire protocol (*zlib*) (glob)
  checking commit editor (internal:none)
  checking username (test)
  no problems detected

