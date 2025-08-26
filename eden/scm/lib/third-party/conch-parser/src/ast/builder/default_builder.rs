use crate::ast::builder::*;
use crate::ast::*;
use std::default::Default;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use void::Void;

/// A macro for defining a default builder, its boilerplate, and delegating
/// the `Builder` trait to its respective `CoreBuilder` type.
///
/// This allows us to create concrete atomic/non-atomic builders which
/// wrap a concrete `CoreBuilder` implementation so we can hide its type
/// complexity from the consumer.
///
/// We could accomplish this by using a public type alias to the private
/// builder type, however, rustdoc will only generate docs for the alias
/// definition is, and the docs for the inner builder will be rendered in
/// their entire complexity.
// FIXME: might be good to revisit this complexity/indirection
macro_rules! default_builder {
    ($(#[$attr:meta])*
     pub struct $Builder:ident,
     $CoreBuilder:ident,
     $Word:ident,
     $Cmd:ident,
     $PipeableCmd:ident,
    ) => {
        $(#[$attr])*
        pub struct $Builder<T>($CoreBuilder<T, $Word<T>, $Cmd<T>>);

        impl<T> $Builder<T> {
            /// Constructs a builder.
            pub fn new() -> Self {
                $Builder($CoreBuilder::new())
            }
        }

        impl<T> fmt::Debug for $Builder<T> {
            fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt.debug_struct(stringify!($Builder))
                    .finish()
            }
        }

        impl<T> Default for $Builder<T> {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<T> Clone for $Builder<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $Builder<T> {}

        impl<T: From<String>> Builder for $Builder<T> {
            type Command         = $Cmd<T>;
            type CommandList     = AndOrList<Self::ListableCommand>;
            type ListableCommand = ListableCommand<Self::PipeableCommand>;
            type PipeableCommand = $PipeableCmd<T, Self::Word, Self::Command>;
            type CompoundCommand = ShellCompoundCommand<T, Self::Word, Self::Command>;
            type Word            = $Word<T>;
            type Redirect        = Redirect<Self::Word>;
            type Error           = Void;

            fn complete_command(&mut self,
                                pre_cmd_comments: Vec<Newline>,
                                list: Self::CommandList,
                                separator: SeparatorKind,
                                cmd_comment: Option<Newline>)
                -> Result<Self::Command, Self::Error>
            {
                self.0.complete_command(pre_cmd_comments, list, separator, cmd_comment)
            }

            fn and_or_list(&mut self,
                      first: Self::ListableCommand,
                      rest: Vec<(Vec<Newline>, AndOr<Self::ListableCommand>)>)
                -> Result<Self::CommandList, Self::Error>
            {
                self.0.and_or_list(first, rest)
            }

            fn pipeline(&mut self,
                        bang: bool,
                        cmds: Vec<(Vec<Newline>, Self::PipeableCommand)>)
                -> Result<Self::ListableCommand, Self::Error>
            {
                self.0.pipeline(bang, cmds)
            }

            fn simple_command(
                &mut self,
                redirects_or_env_vars: Vec<RedirectOrEnvVar<Self::Redirect, String, Self::Word>>,
                redirects_or_cmd_words: Vec<RedirectOrCmdWord<Self::Redirect, Self::Word>>
            ) -> Result<Self::PipeableCommand, Self::Error>
            {
                self.0.simple_command(redirects_or_env_vars, redirects_or_cmd_words)
            }

            fn brace_group(&mut self,
                           cmds: CommandGroup<Self::Command>,
                           redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.brace_group(cmds, redirects)
            }

            fn subshell(&mut self,
                        cmds: CommandGroup<Self::Command>,
                        redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.subshell(cmds, redirects)
            }

            fn loop_command(&mut self,
                            kind: LoopKind,
                            guard_body_pair: GuardBodyPairGroup<Self::Command>,
                            redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.loop_command(kind, guard_body_pair, redirects)
            }

            fn if_command(&mut self,
                          fragments: IfFragments<Self::Command>,
                          redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.if_command(fragments, redirects)
            }

            fn for_command(&mut self,
                           fragments: ForFragments<Self::Word, Self::Command>,
                           redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.for_command(fragments, redirects)
            }

            fn case_command(&mut self,
                            fragments: CaseFragments<Self::Word, Self::Command>,
                            redirects: Vec<Self::Redirect>)
                -> Result<Self::CompoundCommand, Self::Error>
            {
                self.0.case_command(fragments, redirects)
            }

            fn compound_command_into_pipeable(&mut self,
                                              cmd: Self::CompoundCommand)
                -> Result<Self::PipeableCommand, Self::Error>
            {
                self.0.compound_command_into_pipeable(cmd)
            }

            fn function_declaration(&mut self,
                                    name: String,
                                    post_name_comments: Vec<Newline>,
                                    body: Self::CompoundCommand)
                -> Result<Self::PipeableCommand, Self::Error>
            {
                self.0.function_declaration(name, post_name_comments, body)
            }

            fn comments(&mut self,
                        comments: Vec<Newline>)
                -> Result<(), Self::Error>
            {
                self.0.comments(comments)
            }

            fn word(&mut self,
                    kind: ComplexWordKind<Self::Command>)
                -> Result<Self::Word, Self::Error>
            {
                self.0.word(kind)
            }

            fn redirect(&mut self,
                        kind: RedirectKind<Self::Word>)
                -> Result<Self::Redirect, Self::Error>
            {
                self.0.redirect(kind)
            }
        }
    };
}

type RcCoreBuilder<T, W, C> = CoreBuilder<T, W, C, Rc<ShellCompoundCommand<T, W, C>>>;
type ArcCoreBuilder<T, W, C> = CoreBuilder<T, W, C, Arc<ShellCompoundCommand<T, W, C>>>;

default_builder! {
    /// A `Builder` implementation which builds shell commands
    /// using the (non-atomic) AST definitions in the `ast` module.
    pub struct DefaultBuilder,
    RcCoreBuilder,
    TopLevelWord,
    TopLevelCommand,
    ShellPipeableCommand,
}

default_builder! {
    /// A `Builder` implementation which builds shell commands
    /// using the (atomic) AST definitions in the `ast` module.
    pub struct AtomicDefaultBuilder,
    ArcCoreBuilder,
    AtomicTopLevelWord,
    AtomicTopLevelCommand,
    AtomicShellPipeableCommand,
}

/// A `DefaultBuilder` implementation which uses regular `String`s when
/// representing shell words.
pub type StringBuilder = DefaultBuilder<String>;

/// A `DefaultBuilder` implementation which uses `Rc<String>`s when
/// representing shell words.
pub type RcBuilder = DefaultBuilder<Rc<String>>;

/// A `DefaultBuilder` implementation which uses `Arc<String>`s when
/// representing shell words.
pub type ArcBuilder = AtomicDefaultBuilder<Arc<String>>;

/// The actual provided `Builder` implementation.
/// The various type parameters are used to swap out atomic/non-atomic AST versions.
pub struct CoreBuilder<T, W, C, F> {
    phantom_data: PhantomData<(T, W, C, F)>,
}

impl<T, W, C, F> fmt::Debug for CoreBuilder<T, W, C, F> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("CoreBuilder").finish()
    }
}

impl<T, W, C, F> Clone for CoreBuilder<T, W, C, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, W, C, F> Copy for CoreBuilder<T, W, C, F> {}

impl<T, W, C, F> Default for CoreBuilder<T, W, C, F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, W, C, F> CoreBuilder<T, W, C, F> {
    /// Constructs a builder.
    pub fn new() -> Self {
        CoreBuilder {
            phantom_data: PhantomData,
        }
    }
}

type BuilderPipeableCommand<T, W, C, F> = PipeableCommand<
    T,
    Box<SimpleCommand<T, W, Redirect<W>>>,
    Box<ShellCompoundCommand<T, W, C>>,
    F,
>;

impl<T, W, C, F> Builder for CoreBuilder<T, W, C, F>
where
    T: From<String>,
    W: From<ShellWord<T, W, C>>,
    C: From<Command<AndOrList<ListableCommand<BuilderPipeableCommand<T, W, C, F>>>>>,
    F: From<ShellCompoundCommand<T, W, C>>,
{
    type Command = C;
    type CommandList = AndOrList<Self::ListableCommand>;
    type ListableCommand = ListableCommand<Self::PipeableCommand>;
    type PipeableCommand = BuilderPipeableCommand<T, W, C, F>;
    type CompoundCommand = ShellCompoundCommand<T, Self::Word, Self::Command>;
    type Word = W;
    type Redirect = Redirect<Self::Word>;
    type Error = Void;

    /// Constructs a `Command::Job` node with the provided inputs if the command
    /// was delimited by an ampersand or the command itself otherwise.
    fn complete_command(
        &mut self,
        _pre_cmd_comments: Vec<Newline>,
        list: Self::CommandList,
        separator: SeparatorKind,
        _cmd_comment: Option<Newline>,
    ) -> Result<Self::Command, Self::Error> {
        let cmd = match separator {
            SeparatorKind::Semi | SeparatorKind::Other | SeparatorKind::Newline => {
                Command::List(list)
            }
            SeparatorKind::Amp => Command::Job(list),
        };

        Ok(cmd.into())
    }

    /// Constructs a `Command::List` node with the provided inputs.
    fn and_or_list(
        &mut self,
        first: Self::ListableCommand,
        rest: Vec<(Vec<Newline>, AndOr<Self::ListableCommand>)>,
    ) -> Result<Self::CommandList, Self::Error> {
        Ok(AndOrList {
            first,
            rest: rest.into_iter().map(|(_, c)| c).collect(),
        })
    }

    /// Constructs a `Command::Pipe` node with the provided inputs or a `Command::Simple`
    /// node if only a single command with no status inversion is supplied.
    fn pipeline(
        &mut self,
        bang: bool,
        cmds: Vec<(Vec<Newline>, Self::PipeableCommand)>,
    ) -> Result<Self::ListableCommand, Self::Error> {
        debug_assert!(!cmds.is_empty());
        let mut cmds: Vec<_> = cmds.into_iter().map(|(_, c)| c).collect();

        // Pipe is the only AST node which allows for a status
        // negation, so we are forced to use it even if we have a single
        // command. Otherwise there is no need to wrap it further.
        if bang || cmds.len() > 1 {
            cmds.shrink_to_fit();
            Ok(ListableCommand::Pipe(bang, cmds))
        } else {
            Ok(ListableCommand::Single(cmds.pop().unwrap()))
        }
    }

    /// Constructs a `Command::Simple` node with the provided inputs.
    fn simple_command(
        &mut self,
        redirects_or_env_vars: Vec<RedirectOrEnvVar<Self::Redirect, String, Self::Word>>,
        mut redirects_or_cmd_words: Vec<RedirectOrCmdWord<Self::Redirect, Self::Word>>,
    ) -> Result<Self::PipeableCommand, Self::Error> {
        let redirects_or_env_vars = redirects_or_env_vars
            .into_iter()
            .map(|roev| match roev {
                RedirectOrEnvVar::Redirect(red) => RedirectOrEnvVar::Redirect(red),
                RedirectOrEnvVar::EnvVar(k, v) => RedirectOrEnvVar::EnvVar(k.into(), v),
            })
            .collect();

        redirects_or_cmd_words.shrink_to_fit();

        Ok(PipeableCommand::Simple(Box::new(SimpleCommand {
            redirects_or_env_vars,
            redirects_or_cmd_words,
        })))
    }

    /// Constructs a `CompoundCommand::Brace` node with the provided inputs.
    fn brace_group(
        &mut self,
        cmd_group: CommandGroup<Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let mut cmds = cmd_group.commands;
        cmds.shrink_to_fit();
        redirects.shrink_to_fit();
        Ok(CompoundCommand {
            kind: CompoundCommandKind::Brace(cmds),
            io: redirects,
        })
    }

    /// Constructs a `CompoundCommand::Subshell` node with the provided inputs.
    fn subshell(
        &mut self,
        cmd_group: CommandGroup<Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let mut cmds = cmd_group.commands;
        cmds.shrink_to_fit();
        redirects.shrink_to_fit();
        Ok(CompoundCommand {
            kind: CompoundCommandKind::Subshell(cmds),
            io: redirects,
        })
    }

    /// Constructs a `CompoundCommand::Loop` node with the provided inputs.
    fn loop_command(
        &mut self,
        kind: LoopKind,
        guard_body_pair: GuardBodyPairGroup<Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let mut guard = guard_body_pair.guard.commands;
        let mut body = guard_body_pair.body.commands;

        guard.shrink_to_fit();
        body.shrink_to_fit();
        redirects.shrink_to_fit();

        let guard_body_pair = GuardBodyPair { guard, body };

        let loop_cmd = match kind {
            LoopKind::While => CompoundCommandKind::While(guard_body_pair),
            LoopKind::Until => CompoundCommandKind::Until(guard_body_pair),
        };

        Ok(CompoundCommand {
            kind: loop_cmd,
            io: redirects,
        })
    }

    /// Constructs a `CompoundCommand::If` node with the provided inputs.
    fn if_command(
        &mut self,
        fragments: IfFragments<Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let IfFragments {
            conditionals,
            else_branch,
        } = fragments;

        let conditionals = conditionals
            .into_iter()
            .map(|gbp| {
                let mut guard = gbp.guard.commands;
                let mut body = gbp.body.commands;

                guard.shrink_to_fit();
                body.shrink_to_fit();

                GuardBodyPair { guard, body }
            })
            .collect();

        let else_branch = else_branch.map(
            |CommandGroup {
                 commands: mut els, ..
             }| {
                els.shrink_to_fit();
                els
            },
        );

        redirects.shrink_to_fit();

        Ok(CompoundCommand {
            kind: CompoundCommandKind::If {
                conditionals,
                else_branch,
            },
            io: redirects,
        })
    }

    /// Constructs a `CompoundCommand::For` node with the provided inputs.
    fn for_command(
        &mut self,
        fragments: ForFragments<Self::Word, Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let words = fragments.words.map(|(_, mut words, _)| {
            words.shrink_to_fit();
            words
        });

        let mut body = fragments.body.commands;
        body.shrink_to_fit();
        redirects.shrink_to_fit();

        Ok(CompoundCommand {
            kind: CompoundCommandKind::For {
                var: fragments.var.into(),
                words,
                body,
            },
            io: redirects,
        })
    }

    /// Constructs a `CompoundCommand::Case` node with the provided inputs.
    fn case_command(
        &mut self,
        fragments: CaseFragments<Self::Word, Self::Command>,
        mut redirects: Vec<Self::Redirect>,
    ) -> Result<Self::CompoundCommand, Self::Error> {
        let arms = fragments
            .arms
            .into_iter()
            .map(|arm| {
                let mut patterns = arm.patterns.pattern_alternatives;
                patterns.shrink_to_fit();

                let mut body = arm.body.commands;
                body.shrink_to_fit();

                PatternBodyPair { patterns, body }
            })
            .collect();

        redirects.shrink_to_fit();
        Ok(CompoundCommand {
            kind: CompoundCommandKind::Case {
                word: fragments.word,
                arms,
            },
            io: redirects,
        })
    }

    /// Converts a `CompoundCommand` into a `PipeableCommand`.
    fn compound_command_into_pipeable(
        &mut self,
        cmd: Self::CompoundCommand,
    ) -> Result<Self::PipeableCommand, Self::Error> {
        Ok(PipeableCommand::Compound(Box::new(cmd)))
    }

    /// Constructs a `Command::FunctionDef` node with the provided inputs.
    fn function_declaration(
        &mut self,
        name: String,
        _post_name_comments: Vec<Newline>,
        body: Self::CompoundCommand,
    ) -> Result<Self::PipeableCommand, Self::Error> {
        Ok(PipeableCommand::FunctionDef(name.into(), body.into()))
    }

    /// Ignored by the builder.
    fn comments(&mut self, _comments: Vec<Newline>) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Constructs a `ast::Word` from the provided input.
    fn word(&mut self, kind: ComplexWordKind<Self::Command>) -> Result<Self::Word, Self::Error> {
        macro_rules! map {
            ($pat:expr) => {
                match $pat {
                    Some(w) => Some(self.word(w)?),
                    None => None,
                }
            };
        }

        fn map_arith<T: From<String>>(kind: DefaultArithmetic) -> Arithmetic<T> {
            use crate::ast::Arithmetic::*;
            match kind {
                Var(v) => Var(v.into()),
                Literal(l) => Literal(l),
                Pow(a, b) => Pow(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                PostIncr(p) => PostIncr(p.into()),
                PostDecr(p) => PostDecr(p.into()),
                PreIncr(p) => PreIncr(p.into()),
                PreDecr(p) => PreDecr(p.into()),
                UnaryPlus(a) => UnaryPlus(Box::new(map_arith(*a))),
                UnaryMinus(a) => UnaryMinus(Box::new(map_arith(*a))),
                LogicalNot(a) => LogicalNot(Box::new(map_arith(*a))),
                BitwiseNot(a) => BitwiseNot(Box::new(map_arith(*a))),
                Mult(a, b) => Mult(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Div(a, b) => Div(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Modulo(a, b) => Modulo(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Add(a, b) => Add(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Sub(a, b) => Sub(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                ShiftLeft(a, b) => ShiftLeft(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                ShiftRight(a, b) => ShiftRight(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Less(a, b) => Less(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                LessEq(a, b) => LessEq(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Great(a, b) => Great(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                GreatEq(a, b) => GreatEq(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Eq(a, b) => Eq(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                NotEq(a, b) => NotEq(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                BitwiseAnd(a, b) => BitwiseAnd(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                BitwiseXor(a, b) => BitwiseXor(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                BitwiseOr(a, b) => BitwiseOr(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                LogicalAnd(a, b) => LogicalAnd(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                LogicalOr(a, b) => LogicalOr(Box::new(map_arith(*a)), Box::new(map_arith(*b))),
                Ternary(a, b, c) => Ternary(
                    Box::new(map_arith(*a)),
                    Box::new(map_arith(*b)),
                    Box::new(map_arith(*c)),
                ),
                Assign(v, a) => Assign(v.into(), Box::new(map_arith(*a))),
                Sequence(ariths) => Sequence(ariths.into_iter().map(map_arith).collect()),
            }
        }

        let map_param = |kind: DefaultParameter| -> Parameter<T> {
            use crate::ast::Parameter::*;
            match kind {
                At => At,
                Star => Star,
                Pound => Pound,
                Question => Question,
                Dash => Dash,
                Dollar => Dollar,
                Bang => Bang,
                Positional(p) => Positional(p),
                Var(v) => Var(v.into()),
            }
        };

        let mut map_simple = |kind| {
            use crate::ast::builder::ParameterSubstitutionKind::*;

            let simple = match kind {
                SimpleWordKind::Literal(s) => SimpleWord::Literal(s.into()),
                SimpleWordKind::Escaped(s) => SimpleWord::Escaped(s.into()),
                SimpleWordKind::Param(p) => SimpleWord::Param(map_param(p)),
                SimpleWordKind::Star => SimpleWord::Star,
                SimpleWordKind::Question => SimpleWord::Question,
                SimpleWordKind::SquareOpen => SimpleWord::SquareOpen,
                SimpleWordKind::SquareClose => SimpleWord::SquareClose,
                SimpleWordKind::Tilde => SimpleWord::Tilde,
                SimpleWordKind::Colon => SimpleWord::Colon,

                SimpleWordKind::CommandSubst(c) => {
                    SimpleWord::Subst(Box::new(ParameterSubstitution::Command(c.commands)))
                }

                SimpleWordKind::Subst(s) => {
                    // Force a move out of the boxed substitution. For some reason doing
                    // the deref in the match statement gives a strange borrow failure
                    let s = *s;
                    let subst = match s {
                        Len(p) => ParameterSubstitution::Len(map_param(p)),
                        Command(c) => ParameterSubstitution::Command(c.commands),
                        Arith(a) => ParameterSubstitution::Arith(a.map(map_arith)),
                        Default(c, p, w) => {
                            ParameterSubstitution::Default(c, map_param(p), map!(w))
                        }
                        Assign(c, p, w) => ParameterSubstitution::Assign(c, map_param(p), map!(w)),
                        Error(c, p, w) => ParameterSubstitution::Error(c, map_param(p), map!(w)),
                        Alternative(c, p, w) => {
                            ParameterSubstitution::Alternative(c, map_param(p), map!(w))
                        }
                        RemoveSmallestSuffix(p, w) => {
                            ParameterSubstitution::RemoveSmallestSuffix(map_param(p), map!(w))
                        }
                        RemoveLargestSuffix(p, w) => {
                            ParameterSubstitution::RemoveLargestSuffix(map_param(p), map!(w))
                        }
                        RemoveSmallestPrefix(p, w) => {
                            ParameterSubstitution::RemoveSmallestPrefix(map_param(p), map!(w))
                        }
                        RemoveLargestPrefix(p, w) => {
                            ParameterSubstitution::RemoveLargestPrefix(map_param(p), map!(w))
                        }
                    };
                    SimpleWord::Subst(Box::new(subst))
                }
            };
            Ok(simple)
        };

        let mut map_word = |kind| {
            let word = match kind {
                WordKind::Simple(s) => Word::Simple(map_simple(s)?),
                WordKind::SingleQuoted(s) => Word::SingleQuoted(s.into()),
                WordKind::DoubleQuoted(v) => Word::DoubleQuoted(
                    v.into_iter()
                        .map(&mut map_simple)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            };
            Ok(word)
        };

        let word = match compress(kind) {
            ComplexWordKind::Single(s) => ComplexWord::Single(map_word(s)?),
            ComplexWordKind::Concat(words) => ComplexWord::Concat(
                words
                    .into_iter()
                    .map(map_word)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };

        Ok(word.into())
    }

    /// Constructs a `ast::Redirect` from the provided input.
    fn redirect(&mut self, kind: RedirectKind<Self::Word>) -> Result<Self::Redirect, Self::Error> {
        let io = match kind {
            RedirectKind::Read(fd, path) => Redirect::Read(fd, path),
            RedirectKind::Write(fd, path) => Redirect::Write(fd, path),
            RedirectKind::ReadWrite(fd, path) => Redirect::ReadWrite(fd, path),
            RedirectKind::Append(fd, path) => Redirect::Append(fd, path),
            RedirectKind::Clobber(fd, path) => Redirect::Clobber(fd, path),
            RedirectKind::Heredoc(fd, body) => Redirect::Heredoc(fd, body),
            RedirectKind::DupRead(src, dst) => Redirect::DupRead(src, dst),
            RedirectKind::DupWrite(src, dst) => Redirect::DupWrite(src, dst),
        };

        Ok(io)
    }
}

#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
struct Coalesce<I: Iterator, F> {
    iter: I,
    cur: Option<I::Item>,
    func: F,
}

impl<I: Iterator, F> Coalesce<I, F> {
    fn new<T>(iter: T, func: F) -> Self
    where
        T: IntoIterator<IntoIter = I, Item = I::Item>,
    {
        Coalesce {
            iter: iter.into_iter(),
            cur: None,
            func,
        }
    }
}

type CoalesceResult<T> = Result<T, (T, T)>;
impl<I, F> Iterator for Coalesce<I, F>
where
    I: Iterator,
    F: FnMut(I::Item, I::Item) -> CoalesceResult<I::Item>,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.cur.take().or_else(|| self.iter.next());
        let (mut left, mut right) = match (cur, self.iter.next()) {
            (Some(l), Some(r)) => (l, r),
            (Some(l), None) | (None, Some(l)) => return Some(l),
            (None, None) => return None,
        };

        loop {
            match (self.func)(left, right) {
                Ok(combined) => match self.iter.next() {
                    Some(next) => {
                        left = combined;
                        right = next;
                    }
                    None => return Some(combined),
                },

                Err((left, right)) => {
                    debug_assert!(self.cur.is_none());
                    self.cur = Some(right);
                    return Some(left);
                }
            }
        }
    }
}

fn compress<C>(word: ComplexWordKind<C>) -> ComplexWordKind<C> {
    use crate::ast::builder::ComplexWordKind::*;
    use crate::ast::builder::SimpleWordKind::*;
    use crate::ast::builder::WordKind::*;

    fn coalesce_simple<C>(
        a: SimpleWordKind<C>,
        b: SimpleWordKind<C>,
    ) -> CoalesceResult<SimpleWordKind<C>> {
        match (a, b) {
            (Literal(mut a), Literal(b)) => {
                a.push_str(&b);
                Ok(Literal(a))
            }
            (a, b) => Err((a, b)),
        }
    }

    fn coalesce_word<C>(a: WordKind<C>, b: WordKind<C>) -> CoalesceResult<WordKind<C>> {
        match (a, b) {
            (Simple(a), Simple(b)) => coalesce_simple(a, b)
                .map(Simple)
                .map_err(|(a, b)| (Simple(a), Simple(b))),
            (SingleQuoted(mut a), SingleQuoted(b)) => {
                a.push_str(&b);
                Ok(SingleQuoted(a))
            }
            (DoubleQuoted(a), DoubleQuoted(b)) => {
                let quoted = Coalesce::new(a.into_iter().chain(b), coalesce_simple).collect();
                Ok(DoubleQuoted(quoted))
            }
            (a, b) => Err((a, b)),
        }
    }

    match word {
        Single(s) => Single(match s {
            s @ Simple(_) | s @ SingleQuoted(_) => s,
            DoubleQuoted(v) => DoubleQuoted(Coalesce::new(v, coalesce_simple).collect()),
        }),
        Concat(v) => {
            let mut body: Vec<_> = Coalesce::new(v, coalesce_word).collect();
            if body.len() == 1 {
                Single(body.pop().unwrap())
            } else {
                Concat(body)
            }
        }
    }
}
