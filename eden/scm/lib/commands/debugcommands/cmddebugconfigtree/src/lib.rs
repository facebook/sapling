/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use cmdutil::define_flags;
use cmdutil::Config;
use cmdutil::Repo;
use cmdutil::ReqCtx;
use cmdutil::Result;

define_flags! {
    pub struct DebugConfigTreeOpts {
        /// show non-editable internal configs
        #[short('i')]
        internal: bool,

        /// show config source
        #[short('s')]
        source: bool,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugConfigTreeOpts>, repo: Option<&mut Repo>) -> Result<u8> {
    // repo is used to load repo-specific config.
    let config = match repo {
        None => ctx.config(),
        Some(repo) => repo.config(),
    };

    ctx.maybe_start_pager(config)?;

    let mut state = State::new(&ctx);
    state.print_config_layer(&config)?;

    Ok(0)
}

struct State<'a> {
    ctx: &'a ReqCtx<DebugConfigTreeOpts>,
    /// Print every tiem in those sections.
    full_sections: HashSet<&'a str>,
    /// Print some items in those sections.
    /// If empty, print everything.
    maybe_sections: HashSet<&'a str>,
    /// Print configs that match the section and name.
    section_names: HashSet<(&'a str, &'a str)>,
    /// Print "source" information.
    show_source: bool,
    /// Print builtin configs.
    show_internal: bool,
    /// (Mutable) indentation of the tree output.
    indent: String,
}

impl<'a> State<'a> {
    fn new(ctx: &'a ReqCtx<DebugConfigTreeOpts>) -> Self {
        let show_source = ctx.opts.source || ctx.global_opts().debug;
        let show_internal = ctx.opts.internal || ctx.global_opts().verbose;
        let args = &ctx.opts.args;
        let mut full_sections = HashSet::new();
        let mut maybe_sections = HashSet::new();
        let mut section_names = HashSet::new();
        for arg in args {
            match arg.split_once('.') {
                None => {
                    let section = arg.as_str();
                    full_sections.insert(section);
                    maybe_sections.insert(section);
                }
                Some((section, name)) => {
                    section_names.insert((section, name));
                    maybe_sections.insert(section);
                }
            }
        }
        Self {
            ctx,
            full_sections,
            maybe_sections,
            section_names,
            show_source,
            show_internal,
            indent: String::new(),
        }
    }

    fn write_line(&self, line: &str) -> Result<()> {
        let io = self.ctx.io();
        io.write(&self.indent)?;
        io.write(line)?;
        io.write("\n")?;
        Ok(())
    }

    fn should_print_everything(&self) -> bool {
        self.maybe_sections.is_empty()
    }

    fn should_print(&self, section: &str, name: &str) -> bool {
        self.should_print_everything()
            || self.full_sections.contains(&section)
            || self.section_names.contains(&(section, name))
    }

    fn should_print_section(&self, section: &str) -> bool {
        self.should_print_everything() || self.maybe_sections.contains(section)
    }

    fn push_indent(&mut self) {
        self.indent.push_str("  ");
    }

    fn pop_indent(&mut self) {
        self.indent.pop();
        self.indent.pop();
    }

    fn print_config_layer(&mut self, config: &dyn Config) -> Result<()> {
        self.write_line(&format!("<{}>", config.layer_name()))?;
        self.push_indent();
        let layers = config.layers();
        if layers.is_empty() {
            // Leaf node. Show configs.
            self.print_config_items(config)?;
        } else {
            // Recursively show sub-layers.
            for layer in layers {
                self.print_config_layer(&layer)?;
            }
        }
        self.pop_indent();
        Ok(())
    }

    fn print_config_items(&mut self, config: &dyn Config) -> Result<()> {
        let mut first_section = true;
        for section in config.sections().iter() {
            if !self.should_print_section(section) {
                continue;
            }
            let mut section_printed = false;
            for name in config.keys(section) {
                if !self.should_print(section, &name) {
                    continue;
                }
                let sources = config.get_sources(section, &name);
                for source in sources.iter() {
                    let source_name = source.source();
                    if (source_name.starts_with("builtin:") || *source_name == "dynamic")
                        && !self.show_internal
                    {
                        continue;
                    }
                    if !section_printed {
                        if first_section {
                            first_section = false;
                        } else {
                            self.write_line("")?;
                        }
                        self.write_line(&format!("[{}]", section))?;
                        section_printed = true;
                    }
                    let value_str = match source.value() {
                        None => "(unset)",
                        Some(v) => v.as_ref(),
                    };
                    let mut value_lines = value_str.lines();
                    let first_value_line = value_lines.next().unwrap_or("");
                    let source_str = if self.show_source {
                        let mut source_str = format!(" # {}", source.source());
                        if let (Some(line_no), Some((path, _))) =
                            (source.line_number(), source.location())
                        {
                            source_str.push_str(&format!(" at {}:{}", path.display(), line_no));
                        }
                        source_str
                    } else {
                        String::new()
                    };
                    self.write_line(&format!("{}={}{}", name, first_value_line, source_str))?;
                    self.push_indent();
                    for line in value_lines {
                        self.write_line(line)?;
                    }
                    self.pop_indent();
                }
            }
        }
        Ok(())
    }
}

pub fn aliases() -> &'static str {
    "debugconfigtree"
}

pub fn doc() -> &'static str {
    "show config hierarchical"
}

pub fn synopsis() -> Option<&'static str> {
    Some("[NAME]...")
}
