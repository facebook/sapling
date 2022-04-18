use crate::ast::builder::*;
use crate::ast::{AndOr, RedirectOrCmdWord, RedirectOrEnvVar};
use void::Void;

/// A no-op `Builder` which ignores all inputs and always returns `()`.
///
/// Useful for validation of correct programs (i.e. parsing input without
/// caring about the actual AST representations).
#[derive(Debug, Copy, Clone)]
pub struct EmptyBuilder;

impl Default for EmptyBuilder {
    fn default() -> Self {
        EmptyBuilder::new()
    }
}

impl EmptyBuilder {
    /// Constructs a builder.
    pub fn new() -> Self {
        EmptyBuilder
    }
}

impl Builder for EmptyBuilder {
    type Command = ();
    type CommandList = ();
    type ListableCommand = ();
    type PipeableCommand = ();
    type CompoundCommand = ();
    type Word = ();
    type Redirect = ();
    type Error = Void;

    fn complete_command(
        &mut self,
        _pre_cmd_comments: Vec<Newline>,
        _cmd: Self::Command,
        _separator: SeparatorKind,
        _cmd_comment: Option<Newline>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn and_or_list(
        &mut self,
        _first: Self::ListableCommand,
        _rest: Vec<(Vec<Newline>, AndOr<Self::ListableCommand>)>,
    ) -> Result<Self::CommandList, Self::Error> {
        Ok(())
    }

    fn pipeline(
        &mut self,
        _bang: bool,
        _cmds: Vec<(Vec<Newline>, Self::Command)>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn simple_command(
        &mut self,
        _redirects_or_env_vars: Vec<RedirectOrEnvVar<Self::Redirect, String, Self::Word>>,
        _redirects_or_cmd_words: Vec<RedirectOrCmdWord<Self::Redirect, Self::Word>>,
    ) -> Result<Self::PipeableCommand, Self::Error> {
        Ok(())
    }

    fn brace_group(
        &mut self,
        _cmds: CommandGroup<Self::Command>,
        _redirects: Vec<Self::Redirect>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn subshell(
        &mut self,
        _cmds: CommandGroup<Self::Command>,
        _redirects: Vec<Self::Redirect>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn loop_command(
        &mut self,
        __kind: LoopKind,
        __guard_body_pair: GuardBodyPairGroup<Self::Command>,
        __redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        Ok(())
    }

    fn if_command(
        &mut self,
        _fragments: IfFragments<Self::Command>,
        _redirects: Vec<Self::Redirect>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn for_command(
        &mut self,
        _fragments: ForFragments<Self::Word, Self::Command>,
        _redirects: Vec<Self::Redirect>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn case_command(
        &mut self,
        _fragments: CaseFragments<Self::Word, Self::Command>,
        _redirects: Vec<Self::Redirect>,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn function_declaration(
        &mut self,
        _name: String,
        _post_name_comments: Vec<Newline>,
        _body: Self::CompoundCommand,
    ) -> Result<Self::Command, Self::Error> {
        Ok(())
    }

    fn comments(&mut self, _comments: Vec<Newline>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn word(&mut self, _kind: ComplexWordKind<Self::Command>) -> Result<Self::Word, Self::Error> {
        Ok(())
    }

    fn redirect(&mut self, _kind: RedirectKind<Self::Word>) -> Result<Self::Redirect, Self::Error> {
        Ok(())
    }

    fn compound_command_into_pipeable(
        &mut self,
        _cmd: Self::CompoundCommand,
    ) -> Result<Self::PipeableCommand, Self::Error> {
        Ok(())
    }
}
