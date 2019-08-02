Writing Native Commands
=======================

The Rust library ``clidispatch`` found at ``scm/hg/lib/clidispatch`` is a library
that mimics Mercurial's Python's ``dispatch`` logic, allowing command registration,
command resolution, command line parsing, and handing execution off to a command
handler to satisfy the end-user request.  Writing native commands allows requests
to be satisfied without having to invoke the Python interpreter, thus creating a
faster response time.  This library aims to eventually replace the seemingly hacky
behavior of the telemetry wrapper ( ``scm/hg/telemetry/telemetry`` ) and allow
more complex commands to be written in Rust.

Flag Definitions
----------------

A flag definition is a type alias for a tuple which mimics the Python API for
conciseness and familiarity.

The Python flag definitions look like:

.. sourcecode:: python

    ( short_name, long_name, default_value, description, display_value )

The Rust flag definitions look like:

.. sourcecode:: rust

    ( short_name, long_name, description, default_value )

The actual type alias of the Rust definition is:

.. sourcecode:: rust

    let FlagDefinition<'a> = (char, Cow<'a, str>, Cow<'a, str>, Value);

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

In Python we can have a definition such as ``--noninteractive``

.. sourcecode:: python

    ( "y", "noninteractive", False, _("do not prompt...") )
    
Translating this definition to Rust we would end up with:

.. sourcecode:: rust

    ( 'y', "noninteractive".into(), "do not prompt...".into(), Value::Bool(false) )
    
If there is no ``short_name`` for a flag then pass an empty character literal
e.g. ``' '`` in the ``short_name`` place.

Command Definitions
-------------------

Command definitions are essentially metadata about all Mercurial commands.  The
reason that Rust must know about *all* commands and not simply Rust-only commands
is to be able to correctly prefix match on aliases and commands alike.  Commands
loaded from ``commands.names`` config option are marked as Python and only kept for their name.  Commands
actually defined in rust are of much more interest.  The definition has a basic
builder pattern to add flags, add documentation ( to interop with Mercurial's 
``help`` command ).

.. sourcecode:: rust
    
    let command = CommandDefinition::new(command_name)
        .add_flag(flag_definition)
        .with_doc(r#"documentation goes here"#);

Add as many flags are is necessary.  The flags that are parsed will be the superset
of the global flags applying to all commands plus the commands specific flags.

Real Example
~~~~~~~~~~~~

.. sourcecode:: rust

    let root_command = CommandDefinition::new("root")
        .add_flag((' ', "shared".into(), "show shared...".into(), Value::Bool(false)))
        .with_doc(r#"show root of the repo returns 0 on success."#);

Command Handlers
----------------

Command handlers are where actual command logic lives.  handlers have one of three
specific function signatures that imply what type of commands they are, which is
an implicit version of what Python does with ``inferrepo=True``.

Command Types
~~~~~~~~~~~~~

* ``Repo`` | ``(From<ParseOutput>, Vec<String>, &mut IO, Repo) -> Result<u8, DispatchError>``
* ``InferRepo`` | ``(From<ParseOutput>, Vec<String>, &mut IO, Option<Repo>) -> Result<u8, DispatchError>``
* ``NoRepo`` | ``(From<ParseOutput, Vec<String>, &mut IO) -> Result<u8, DispatchError>``

By changing your command handler, you are able to select where / when this command
would be made available to the user.  Some commands require that they are executed
from a Repo while some commands do not need a Repo at all.

Defining The Arguments
~~~~~~~~~~~~~~~~~~~~~~

There are a possible total of 4 arguments being passed into command handlers:

``From<ParseOutput>``: This argument is the most similar to the ``opts``
argument in the Python codebase, and is essentially a map of flag's ``long_name``
to Value variant.  The ``From`` trait is used to allow more flexibility in this
type, such as being able to have a custom struct converted from this ParseOutput.
This is a building block to approach an API similar to that of Structopts where
flags can be inferred without having to tediously write out builder patterns.

``Vec<String>``: This argument are the positional arguments to the command.
The order is preserved from the command line.

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

Registering A Command
~~~~~~~~~~~~~~~~~~~~~

First, create a CommandDefinition ( currently the pattern is to have a method for this ).
Next, create a command handler that pairs with this definition.  Create the dispatcher
and register the definition with the handler:

.. sourcecode:: rust

    let root_command: CommandDefinition = root_command();
    let mut dispatcher: Dispatcher = Dispatcher::new();
    dispatcher.register(root_command, root); // assume function named `root` is handler

Then ``dispatcher.dispatch(args)`` will handle all of the parsing, calling the
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
