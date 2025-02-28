//! Actions.

use std::sync::{Arc, Mutex};

use crate::error::Error;
use crate::event::{Event, EventSender};

/// Actions that can be performed on the pager.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Action {
    /// Quit the pager.
    Quit,

    /// Refresh the screen.
    Refresh,

    /// Show the help screen.
    Help,

    /// Cancel the current action.
    Cancel,

    /// Switch to the previous file.
    PreviousFile,

    /// Switch to the next file.
    NextFile,

    /// Toggle visibility of the ruler.
    ToggleRuler,

    /// Scroll up *n* lines.
    ScrollUpLines(usize),

    /// Scroll down *n* lines.
    ScrollDownLines(usize),

    /// Scroll up 1/*n* of the screen height.
    ScrollUpScreenFraction(usize),

    /// Scroll down 1/*n* of the screen height.
    ScrollDownScreenFraction(usize),

    /// Scroll to the top of the file.
    ScrollToTop,

    /// Scroll to the bottom of the file, and start following it.
    ScrollToBottom,

    /// Scroll left *n* columns.
    ScrollLeftColumns(usize),

    /// Scroll right *n* columns.
    ScrollRightColumns(usize),

    /// Scroll left 1/*n* of the screen width.
    ScrollLeftScreenFraction(usize),

    /// Scroll right 1/*n* of the screen width.
    ScrollRightScreenFraction(usize),

    /// Toggle display of line numbers.
    ToggleLineNumbers,

    /// Toggle line wrapping mode.
    ToggleLineWrapping,

    /// Prompt the user for a line to move to.
    PromptGoToLine,

    /// Prompt the user for a search term.  The search will start at the beginning of the file.
    PromptSearchFromStart,

    /// Prompt the user for a search term.  The search will start at the top of the screen.
    PromptSearchForwards,

    /// Prompt the user for a search term.  The search will start from the bottom of the screen and
    /// proceed backwards.
    PromptSearchBackwards,

    /// Move to the previous match.
    PreviousMatch,

    /// Move to the next match.
    NextMatch,

    /// Move the previous line that contains a match.
    PreviousMatchLine,

    /// Move to the next line that contains a match.
    NextMatchLine,

    /// Move to the previous match, follow the current screen.
    PreviousMatchScreen,

    /// Move to the next match, follow the current screen.
    NextMatchScreen,

    /// Move to the first match.
    FirstMatch,

    /// Move to the last match.
    LastMatch,

    /// Append a digit to the "repeat count".
    /// The count defines how many times to do the next operation.
    AppendDigitToRepeatCount(usize),
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use Action::*;
        match *self {
            Quit => write!(f, "Quit"),
            Refresh => write!(f, "Refresh the screen"),
            Help => write!(f, "Show this help"),
            Cancel => write!(f, "Close help or any open prompt"),
            PreviousFile => write!(f, "Switch to the previous file"),
            NextFile => write!(f, "Switch to the next file"),
            ToggleRuler => write!(f, "Toggle the ruler"),
            ScrollUpLines(1) => write!(f, "Scroll up"),
            ScrollUpLines(n) => write!(f, "Scroll up {} lines", n),
            ScrollDownLines(1) => write!(f, "Scroll down"),
            ScrollDownLines(n) => write!(f, "Scroll down {} lines", n),
            ScrollUpScreenFraction(1) => write!(f, "Scroll up one screen"),
            ScrollUpScreenFraction(n) => write!(f, "Scroll up 1/{} screen", n),
            ScrollDownScreenFraction(1) => write!(f, "Scroll down one screen"),
            ScrollDownScreenFraction(n) => write!(f, "Scroll down 1/{} screen", n),
            ScrollToTop => write!(f, "Move to the start of the file"),
            ScrollToBottom => write!(f, "Move to and follow the end of the file"),
            ScrollLeftColumns(1) => write!(f, "Scroll left"),
            ScrollLeftColumns(n) => write!(f, "Scroll left {} columns", n),
            ScrollRightColumns(1) => write!(f, "Scroll right"),
            ScrollRightColumns(n) => write!(f, "Scroll right {} columns", n),
            ScrollLeftScreenFraction(1) => write!(f, "Scroll left one screen"),
            ScrollLeftScreenFraction(n) => write!(f, "Scroll left 1/{} screen", n),
            ScrollRightScreenFraction(1) => write!(f, "Scroll right one screen"),
            ScrollRightScreenFraction(n) => write!(f, "Scroll right 1/{} screen", n),
            ToggleLineNumbers => write!(f, "Toggle line numbers"),
            ToggleLineWrapping => write!(f, "Cycle through line wrapping modes"),
            PromptGoToLine => write!(f, "Go to position in file"),
            PromptSearchFromStart => write!(f, "Search from the start of the file"),
            PromptSearchForwards => write!(f, "Search forwards"),
            PromptSearchBackwards => write!(f, "Search backwards"),
            PreviousMatch => write!(f, "Move to the previous match"),
            NextMatch => write!(f, "Move to the next match"),
            PreviousMatchLine => write!(f, "Move to the previous matching line"),
            NextMatchLine => write!(f, "Move the the next matching line"),
            PreviousMatchScreen => write!(f, "Move to the previous match following the screen"),
            NextMatchScreen => write!(f, "Move to the next match following the screen"),
            FirstMatch => write!(f, "Move to the first match"),
            LastMatch => write!(f, "Move to the last match"),
            AppendDigitToRepeatCount(n) => write!(f, "Append digit {} to repeat count", n),
        }
    }
}

/// A handle that can be used to send actions to the pager.
#[derive(Clone)]
pub struct ActionSender(Arc<Mutex<EventSender>>);

impl ActionSender {
    /// Create an action sender for an event sender.
    pub(crate) fn new(event_sender: EventSender) -> ActionSender {
        ActionSender(Arc::new(Mutex::new(event_sender)))
    }

    /// Send an action to the pager.
    pub fn send(&self, action: Action) -> Result<(), Error> {
        let sender = self.0.lock().unwrap();
        sender.send(Event::Action(action))?;
        Ok(())
    }
}
