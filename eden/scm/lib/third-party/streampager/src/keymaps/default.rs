//! Default keymap

keymap! {
    CTRL 'c', 'q', ('Q') => Quit;
    Escape => Cancel;
    CTRL 'l', 'r' => Refresh;
    CTRL 'r' => ToggleRuler;
    UpArrow, 'k', (CTRL 'k'), (CTRL 'p') => ScrollUpLines(1);
    DownArrow, 'j', (CTRL 'n'), Enter => ScrollDownLines(1);
    SHIFT UpArrow, (ApplicationUpArrow) => ScrollUpScreenFraction(4);
    SHIFT DownArrow, (ApplicationDownArrow) => ScrollDownScreenFraction(4);
    CTRL UpArrow, 'u', CTRL 'u' => ScrollUpScreenFraction(2);
    CTRL DownArrow, 'd', CTRL 'd' => ScrollDownScreenFraction(2);
    PageUp, Backspace, 'b', CTRL 'b', ALT 'v' => ScrollUpScreenFraction(1);
    PageDown, ' ', 'f', CTRL 'f', CTRL 'v' => ScrollDownScreenFraction(1);
    Home, 'g', '<' => ScrollToTop;
    End, 'F', 'G', '>' => ScrollToBottom;
    LeftArrow => ScrollLeftColumns(4);
    RightArrow => ScrollRightColumns(4);
    SHIFT LeftArrow => ScrollLeftScreenFraction(4);
    SHIFT RightArrow => ScrollRightScreenFraction(4);
    '[', SHIFT Tab => PreviousFile;
    ']', Tab => NextFile;
    'h', F 1 => Help;
    '#' => ToggleLineNumbers;
    '\\' => ToggleLineWrapping;
    ':', '%' => PromptGoToLine;
    '/' => PromptSearchForwards;
    '?' => PromptSearchBackwards;
    ',' => PreviousMatch;
    '.' => NextMatch;
    'p', ('N') => PreviousMatchScreen;
    'n' => NextMatchScreen;
    '(' => FirstMatch;
    ')' => LastMatch;
    '0' => AppendDigitToRepeatCount(0);
    '1' => AppendDigitToRepeatCount(1);
    '2' => AppendDigitToRepeatCount(2);
    '3' => AppendDigitToRepeatCount(3);
    '4' => AppendDigitToRepeatCount(4);
    '5' => AppendDigitToRepeatCount(5);
    '6' => AppendDigitToRepeatCount(6);
    '7' => AppendDigitToRepeatCount(7);
    '8' => AppendDigitToRepeatCount(8);
    '9' => AppendDigitToRepeatCount(9);
}
