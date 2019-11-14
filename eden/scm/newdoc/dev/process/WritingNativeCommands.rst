Writing Native Commands
=======================

Quick Start
-----------

First, define flags. This is similarily magical as ``structopt``:

.. sourcecode:: rust

    use cliparser::define_flags;

    define_flags! {
        struct FooOpts {
            /// help doc of shared
            shared: bool,

            /// a flag with a short name and default value
            #[short('f')]
            foo_bar: bool = true,

            /// revisions
            #[short('r')]
            revs: Vec<String>,

            #[arg]
            first: String,

            #[arg]
            second: String,

            #[args]
            rest: Vec<String>,
        }
    }

Fields with ``#[arg]`` or ``#[args]`` attribute are optional. If they are
missing, the command wouldn't accept positional arguments. There is another
attribute - ``#[command_name]``. It provides the "arg0" and may be useful
for cases where mutliple commands share a same implementation.

Then, define the function body.

.. sourcecode:: rust

    pub fn foo(flags: FooOpts, io: &mut IO, repo: Repo) -> Result<u8, DispatchError> {
        // use io.write to output
        io.write(format!("args: {} {} {:?}\n", opts.first, opts.second, opts.args));
        io.write_err(format!("foo_bar: {:?}\n", flags.foo_bar));
    }

``repo: Repo`` indicates a repo is required. For commands not requiring a repo,
just remove the argument:

.. sourcecode:: rust

    pub fn foo(flags: FooOpts, io: &mut IO) -> Result<u8, DispatchError> {
         // ...
    }

Finally, register the command to the command table:

.. sourcecode:: rust

    // search table.register to find the place to change
    table.register(foo, "foo", "help text of foo");

It's done. Recompile and the ``foo`` command will be available.


Flag Definitions
----------------

A flag definition is a type alias for a tuple which mimics the Python API for
conciseness and familiarity.

The Python flag definitions look like:

.. sourcecode:: python

    ( short_name, long_name, default_value, description, display_value )

The Rust ``Flag`` type implements convertion from Rust tuples that look like
the Python tuple:

.. sourcecode:: rust

    ( short_name, long_name, description, default_value )

Value Enum
~~~~~~~~~~

To achieve an API where we are able to deal with various types elegantly, we
define a ``Value`` enum with variants representing the supported types.  All
variants accept a default param.  Currently types supported are:


===============    ============================================================
Variant            Usage
===============    ============================================================
``Value::Str``     Expecting a single String argument e.g. ``--foo "bar"``
``Value::Bool``    Expecting either a True / False value ( supports no-prefix )
                   e.g. ``--foo`` or ``--no-foo``
``Value::Int``     Expecting a number ( i64 ) e.g. ``--foo 5``
``Value::List``    Expecting multiple String arguments e.g. ``--foo "bar"
                   --foo "baz"``
===============    ============================================================

There is a final variant ``Value::OptBool``, however this was created as a
compatibility layer between the existing Python code that uses ``None`` as a
default value.  It is currently not expected that this should be used when writing
native commands.

Real Example
~~~~~~~~~~~~

.. note::

   You won't need to define ``Flag`` manually if you're using the
   ``define_flags!`` macro.

In Python we can have a definition such as ``--noninteractive``

.. sourcecode:: python

    ( "y", "noninteractive", False, _("do not prompt...") )

Translating this definition to Rust we would end up with:

.. sourcecode:: rust

    let flag: Flag = ('y', "noninteractive", "do not prompt...", false).into();

If there is no ``short_name`` for a flag then pass an empty character literal
e.g. ``' '`` in the ``short_name`` place.

Command Definitions
-------------------

Command definitions are essentially metadata about all Mercurial commands.  The
reason that Rust must know about *all* commands and not simply Rust-only commands
is to be able to correctly prefix match on aliases and commands alike.  Commands
loaded from ``commands.names`` config option are marked as Python and only kept for their name.  Commands
actually defined in Rust are of much more interest.  The definition contains
the name, doc, flags and the function body of a command.

.. note::

   You shouldn't use ``CommandDefinition`` directly.
   Use ``dispatcher.register`` instead.

.. sourcecode:: rust

    let def = CommandDefinition::new(name, doc, flags, func);


Command Handlers
----------------

Command handlers are where actual command logic lives.  handlers have one of three
specific function signatures that imply what type of commands they are, which is
an implicit version of what Python does with ``inferrepo=True``.

Command Types
~~~~~~~~~~~~~

* ``Repo`` | ``(Opts, &mut IO, Repo) -> Result<u8, DispatchError>``
* ``InferRepo`` | ``(Opts, &mut IO, Option<Repo>) -> Result<u8, DispatchError>``
* ``NoRepo`` | ``(Opts, &mut IO) -> Result<u8, DispatchError>``

By changing your command handler, you are able to select where / when this command
would be made available to the user.  Some commands require that they are executed
from a Repo while some commands do not need a Repo at all.

Defining The Arguments
~~~~~~~~~~~~~~~~~~~~~~

There are a possible total of 4 arguments being passed into command handlers:

``Opts``: This argument is the most similar to the ``opts`` and ``args``
arguments in the Python codebase. It is mostly likely crated by the
``define_flags!`` macro.

``&mut IO``: IO is most similar to the ``UI`` object from Python, without the
god class features.  It is simply a layer to write and read from stdin / stdout
( and since it accepts any Read / Write trait object, it can be very flexible ).
Currently, using the IO object for its ``write_str`` method would be the most
common and would print to the terminal from inside the handler.

``Repo``: Repo is the struct of the repo itself.  Currently, it only has a path
to the root of the repo, as well as a method ``sharedpath`` that will return the
sharedpath of the repo.  It also has the configuration that was loaded from that repo.
As more information about a repo becomes necessary, this struct can be modified
and hold this type of information.

Error Handler In Command Handler
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Handling errors is made very easy as all command handlers return a ``Result<u8, DispatchError>``.
The ``u8`` is the return code.  ``DispatchError`` allows a command to return
an error that may either end the execution chain, or fallback to Python.  This is
useful for incrementally replacing behavior with a Native fast path, and allowing
Python to handle legacy flags or complex features not ready to be switched fully
to Rust.

Add a new variant to DispatchError and modify the HighLevelError ``From<DispatchError>``
to decide what should happen.  In general, if the Python would not be able to handle
anything in a better way, having the Rust end the execution is preferable.
Especially in cases where the command is only backed by Rust and Python may not
be able to handle anything command specific ( aside from ``help`` ).

Dispatcher
----------

``clidispatch::dispatch::Dispatcher`` is the struct that allows command registration,
and dispatching command line arguments.  Usage is very simple, and the correct
version of ``register`` will be called based on the function signature of
your command handler.

See :ref:`Quick Start` for code example.

``dispatcher.dispatch(args)`` will handle all of the parsing, calling the
correct handler, and if the command is not backed by Rust, it will fallback to
Python automatically.

Dispatch Properties
~~~~~~~~~~~~~~~~~~~

Dispatching mimics Mercurial's current Python dispatch.  This means that it can:

* Early parse global flags
* Handle ``--cwd``
* Handle ``-R``, ``--repo``, ``--repository``
* Load system, user, and repo configuration
* Handle aliases
* Handle defaults
* Handle command specific errors ( i.e. not in a repository but the handler requires one )
* Dispatching to the command handler
* Falling back to Python or exiting after either a success case or an error that should not go to Python

If ``-h`` or ``--help`` flags are found it will go to Python for help handling,
which **does** work with Rust-only commands.
