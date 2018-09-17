Command names are sent over the ssh pipe as plain text, followed by a *single character* linebreak. This is important on systems that automatically use a two character line-break, such as CR+LF on Windows: if there is extra whitespace on the end of the command (in the case of windows, there will be an extra CR at the end), it will not be recognized.

Arguments are sent as ``[argname] [value length]\n``, followed by the value. Responses are ``[response length]\n``, followed by the response.

Example:

To issue the "lookup" command on the key "tip", the client issues the following:

::

   lookup
   key 3
   tip

And the server might respond:

::

   25
   1 9b4a87d1a1c9c3577b12990ce5819e2955347083

Version detection
,,,,,,,,,,,,,,,,,

Because older Mercurial versions give no/zero-length responses to unknown commands, you must first send the ``hello`` command followed by a command with known output, and then determine if the ``hello`` command responded before the known output was sent.

Over STDIO
~~~~~~~~~~

You can use the SSH command protocol over stdio with the following command:

::

   hg serve --stdio

You can now type or otherwise send your client commands to the server directly through its STDIN stream, and it will respond on its STDOUT stream.


