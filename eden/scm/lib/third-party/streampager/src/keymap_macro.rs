//! Keymap macro

// Keymap macro implementation.
//
// Token-tree muncher: { rest } ( visible ) ( modifiers ) ( keys ) [ data ]
//
// Consumes definition from 'rest'.  Modifiers are accumulated in 'modifiers'.  Key definitions are
// accumulated in 'keys'.  Bindings are accumulated in 'data'.
macro_rules! keymap_impl {
    // Base case: generate keymap data.
    ( {} ( $visible:literal ) () () $data:tt ) => {
        pub(crate) static KEYMAP: $crate::keymaps::KeymapData = &$data;
    };

    // , (consume comma between keys)
    (
        { , $( $rest:tt )* }
        ( $visible:literal )
        ( )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };

    // => Binding (termination)
    (
        {
            => $action:ident $( ( $( $action_params:tt )* ) )? ;
            $( $rest:tt )*
        }
        ( $visible:literal )
        ( )
        ( )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( )
            ( )
            [ $( $data )* ]
        }
    };

    // => Binding (assign key)
    (
        {
            => $action:ident $( ( $( $action_params:tt )* ) )? ;
            $( $rest:tt )*
        }
        ( $visible:literal )
        ( )
        ( $key:tt $key_visible:literal $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            {
                => $action $( ( $( $action_params )* ) )? ;
                $( $rest )*
            }
            ( $visible )
            ( )
            ( $( $keys )* )
            [
                $( $data )*
                (
                    $key,
                    $crate::bindings::BindingConfig {
                        binding: $crate::bindings::Binding::Action(
                            $crate::action::Action::$action $( ( $( $action_params )* ) )?
                        ),
                        visible: $key_visible,
                    },
                ),
            ]
        }
    };

    // CTRL
    (
        { CTRL $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( $( $modifier )* CTRL )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };

    // SHIFT
    (
        { SHIFT $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( $( $modifier )* SHIFT )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };

    // ALT
    (
        { ALT $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( $( $modifier )* ALT )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };

    // SUPER
    (
        { SUPER $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( $visible )
            ( $( $modifier )* SUPER )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };

    // Character key (e.g. 'c')
    (
        { $key:literal $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( true )
            ( )
            (
                $( $keys )*
                (
                    termwiz::input::Modifiers::from_bits_truncate(
                        $( termwiz::input::Modifiers::$modifier.bits() | )*
                        termwiz::input::Modifiers::NONE.bits()
                    ),
                    termwiz::input::KeyCode::Char($key),
                )
                $visible
            )
            [ $( $data )* ]
        }
    };

    // F <number>
    (
        { F $num:literal $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( true )
            ( )
            (
                $( $keys )*
                (
                    termwiz::input::Modifiers::from_bits_truncate(
                        $( termwiz::input::Modifiers::$modifier.bits() | )*
                        termwiz::input::Modifiers::NONE.bits()
                    ),
                    termwiz::input::KeyCode::Function($num),
                )
                $visible
            )
            [ $( $data )* ]
        }
    };

    // KeyCode
    (
        { $key:ident $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $rest )* }
            ( true )
            ( )
            (
                $( $keys )*
                (
                    termwiz::input::Modifiers::from_bits_truncate(
                        $( termwiz::input::Modifiers::$modifier.bits() | )*
                        termwiz::input::Modifiers::NONE.bits()
                    ),
                    termwiz::input::KeyCode::$key,
                )
                $visible
            )
            [ $( $data )* ]
        }
    };

    // ( hidden binding )
    (
        { ( $( $bind:tt )* ) $( $rest:tt )* }
        ( $visible:literal )
        ( $( $modifier:ident )* )
        ( $( $keys:tt )* )
        [ $( $data:tt )* ]
    ) => {
        keymap_impl! {
            { $( $bind )* $( $rest )* }
            ( false )
            ( $( $modifier )* )
            ( $( $keys )* )
            [ $( $data )* ]
        }
    };
}

macro_rules! keymap {
    ( $( $all:tt )* ) => {
        keymap_impl! { { $( $all )* } (true) () () [] }
    };
}
