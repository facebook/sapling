Unlinking Files on Windows
==========================

This page describes what happens when Python's '``os.unlink(f)``' is called on Windows.

File opened using Python's "open"
---------------------------------

If the file f itself or *any hardlinked copy of f* has been opened for reading by another process using Python's '``open()``', then calling '``os.unlink(f)``' or '``os.rename(f, ..)``' will raise

::

   WindowsError: [Error 32] The process cannot access the file because it is being
   used by another process: <f>

This could be fixed in Microsoft's C runtime implementation by patching  the file open.c (VC8):

::

   diff --git a/open.c b/open.c
   --- a/open.c
   +++ b/open.c
   @@ -395,6 +395,9 @@

            *punlock_flag = 1;

   +        if (osplatform == VER_PLATFORM_WIN32_NT )
   +            fileshare  |= FILE_SHARE_DELETE;
   +
            /*
             * try to open/create the file
             */

and then making sure Python would use that modified C runtime. Python's '``open``' would then behave like Mercurial's '``posixfile``'.

File opened using Mercurial's "posixfile"
-----------------------------------------

If the file f has been opened for reading by another process with '``posixfile(f)``', calling '``os.rename(f, ..)``' succeeds.

Calling unlink will send that file into a "scheduled delete" state.

Scheduled delete has the following characteristics:

  (a) the entry in the directory for f is still kept

  (b) calling '``fd = posixfile(f, 'w')``' will raise '``IOError: [Errno 13] <f>: Access is denied``'

  (c) calling '``os.rename(f, f+'.foo')``' will raise '``WindowsError: [Error 5] Access is denied``'

  (d) calling '``os.lstat(f)``' will raise '``WindowsError: [Error 5] Access is denied: <f>``'

  (e) calling '``os.path.exists(f)``' returns False

Scheduled delete is left as soon as the other process closes the file.

