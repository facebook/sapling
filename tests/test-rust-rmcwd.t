#require rmcwd

(rmcwd is incompatible with Python tests right now - os.getcwd() will fail)

  $ A=$TESTTMP/a
  $ mkdir $A
  $ cd $A

Removed cwd

  $ rmdir $A

Note: With buck build the hg script can be a wrapper that runs shell commands.
That can print extra noisy outputs like " shell-init: error retrieving current
directory: getcwd: cannot access parent directories". So we skip this test for
buck build.

#if normal-layout
  $ hg debug-args
  abort: cannot get current directory: * (glob)
  [74]

#endif

Recreated cwd

  $ mkdir $A
  $ hg debug-args a
  (warning: the current directory was recrated, consider running 'cd $PWD' to fix your shell)
  ["a"]
