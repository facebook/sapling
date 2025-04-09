//! Key bindings.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use thiserror::Error;

use crate::action::Action;
use crate::file::FileIndex;

/// Key codes for key bindings.
///
pub use termwiz::input::KeyCode;

/// Keyboard modifiers for key bindings.
///
pub use termwiz::input::Modifiers;

/// Errors specific to bindings.
#[derive(Debug, Error)]
pub enum BindingError {
    /// Error when a binding is invalid.
    #[error("invalid keybinding: {0}")]
    Invalid(String),

    /// Binding is missing a parameter.
    #[error("{0} missing parameter {1}")]
    MissingParameter(String, usize),

    /// Integer parsing error.
    #[error("invalid integer")]
    InvalidInt(#[from] std::num::ParseIntError),

    /// Wrapped error within the context of a binding parameter.
    #[error("invalid {binding} parameter {index}")]
    ForParameter {
        /// Wrapped error.
        #[source]
        error: Box<BindingError>,

        /// Binding.
        binding: String,

        /// Parameter index.
        index: usize,
    },
}

impl BindingError {
    fn for_parameter(self, binding: String, index: usize) -> Self {
        Self::ForParameter {
            error: Box::new(self),
            binding,
            index,
        }
    }
}

type Result<T> = std::result::Result<T, BindingError>;

/// A key binding category.
///
/// Key bindings are listed by category in the help screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Category {
    /// Uncategorized actions.
    None,

    /// Actions for controlling the pager.
    General,

    /// Actions for moving around the file.
    Navigation,

    /// Actions that affect the presentation of the file.
    Presentation,

    /// Actions that initiate or modify searches.
    Searching,

    /// Actions that are hidden in help view (for example, too verbose).
    Hidden,
}

impl Category {
    /// Non-hidden categories.
    pub(crate) fn categories() -> impl Iterator<Item = Category> {
        [
            Category::General,
            Category::Navigation,
            Category::Presentation,
            Category::Searching,
            Category::None,
        ]
        .iter()
        .cloned()
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Category::None => f.write_str("Other"),
            Category::General => f.write_str("General"),
            Category::Navigation => f.write_str("Navigation"),
            Category::Presentation => f.write_str("Presentation"),
            Category::Searching => f.write_str("Searching"),
            Category::Hidden => f.write_str("Hidden"),
        }
    }
}

/// An action that may be bound to a key.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Binding {
    /// An action.
    Action(Action),

    /// A custom binding.
    Custom(CustomBinding),

    /// An unrecognised binding.
    Unrecognized(String),
}

impl Binding {
    /// Create new custom binding.
    ///
    /// When this binding is invoked, the callback is called.  The callback is provided with the
    /// file index of the file that is currently being displayed.  Note that this may differ from
    /// any of the file indexes returned by the `add` methods on the `Pager`, as additional file
    /// indexes can be allocated, e.g. for the help screen.
    pub fn custom(
        category: Category,
        description: impl Into<String>,
        callback: impl Fn(FileIndex) + Send + Sync + 'static,
    ) -> Self {
        Binding::Custom(CustomBinding::new(category, description, callback))
    }

    pub(crate) fn category(&self) -> Category {
        match self {
            Binding::Action(action) => {
                use Action::*;
                match action {
                    Quit | Refresh | Help | Cancel => Category::General,
                    PreviousFile
                    | NextFile
                    | ScrollUpLines(_)
                    | ScrollDownLines(_)
                    | ScrollUpScreenFraction(_)
                    | ScrollDownScreenFraction(_)
                    | ScrollToTop
                    | ScrollToBottom
                    | ScrollLeftColumns(_)
                    | ScrollRightColumns(_)
                    | ScrollLeftScreenFraction(_)
                    | ScrollRightScreenFraction(_)
                    | PromptGoToLine => Category::Navigation,
                    ToggleRuler | ToggleLineNumbers | ToggleLineWrapping => Category::Presentation,
                    PromptSearchFromStart
                    | PromptSearchForwards
                    | PromptSearchBackwards
                    | NextMatch
                    | PreviousMatch
                    | NextMatchLine
                    | PreviousMatchLine
                    | PreviousMatchScreen
                    | NextMatchScreen
                    | FirstMatch
                    | LastMatch => Category::Searching,
                    AppendDigitToRepeatCount(_) => Category::Hidden,
                }
            }
            Binding::Custom(binding) => binding.category,
            Binding::Unrecognized(_) => Category::None,
        }
    }

    /// Parse a keybinding identifier and list of parameters into a key binding.
    pub fn parse(ident: String, params: Vec<String>) -> Result<Self> {
        use Action::*;

        let param_usize = |index| -> Result<usize> {
            let value: &String = params
                .get(index)
                .ok_or_else(|| BindingError::MissingParameter(ident.clone(), index))?;
            let value = value
                .parse::<usize>()
                .map_err(|err| BindingError::from(err).for_parameter(ident.clone(), index))?;
            Ok(value)
        };

        let action = match ident.as_str() {
            "Quit" => Quit,
            "Refresh" => Refresh,
            "Help" => Help,
            "Cancel" => Cancel,
            "PreviousFile" => PreviousFile,
            "NextFile" => NextFile,
            "ToggleRuler" => ToggleRuler,
            "ScrollUpLines" => ScrollUpLines(param_usize(0)?),
            "ScrollDownLines" => ScrollDownLines(param_usize(0)?),
            "ScrollUpScreenFraction" => ScrollUpScreenFraction(param_usize(0)?),
            "ScrollDownScreenFraction" => ScrollDownScreenFraction(param_usize(0)?),
            "ScrollToTop" => ScrollToTop,
            "ScrollToBottom" => ScrollToBottom,
            "ScrollLeftColumns" => ScrollLeftColumns(param_usize(0)?),
            "ScrollRightColumns" => ScrollRightColumns(param_usize(0)?),
            "ScrollLeftScreenFraction" => ScrollLeftScreenFraction(param_usize(0)?),
            "ScrollRightScreenFraction" => ScrollRightScreenFraction(param_usize(0)?),
            "ToggleLineNumbers" => ToggleLineNumbers,
            "ToggleLineWrapping" => ToggleLineWrapping,
            "PromptGoToLine" => PromptGoToLine,
            "PromptSearchFromStart" => PromptSearchFromStart,
            "PromptSearchForwards" => PromptSearchForwards,
            "PromptSearchBackwards" => PromptSearchBackwards,
            "PreviousMatch" => PreviousMatch,
            "NextMatch" => NextMatch,
            "PreviousMatchLine" => PreviousMatchLine,
            "NextMatchLine" => NextMatchLine,
            "FirstMatch" => FirstMatch,
            "LastMatch" => LastMatch,
            _ => return Ok(Binding::Unrecognized(ident)),
        };

        Ok(Binding::Action(action))
    }
}

impl From<Action> for Binding {
    fn from(action: Action) -> Binding {
        Binding::Action(action)
    }
}

impl From<Action> for Option<Binding> {
    fn from(action: Action) -> Option<Binding> {
        Some(Binding::Action(action))
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Binding::Action(ref a) => write!(f, "{}", a),
            Binding::Custom(ref b) => write!(f, "{}", b.description),
            Binding::Unrecognized(ref s) => write!(f, "Unrecognized binding ({})", s),
        }
    }
}

static CUSTOM_BINDING_ID: AtomicUsize = AtomicUsize::new(0);

/// A custom binding.  This can be used by applications using streampager
/// to add custom actions on keys.
#[derive(Clone)]
pub struct CustomBinding {
    /// The id of this binding.  This is unique for each binding.
    id: usize,

    /// The category of this binding.
    category: Category,

    /// The description of this binding.
    description: String,

    /// Called when the action is triggered.
    callback: Arc<dyn Fn(FileIndex) + Sync + Send>,
}

impl CustomBinding {
    /// Create a new custom binding.
    ///
    /// The category and description are used in the help screen.  The
    /// callback is executed whenever the binding is triggered.
    pub fn new(
        category: Category,
        description: impl Into<String>,
        callback: impl Fn(FileIndex) + Sync + Send + 'static,
    ) -> CustomBinding {
        CustomBinding {
            id: CUSTOM_BINDING_ID.fetch_add(1, Ordering::SeqCst),
            category,
            description: description.into(),
            callback: Arc::new(callback),
        }
    }

    /// Trigger the binding and run its callback.
    pub fn run(&self, file_index: FileIndex) {
        (self.callback)(file_index)
    }
}

impl PartialEq for CustomBinding {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CustomBinding {}

impl std::hash::Hash for CustomBinding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Debug for CustomBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CustomBinding")
            .field(&self.id)
            .field(&self.description)
            .finish()
    }
}

/// A binding to a key and its associated help visibility.  Used by
/// the keymaps macro to provide binding configuration.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct BindingConfig {
    /// The binding.
    pub binding: Binding,

    /// Whether this binding is visible in the help screen.
    pub visible: bool,
}

/// A collection of key bindings.
#[derive(PartialEq, Eq)]
pub struct Keymap {
    /// Map of bindings from keys.
    bindings: HashMap<(Modifiers, KeyCode), Binding>,

    /// Map of visible keys from bindings.
    keys: HashMap<Binding, Vec<(Modifiers, KeyCode)>>,

    /// Order of `keys`.
    keys_order: Vec<Binding>,
}

impl<'a, I: IntoIterator<Item = &'a ((Modifiers, KeyCode), BindingConfig)>> From<I> for Keymap {
    fn from(iter: I) -> Keymap {
        let iter = iter.into_iter();
        let size_hint = iter.size_hint();
        let mut bindings = HashMap::with_capacity(size_hint.0);
        let mut keys = HashMap::with_capacity(size_hint.0);
        let mut keys_order = Vec::with_capacity(size_hint.0);
        for &((modifiers, keycode), ref binding_config) in iter {
            bindings.insert((modifiers, keycode), binding_config.binding.clone());
            if binding_config.visible {
                keys.entry(binding_config.binding.clone())
                    .or_insert_with(|| {
                        keys_order.push(binding_config.binding.clone());
                        Vec::new()
                    })
                    .push((modifiers, keycode));
            }
        }
        Keymap { bindings, keys, keys_order }
    }
}

impl Keymap {
    /// Create a new, empty, keymap.
    pub fn new() -> Self {
        Keymap {
            bindings: HashMap::new(),
            keys: HashMap::new(),
            keys_order: Vec::new(),
        }
    }

    /// Get the binding associated with a key combination.
    pub fn get(&self, modifiers: Modifiers, keycode: KeyCode) -> Option<&Binding> {
        self.bindings.get(&(modifiers, keycode))
    }

    /// Bind (or unbind) a key combination.
    pub fn bind(
        &mut self,
        modifiers: Modifiers,
        keycode: KeyCode,
        binding: impl Into<Option<Binding>>,
    ) -> &mut Self {
        self.bind_impl(modifiers, keycode, binding.into(), true)
    }

    /// Bind (or unbind) a key combination, but exclude it from the help screen.
    pub fn bind_hidden(
        &mut self,
        modifiers: Modifiers,
        keycode: KeyCode,
        binding: impl Into<Option<Binding>>,
    ) -> &mut Self {
        self.bind_impl(modifiers, keycode, binding.into(), false)
    }

    fn bind_impl(
        &mut self,
        modifiers: Modifiers,
        keycode: KeyCode,
        binding: Option<Binding>,
        visible: bool,
    ) -> &mut Self {
        if let Some(old_binding) = self.bindings.remove(&(modifiers, keycode)) {
            if let Some(keys) = self.keys.get_mut(&old_binding) {
                keys.retain(|&item| item != (modifiers, keycode));
            }
        }
        if let Some(binding) = binding {
            self.bindings.insert((modifiers, keycode), binding.clone());
            if visible {
                self.keys
                    .entry(binding.clone())
                    .or_insert_with(|| {
                        self.keys_order.push(binding);
                        Vec::new()
                    })
                    .push((modifiers, keycode));
            }
        }
        self
    }

    pub(crate) fn iter_keys(&self) -> impl Iterator<Item = (&Binding, &Vec<(Modifiers, KeyCode)>)> {
        self.keys_order.iter().filter_map(|b| {
             self.keys.get(b).map(|v| (b, v))
        })
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::from(crate::keymaps::default::KEYMAP.iter())
    }
}

impl std::fmt::Debug for Keymap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Keymap")
            .field(&format!("<{} keys bound>", self.bindings.len()))
            .finish()
    }
}
