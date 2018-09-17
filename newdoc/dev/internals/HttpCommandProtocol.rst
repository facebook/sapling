HTTP commands are sent as CGI requests having the following form:

::

   GET /hgweb.cgi?cmd=foo&param1=bar HTTP/1.1

Results are returned with the Content-Type application/mercurial-0.1.

The best way to explore the protocol is to run ``hg serve`` in a terminal, then try out the various commands. Errors, such as missing parameters, will be logged in the terminal window, including source references.

Available commands
==================

The available commands can be seen at the end of ``mercurial/wireproto.py``, along with arguments.

lookup
~~~~~~

Given a changeset reference (given by the ``key`` parameter), yields the changeset ID.

Returns a status code (1 on success, 0 on failure) and a result (the changeset ID or error message).

Examples:

::

   $ curl 'http://selenic.com/hg/?cmd=lookup&key=0'
   1 9117c6561b0bd7792fa13b50d28239d51b78e51f

   $ curl 'http://selenic.com/hg/?cmd=lookup&key=33d290cc14ae48c8c18d2a2c9dfae99728ee0cff'
   0 unknown revision '33d290cc14ae48c8c18d2a2c9dfae99728ee0cff'

   $ curl 'http://selenic.com/hg/?cmd=lookup&key=tip'
   1 55724f42fa14b6759a47106998feea25a032e45c

heads
~~~~~

Returns a space separated list of `changeset ID`_ identifying all the heads in the repository. Takes no parameters

branches
~~~~~~~~

changegroup
~~~~~~~~~~~

changegroupsubset
~~~~~~~~~~~~~~~~~

between
~~~~~~~

capabilities
~~~~~~~~~~~~

Accepts no parameters. Returns a whitespace-separated list of other commands accepted by this server. For the *unbundle* command, produces the form unbundle=HG10GZ,HG10BZ,HG10UN if all three compression schemes are supported.

unbundle
~~~~~~~~

Usage:

::

   POST /hgweb.cgi?cmd=unbundle&heads=HEADS HTTP/1.1
   content-type: application/octet-stream

 

This command allows for the upload of new changes to the repository. The body of the POST request must be a changegroup in bundle format. The returned output is the same as what the *hg unbundle* command would print to standard output if it was being run locally.

stream_out
~~~~~~~~~~

