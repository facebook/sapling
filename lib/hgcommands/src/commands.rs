// Copyright Facebook, Inc. 2019

use clidispatch::command::CommandDefinition;
use clidispatch::dispatch::*;
use clidispatch::errors::DispatchError;
use clidispatch::io::IO;
use clidispatch::repo::Repo;
use cliparser::parser::ParseOutput;

#[allow(dead_code)]
pub fn create_dispatcher() -> Dispatcher {
    let mut dispatcher = Dispatcher::new();
    let root_command = root_command();
    dispatcher.register(root_command, root);

    dispatcher
}

#[allow(dead_code)]
pub fn dispatch(dispatcher: &mut Dispatcher) -> Result<u8, DispatchError> {
    let args = args()?;

    dispatcher.dispatch(args)
}

fn root_command() -> CommandDefinition {
    let command = CommandDefinition::new("root")
        .add_flag((' ', "shared", "show root of the shared repo", false))
        .with_doc(
            r#"print the root (top) of the current working directory
        
    Print the root directory of the current repository.
        
    Returns 0 on success.
        
        "#,
        );

    command
}

pub struct RootCommand {
    shared: bool,
}

impl From<ParseOutput> for RootCommand {
    fn from(opts: ParseOutput) -> Self {
        let shared: bool = opts.pick("shared");

        RootCommand { shared }
    }
}

pub fn root(
    cmd: RootCommand,
    args: Vec<String>,
    io: &mut IO,
    repo: Repo,
) -> Result<u8, DispatchError> {
    if args.len() > 0 {
        return Err(DispatchError::InvalidArguments {
            command_name: "root".to_string(),
        }); // root doesn't support arguments
    }

    let shared = repo.sharedpath()?;

    let path = if cmd.shared {
        shared.unwrap_or(repo.path().to_owned())
    } else {
        repo.path().to_owned()
    };

    io.write(format!("{}\n", path.canonicalize()?.to_string_lossy()))?;
    Ok(0)
}
