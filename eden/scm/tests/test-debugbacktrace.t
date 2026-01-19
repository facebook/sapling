#inprocess-hg-incompatible
#chg-incompatible
#require lldb

This test requires:
- real processes (therefore inprocess-hg-incompatible)
- python 3.10 or 3.12+ (sapling_cext_evalframe_resolve_frame in cext/evalframe.c is currently only implemented for these versions)
- lldb (used by the debugbacktrace command)

Check python version:

    py_version = sys.version_info[:2]
    if not (py_version == (3, 10) or py_version >= (3, 12)):
        $ exit 80

Run debugshell Python logic:

  $ cat > script.py << EOF
  > import os, time
  > def my_unique_function_name_for_test():
  >     with open('pid.tmp', 'wb') as f: f.write(str(os.getpid()).encode())
  >     os.rename('pid.tmp', 'pid')
  >     while not os.path.exists('done'):
  >         time.sleep(0.1)
  > my_unique_function_name_for_test()
  > EOF

  $ sl debugshell script.py &

Wait for the pid file:

  $ while ! [ -f pid ]; do sleep 0.1; done

Backtrace should include the Python function name:

  $ sl debugbacktrace `cat pid` > out 2>/dev/null
  $ grep my_unique_function_name_for_test out
  *my_unique_function_name_for_test* (glob)

Tell the debugshell process to exit:

  $ touch done

  $ wait
