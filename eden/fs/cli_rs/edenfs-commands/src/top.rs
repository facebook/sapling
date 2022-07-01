/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl top

use async_trait::async_trait;
use clap::Parser;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::RwLock;
use std::time::Duration;

use once_cell::sync::Lazy;
use termwiz::caps::Capabilities;
use termwiz::color::ColorAttribute;
use termwiz::input::InputEvent;
use termwiz::input::KeyCode;
use termwiz::input::KeyEvent;
use termwiz::surface::Change;
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::new_terminal;
use termwiz::terminal::Terminal;
use termwiz::widgets::layout::ChildOrientation;
use termwiz::widgets::layout::Constraints;
use termwiz::widgets::RenderArgs;
use termwiz::widgets::Ui;
use termwiz::widgets::UpdateArgs;
use termwiz::widgets::Widget;
use termwiz::widgets::WidgetEvent;
use termwiz::Error;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Monitor EdenFS accesses by process.")]
pub struct TopCmd {
    /// Don't accumulate data; refresh the screen every update
    /// cycle.
    #[clap(short, long)]
    ephemeral: bool,

    /// Specify the rate (in seconds) at which eden top updates.
    #[clap(short, long, default_value = "1")]
    refresh_rate: u64,
}

#[derive(PartialEq, Copy, Clone)]
enum Pages {
    HelpPage,
    MainPage,
}

// top has a few different "views" or "pages". This controls which page will be
// rendered in the next rendering cycle. Wigets that belong in each view should
// only declare non zero width and height if their parent page is the active page.
// widgets should only render themselves if they and their parent page declared
// a non zero width and height in the _last_ rendering cycle. This is because
// the size of a widget is determined by its declared size in the last rendering
// cycle, and it will cause errors to render anything to a widget of zero size.
// Disclaimer: this is a hack. Ideally termwiz would manage visable and hidden
// widgets.
static OBSERVED_ACTIVE_PAGE: Lazy<RwLock<Pages>> = Lazy::new(|| RwLock::new(Pages::MainPage));

struct EdenTopHeader {}

impl EdenTopHeader {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget for EdenTopHeader {
    fn render(&mut self, args: &mut RenderArgs) {
        args.surface
            .add_change(Change::ClearScreen(ColorAttribute::Default));
        args.surface.add_change("eden top");
    }

    fn get_size_constraints(&self) -> Constraints {
        let mut c = Constraints::default();
        c.set_fixed_height(1);
        c.set_pct_width(100);
        c.child_orientation = ChildOrientation::Horizontal;
        c
    }
}

// This indicates whether the help page delcared itself to have non zero width
// and height in the last cycle. The Help page is allowed to render itself if it
// declared itself to have non zero height.
// Disclaimer: this is a super hack. This could be a member variable of
// HelpPage, but we keep the state global to match the main page state,
// this will also allow any widgets nested in the help page to share state,
// should we add any of these.
static HELP_SHOULD_RENDER: AtomicBool = AtomicBool::new(false);

struct HelpPage;

impl HelpPage {
    pub fn new() -> Self {
        Self {}
    }

    fn set_should_render(&self, new_should_render: bool) {
        HELP_SHOULD_RENDER.store(new_should_render, Ordering::SeqCst)
    }

    fn get_should_render(&self) -> bool {
        HELP_SHOULD_RENDER.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        *OBSERVED_ACTIVE_PAGE.read().unwrap() == Pages::HelpPage
    }
}

impl Widget for HelpPage {
    fn render(&mut self, args: &mut RenderArgs) {
        if self.get_should_render() {
            args.surface
                .add_change(Change::ClearScreen(ColorAttribute::Default));
            args.surface.add_change("eden help");
        }
    }

    fn get_size_constraints(&self) -> Constraints {
        if self.is_active() {
            self.set_should_render(true);
            Constraints::default()
        } else {
            self.set_should_render(false);
            Constraints::with_fixed_width_height(0, 0)
        }
    }
}

// This indicates whether the main page delcared itself to have non zero width
// and height in the last cycle. The Main page and its nested widgets are
// allowed to render themselves if the main page widgets declared themselves to
// have non zero height. They all should either declare 0 size or non zero size.
// There should not be a mix.
// Disclaimer: this is a super hack. We keep this global state to allow
// cordinating between child widgets of the main page.
static MAIN_SHOULD_RENDER: AtomicBool = AtomicBool::new(false);
struct MainPage;

impl MainPage {
    pub fn new() -> Self {
        Self {}
    }

    fn set_should_render(&self, new_should_render: bool) {
        MAIN_SHOULD_RENDER.store(new_should_render, Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        *OBSERVED_ACTIVE_PAGE.read().unwrap() == Pages::MainPage
    }
}

impl Widget for MainPage {
    fn render(&mut self, _args: &mut RenderArgs) {}

    fn get_size_constraints(&self) -> Constraints {
        if self.is_active() {
            self.set_should_render(true);
            let mut c = Constraints {
                child_orientation: ChildOrientation::Vertical,
                ..Default::default()
            };
            c.set_pct_width(100);
            c.set_pct_height(100);
            c
        } else {
            self.set_should_render(false);
            Constraints::with_fixed_width_height(0, 0)
        }
    }
}

struct TopLevelStatsSection {}

impl TopLevelStatsSection {
    pub fn new() -> Self {
        Self {}
    }

    // the main wiget sets should render for this widget

    fn get_should_render(&self) -> bool {
        MAIN_SHOULD_RENDER.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        *OBSERVED_ACTIVE_PAGE.read().unwrap() == Pages::MainPage
    }
}

impl Widget for TopLevelStatsSection {
    fn render(&mut self, args: &mut RenderArgs) {
        if self.get_should_render() {
            args.surface
                .add_change(Change::ClearScreen(ColorAttribute::Default));
            args.surface.add_change("top level stats");
        }
    }

    fn get_size_constraints(&self) -> Constraints {
        if self.is_active() {
            let mut c = Constraints {
                child_orientation: ChildOrientation::Vertical,
                ..Default::default()
            };
            c.set_fixed_height(4);
            c.set_pct_width(100);
            c
        } else {
            Constraints::with_fixed_width_height(0, 0)
        }
    }
}

struct ProcessTableSection {}

impl ProcessTableSection {
    pub fn new() -> Self {
        Self {}
    }

    // the main wiget sets should render for this widget
    fn get_should_render(&self) -> bool {
        MAIN_SHOULD_RENDER.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        *OBSERVED_ACTIVE_PAGE.read().unwrap() == Pages::MainPage
    }
}

impl Widget for ProcessTableSection {
    fn render(&mut self, args: &mut RenderArgs) {
        if self.get_should_render() {
            args.surface
                .add_change(Change::ClearScreen(ColorAttribute::Default));
            args.surface.add_change("process table stats");
        }
    }

    fn get_size_constraints(&self) -> Constraints {
        if self.is_active() {
            let mut c = Constraints {
                child_orientation: ChildOrientation::Vertical,
                ..Default::default()
            };
            c.set_pct_width(100);
            c
        } else {
            Constraints::with_fixed_width_height(0, 0)
        }
    }
}

struct BasePage {
    height: usize,
    width: usize,
}

impl BasePage {
    pub fn new(inital_height: usize, initial_width: usize) -> Self {
        Self {
            height: inital_height,
            width: initial_width,
        }
    }
}

impl Widget for BasePage {
    fn process_event(&mut self, event: &WidgetEvent, _args: &mut UpdateArgs) -> bool {
        match event {
            WidgetEvent::Input(InputEvent::Resized { rows, cols }) => {
                self.height = rows.clone();
                self.width = cols.clone();
            }
            _ => {}
        };
        true
    }

    fn render(&mut self, _args: &mut RenderArgs) {}

    fn get_size_constraints(&self) -> Constraints {
        let mut c = Constraints {
            child_orientation: ChildOrientation::Vertical,
            ..Default::default()
        };
        // we should be doing this, but there is a bug that the width turns
        // this into 1 column so we use a fized width to avoid this for now
        // c.set_pct_width(100);
        // c.set_pct_height(100);
        c.set_fixed_width(self.width as u16);
        c.set_fixed_height(self.height as u16);
        c
    }
}

impl TopCmd {
    fn main_loop(&self) -> Result<(), Error> {
        let caps = Capabilities::new_from_env()?;
        let mut terminal = new_terminal(caps)?;
        // getting the screen size here is a bit of a hack, we should not actually read
        // this value, but do so to get around a bug with percentage based widget size
        let screen_size = terminal.get_screen_size()?;
        let mut buf = BufferedTerminal::new(terminal)?;
        buf.terminal().set_raw_mode()?;

        let mut ui = Ui::new();
        let base_id = ui.set_root(BasePage::new(screen_size.rows, screen_size.cols));
        ui.add_child(base_id, EdenTopHeader::new());
        let main_id = ui.add_child(base_id, MainPage::new());
        let _help_id = ui.add_child(base_id, HelpPage::new());
        ui.add_child(main_id, TopLevelStatsSection::new());
        ui.add_child(main_id, ProcessTableSection::new());

        // allow resize events to go to the base, so that the high level size gets updated
        // with resizing
        ui.set_focus(base_id);

        loop {
            // let the widgets process events
            ui.process_event_queue()?;

            // After updating and processing all of the widgets, compose them
            // and render them to the screen.
            if ui.render_to_screen(&mut buf)? {
                // We have more events to process immediately; don't block waiting
                // for input below, but jump to the top of the loop to re-run the
                // updates.
                continue;
            }
            // Compute an optimized delta to apply to the terminal and display it
            buf.flush()?;

            match buf
                .terminal()
                .poll_input(Some(Duration::from_secs(self.refresh_rate)))
            {
                Ok(Some(InputEvent::Resized { rows, cols })) => {
                    // TODO(kmancini): this is working around a bug where we
                    // don't realize that we should redraw everything on resize
                    // in BufferedTerminal.
                    buf.add_change(Change::ClearScreen(Default::default()));
                    buf.resize(cols, rows);
                    ui.queue_event(WidgetEvent::Input(InputEvent::Resized { rows, cols }));
                }
                Ok(Some(input)) => match input {
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('q'),
                        ..
                    }) => {
                        break;
                    }
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('h'),
                        ..
                    }) => {
                        *OBSERVED_ACTIVE_PAGE.write().unwrap() = Pages::HelpPage;
                    }
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Escape,
                        ..
                    }) => {
                        *OBSERVED_ACTIVE_PAGE.write().unwrap() = Pages::MainPage;
                    }
                    input => {
                        ui.queue_event(WidgetEvent::Input(input));
                    }
                },
                Ok(None) => {}
                Err(e) => {
                    print!("{:?}\r\n", e);
                    break;
                }
            }
        }
        // clear the sceen before we exit because the terminal
        // prompt will write over the eden top stuff and then
        // the next command output and eden top output gets
        // jumbled together.
        buf.add_change(Change::ClearScreen(Default::default()));
        buf.flush()?;
        Ok(())
    }
}

#[async_trait]
impl crate::Subcommand for TopCmd {
    async fn run(&self, _instance: EdenFsInstance) -> Result<ExitCode> {
        match self.main_loop() {
            Ok(_) => Ok(0),
            Err(cause) => {
                println!("Error: {}", cause);
                Ok(1)
            }
        }
    }
}
