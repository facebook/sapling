Mercurial version 0.7 for Windows
---------------------------------

Welcome to Mercurial for Windows!

Mercurial is a command-line application.  You must run it from the
Windows command prompt (or if you're hard core, a MinGW shell).

By default, Mercurial installs to C:\Mercurial.  The Mercurial command
is called hg.exe.  To run this command, the install directory must be
in your search path.


Setting your search path temporarily
------------------------------------

To set your search path temporarily, type the following into a command
prompt window:

set PATH=C:\Mercurial;%PATH%


Setting your search path permanently
------------------------------------

To set your search path permanently, perform the following steps.
These instructions are for Windows NT, 2000 and XP.

1. Open the Control Panel.  Under Windows XP, select the "Classic
   View".

2. Enter the "System" control panel.

3. Click on "Advanced".

4. Click on "Environment Variables".

5. Under "System variables", you will see "Path".  Double-click it.

6. Edit "Variable value".  Each path element is separated by a
   semicolon (";") character.  Append a semicolon to the end of the
   list, followed by the path where you installed Mercurial
   (e.g. C:\Mercurial).

7. Click on the various "OK" buttons until you're back up to the top
   level.

8. Log out and log back in, or restart your system.

9. The next time you run the Windows command prompt, you will be able
   to run hg.exe.


Testing Mercurial
-----------------

The easiest way to check that Mercurial is installed properly is to
just type the following at the command prompt:

hg

It should print a help message.  If it does, it should work fine for
you.


Reporting problems
------------------

Before you report any problems, please consult the Mercurial web site
at http://www.selenic.com/mercurial and see if your question is
already in our list of Frequently Answered Questions (the "FAQ").

If you cannot find an answer to your question, please feel free to
send mail to the Mercurial mailing list, at <mercurial@selenic.com>.
Remember, the more useful information you include in your report, the
easier it will be for us to help you!

If you are IRC-savvy, that's usually the fastest way to get help.  Go
to #mercurial on irc.freenode.net.


Author and copyright information
--------------------------------

Mercurial was written by Matt Mackall, and is maintained by Matt and a
team of volunteers.

The Windows installer was written by Bryan O'Sullivan.

Copyright 2005 Matt Mackall and others.  See the CONTRIBUTORS.txt file
for a list of contributors.

Mercurial is free software, released under the terms of the GNU
General Public License, version 2.
