#require test-repo execbit hg10

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "`dirname "$TESTDIR"`"

  $ testrepohg files . > "$TESTTMP/filelist"

  $ python << EOF
  > import os, stat
  > for path in open(os.path.join(os.environ["TESTTMP"], "filelist")).read().splitlines():
  >     if path.startswith("fb/"):
  >         continue
  >     content = open(path).read()
  >     isexec = bool(stat.S_IEXEC & os.stat(path).st_mode)
  >     ispy = path.endswith(".py")
  >     issh = path.endswith(".sh")
  >     isrs = path.endswith(".rs")
  >     if content.startswith("#!"):
  >         interpreter = os.path.basename(content.split("\n")[0].split()[-1])
  >     else:
  >         interpreter = None
  >     if ispy and isexec and interpreter not in {"python", "python2", "python3"}:
  >         print("%s is a Python script but does not have Python interpreter specified" % path)
  >     elif issh and isexec and interpreter not in {"sh", "bash", "zsh", "fish"}:
  >         print("%s is a Shell script but does not have Shell interpreter specified" % path)
  >     elif isexec and not interpreter:
  >         print("%s is executable but does not have #!" % path)
  >     elif not isexec and interpreter and not isrs:
  >         print("%s is not an executable but does have #!" % path)
  > EOF
  tests/fixtures/addspecial.sh is not an executable but does have #!
  tests/fixtures/mergeexternals.sh is not an executable but does have #!
  tests/fixtures/project_name_with_space.sh is not an executable but does have #!
  tests/fixtures/rename-closed-branch-dir.sh is not an executable but does have #!
  tests/infinitepush/library.sh is not an executable but does have #!
  tests/stresstest-atomicreplace.py is not an executable but does have #!
  tests/test-fb-hgext-cstore-treemanifest.py is a Python script but does not have Python interpreter specified
  tests/test-fb-hgext-cstore-uniondatapackstore.py is a Python script but does not have Python interpreter specified
