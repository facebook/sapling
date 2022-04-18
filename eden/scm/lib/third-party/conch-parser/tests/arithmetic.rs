#![deny(rust_2018_idioms)]
use conch_parser::ast::Arithmetic::*;
use conch_parser::ast::DefaultArithmetic as Arithmetic;
use conch_parser::ast::ParameterSubstitution::Arith;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_arithmetic_substitution_valid() {
    fn x() -> Box<Arithmetic> {
        Box::new(Var(String::from("x")))
    }
    fn y() -> Box<Arithmetic> {
        Box::new(Var(String::from("y")))
    }
    fn z() -> Box<Arithmetic> {
        Box::new(Var(String::from("z")))
    }

    let cases = vec![
        ("$(( x ))", *x()),
        ("$(( 5 ))", Literal(5)),
        ("$(( 0 ))", Literal(0)),
        ("$(( 010 ))", Literal(8)),
        ("$(( 0xa ))", Literal(10)),
        ("$(( 0Xa ))", Literal(10)),
        ("$(( 0xA ))", Literal(10)),
        ("$(( 0XA ))", Literal(10)),
        ("$(( x++ ))", PostIncr(String::from("x"))),
        ("$(( x-- ))", PostDecr(String::from("x"))),
        ("$(( ++x ))", PreIncr(String::from("x"))),
        ("$(( --x ))", PreDecr(String::from("x"))),
        ("$(( +x ))", UnaryPlus(x())),
        ("$(( -x ))", UnaryMinus(x())),
        ("$(( !x ))", LogicalNot(x())),
        ("$(( ~x ))", BitwiseNot(x())),
        ("$(( x ** y))", Pow(x(), y())),
        ("$(( x * y ))", Mult(x(), y())),
        ("$(( x / y ))", Div(x(), y())),
        ("$(( x % y ))", Modulo(x(), y())),
        ("$(( x + y ))", Add(x(), y())),
        ("$(( x - y ))", Sub(x(), y())),
        ("$(( x << y ))", ShiftLeft(x(), y())),
        ("$(( x >> y ))", ShiftRight(x(), y())),
        ("$(( x < y ))", Less(x(), y())),
        ("$(( x <= y ))", LessEq(x(), y())),
        ("$(( x > y ))", Great(x(), y())),
        ("$(( x >= y ))", GreatEq(x(), y())),
        ("$(( x == y ))", Eq(x(), y())),
        ("$(( x != y ))", NotEq(x(), y())),
        ("$(( x & y ))", BitwiseAnd(x(), y())),
        ("$(( x ^ y ))", BitwiseXor(x(), y())),
        ("$(( x | y ))", BitwiseOr(x(), y())),
        ("$(( x && y ))", LogicalAnd(x(), y())),
        ("$(( x || y ))", LogicalOr(x(), y())),
        ("$(( x ? y : z ))", Ternary(x(), y(), z())),
        ("$(( x = y ))", Assign(String::from("x"), y())),
        (
            "$(( x *= y ))",
            Assign(String::from("x"), Box::new(Mult(x(), y()))),
        ),
        (
            "$(( x /= y ))",
            Assign(String::from("x"), Box::new(Div(x(), y()))),
        ),
        (
            "$(( x %= y ))",
            Assign(String::from("x"), Box::new(Modulo(x(), y()))),
        ),
        (
            "$(( x += y ))",
            Assign(String::from("x"), Box::new(Add(x(), y()))),
        ),
        (
            "$(( x -= y ))",
            Assign(String::from("x"), Box::new(Sub(x(), y()))),
        ),
        (
            "$(( x <<= y ))",
            Assign(String::from("x"), Box::new(ShiftLeft(x(), y()))),
        ),
        (
            "$(( x >>= y ))",
            Assign(String::from("x"), Box::new(ShiftRight(x(), y()))),
        ),
        (
            "$(( x &= y ))",
            Assign(String::from("x"), Box::new(BitwiseAnd(x(), y()))),
        ),
        (
            "$(( x ^= y ))",
            Assign(String::from("x"), Box::new(BitwiseXor(x(), y()))),
        ),
        (
            "$(( x |= y ))",
            Assign(String::from("x"), Box::new(BitwiseOr(x(), y()))),
        ),
        (
            "$(( x = 5, x + y ))",
            Sequence(vec![
                Assign(String::from("x"), Box::new(Literal(5))),
                Add(x(), y()),
            ]),
        ),
        ("$(( x + (y - z) ))", Add(x(), Box::new(Sub(y(), z())))),
    ];

    for (s, a) in cases.into_iter() {
        let correct = word_subst(Arith(Some(a)));
        match make_parser(s).parameter() {
            Ok(w) => {
                if w != correct {
                    panic!(
                        "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                        s, w, correct
                    )
                }
            }
            Err(err) => panic!("Failed to parse the source \"{}\": {}", s, err),
        }
    }

    let correct = word_subst(Arith(None));
    assert_eq!(correct, make_parser("$(( ))").parameter().unwrap());
}

#[test]
fn test_arithmetic_substitution_left_to_right_associativity() {
    fn x() -> Box<Arithmetic> {
        Box::new(Var(String::from("x")))
    }
    fn y() -> Box<Arithmetic> {
        Box::new(Var(String::from("y")))
    }
    fn z() -> Box<Arithmetic> {
        Box::new(Var(String::from("z")))
    }

    macro_rules! check {
        ($constructor:path, $op:tt) => {{
            let correct = word_subst(Arith(Some($constructor(
                Box::new($constructor(x(), y())),
                z(),
            ))));

            let src = format!("$((x {0} y {0} z))", stringify!($op));
            match make_parser(&src).parameter() {
                Ok(w) => {
                    if w != correct {
                        panic!(
                            "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                            src, w, correct
                        )
                    }
                }
                Err(err) => panic!("Failed to parse the source \"{}\": {}", src, err),
            }
        }};

        (assig: $constructor:path, $op:tt) => {{
            let correct = word_subst(Arith(Some(Assign(
                String::from("x"),
                Box::new($constructor(
                    x(),
                    Box::new(Assign(String::from("y"), Box::new($constructor(y(), z())))),
                )),
            ))));

            let src = format!("$((x {0}= y {0}= z))", stringify!($op));
            match make_parser(&src).parameter() {
                Ok(w) => {
                    if w != correct {
                        panic!(
                            "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                            src, w, correct
                        )
                    }
                }
                Err(err) => panic!("Failed to parse the source \"{}\": {}", src, err),
            }
        }};
    }

    check!(Mult,       * );
    check!(Div,        / );
    check!(Modulo,     % );
    check!(Add,        + );
    check!(Sub,        - );
    check!(ShiftLeft,  <<);
    check!(ShiftRight, >>);
    check!(Less,       < );
    check!(LessEq,     <=);
    check!(Great ,     > );
    check!(GreatEq,    >=);
    check!(Eq,         ==);
    check!(NotEq,      !=);
    check!(BitwiseAnd, & );
    check!(BitwiseXor, ^ );
    check!(BitwiseOr,  | );
    check!(LogicalAnd, &&);
    check!(LogicalOr,  ||);

    check!(assig: Mult,       * );
    check!(assig: Div,        / );
    check!(assig: Modulo,     % );
    check!(assig: Add,        + );
    check!(assig: Sub,        - );
    check!(assig: ShiftLeft,  <<);
    check!(assig: ShiftRight, >>);
    check!(assig: BitwiseAnd, & );
    check!(assig: BitwiseXor, ^ );
    check!(assig: BitwiseOr,  | );

    let correct = word_subst(Arith(Some(Assign(
        String::from("x"),
        Box::new(Assign(String::from("y"), z())),
    ))));
    assert_eq!(
        correct,
        make_parser("$(( x = y = z ))").parameter().unwrap()
    );
}

#[test]
fn test_arithmetic_substitution_right_to_left_associativity() {
    fn x() -> Box<Arithmetic> {
        Box::new(Var(String::from("x")))
    }
    fn y() -> Box<Arithmetic> {
        Box::new(Var(String::from("y")))
    }
    fn z() -> Box<Arithmetic> {
        Box::new(Var(String::from("z")))
    }
    fn w() -> Box<Arithmetic> {
        Box::new(Var(String::from("w")))
    }
    fn v() -> Box<Arithmetic> {
        Box::new(Var(String::from("v")))
    }

    let cases = vec![
        ("$(( x ** y ** z ))", Pow(x(), Box::new(Pow(y(), z())))),
        (
            "$(( x ? y ? z : w : v ))",
            Ternary(x(), Box::new(Ternary(y(), z(), w())), v()),
        ),
    ];

    for (s, a) in cases.into_iter() {
        let correct = word_subst(Arith(Some(a)));
        match make_parser(s).parameter() {
            Ok(w) => {
                if w != correct {
                    panic!(
                        "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                        s, w, correct
                    )
                }
            }
            Err(err) => panic!("Failed to parse the source \"{}\": {}", s, err),
        }
    }
}

#[test]
fn test_arithmetic_substitution_invalid() {
    let cases = vec![
        // Pre/post increment/decrement must be applied on a variable
        // Otherwise becomes `expr+(+expr)` or `expr-(-expr)`
        ("$(( 5++ ))", Unexpected(Token::ParenClose, src(8, 1, 9))),
        ("$(( 5-- ))", Unexpected(Token::ParenClose, src(8, 1, 9))),
        (
            "$(( (x + y)++ ))",
            Unexpected(Token::ParenClose, src(14, 1, 15)),
        ),
        (
            "$(( (x + y)-- ))",
            Unexpected(Token::ParenClose, src(14, 1, 15)),
        ),
        // Pre/post increment/decrement must be applied on a variable
        (
            "$(( ++5 ))",
            Unexpected(Token::Literal(String::from("5")), src(6, 1, 7)),
        ),
        (
            "$(( --5 ))",
            Unexpected(Token::Literal(String::from("5")), src(6, 1, 7)),
        ),
        (
            "$(( ++(x + y) ))",
            Unexpected(Token::ParenOpen, src(6, 1, 7)),
        ),
        (
            "$(( --(x + y) ))",
            Unexpected(Token::ParenOpen, src(6, 1, 7)),
        ),
        // Incomplete commands
        ("$(( + ))", Unexpected(Token::ParenClose, src(6, 1, 7))),
        ("$(( - ))", Unexpected(Token::ParenClose, src(6, 1, 7))),
        ("$(( ! ))", Unexpected(Token::ParenClose, src(6, 1, 7))),
        ("$(( ~ ))", Unexpected(Token::ParenClose, src(6, 1, 7))),
        ("$(( x ** ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x *  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x /  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x %  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x +  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x -  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x << ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x >> ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x <  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x <= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x >  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x >= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x == ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x != ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x &  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x ^  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x |  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x && ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x || ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x =  ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x *= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x /= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x %= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x += ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x -= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x <<=))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x >>=))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x &= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x ^= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        ("$(( x |= ))", Unexpected(Token::ParenClose, src(9, 1, 10))),
        (
            "$(( x ? y : ))",
            Unexpected(Token::ParenClose, src(12, 1, 13)),
        ),
        (
            "$(( x ?     ))",
            Unexpected(Token::ParenClose, src(12, 1, 13)),
        ),
        (
            "$(( x = 5, ))",
            Unexpected(Token::ParenClose, src(11, 1, 12)),
        ),
        (
            "$(( x + () ))",
            Unexpected(Token::ParenClose, src(9, 1, 10)),
        ),
        // Missing first operand
        ("$(( ** y  ))", Unexpected(Token::Star, src(4, 1, 5))),
        ("$(( * y   ))", Unexpected(Token::Star, src(4, 1, 5))),
        ("$(( / y   ))", Unexpected(Token::Slash, src(4, 1, 5))),
        ("$(( % y   ))", Unexpected(Token::Percent, src(4, 1, 5))),
        ("$(( << y  ))", Unexpected(Token::DLess, src(4, 1, 5))),
        ("$(( >> y  ))", Unexpected(Token::DGreat, src(4, 1, 5))),
        ("$(( < y   ))", Unexpected(Token::Less, src(4, 1, 5))),
        ("$(( <= y  ))", Unexpected(Token::Less, src(4, 1, 5))),
        ("$(( > y   ))", Unexpected(Token::Great, src(4, 1, 5))),
        ("$(( >= y  ))", Unexpected(Token::Great, src(4, 1, 5))),
        ("$(( == y  ))", Unexpected(Token::Equals, src(4, 1, 5))),
        ("$(( & y   ))", Unexpected(Token::Amp, src(4, 1, 5))),
        ("$(( ^ y   ))", Unexpected(Token::Caret, src(4, 1, 5))),
        ("$(( | y   ))", Unexpected(Token::Pipe, src(4, 1, 5))),
        ("$(( && y  ))", Unexpected(Token::AndIf, src(4, 1, 5))),
        ("$(( || y  ))", Unexpected(Token::OrIf, src(4, 1, 5))),
        ("$(( = y   ))", Unexpected(Token::Equals, src(4, 1, 5))),
        ("$(( *= y  ))", Unexpected(Token::Star, src(4, 1, 5))),
        ("$(( /= y  ))", Unexpected(Token::Slash, src(4, 1, 5))),
        ("$(( %= y  ))", Unexpected(Token::Percent, src(4, 1, 5))),
        ("$(( <<= y ))", Unexpected(Token::DLess, src(4, 1, 5))),
        ("$(( >>= y ))", Unexpected(Token::DGreat, src(4, 1, 5))),
        ("$(( &= y  ))", Unexpected(Token::Amp, src(4, 1, 5))),
        ("$(( ^= y  ))", Unexpected(Token::Caret, src(4, 1, 5))),
        ("$(( |= y  ))", Unexpected(Token::Pipe, src(4, 1, 5))),
        ("$(( ? y : z ))", Unexpected(Token::Question, src(4, 1, 5))),
        ("$(( , x + y ))", Unexpected(Token::Comma, src(4, 1, 5))),
        // Each of the following leading tokens will be parsed as unary
        // operators, thus the error will occur on the `=`.
        ("$(( != y  ))", Unexpected(Token::Equals, src(5, 1, 6))),
        ("$(( += y  ))", Unexpected(Token::Equals, src(5, 1, 6))),
        ("$(( -= y  ))", Unexpected(Token::Equals, src(5, 1, 6))),
    ];

    for (s, correct) in cases.into_iter() {
        match make_parser(s).parameter() {
            Ok(w) => panic!("Unexpectedly parsed the source \"{}\" as\n{:?}", s, w),
            Err(ref err) => {
                if err != &correct {
                    panic!(
                        "Expected the source \"{}\" to return the error `{:?}`, but got `{:?}`",
                        s, correct, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_arithmetic_substitution_precedence() {
    fn var(x: &str) -> Box<Arithmetic> {
        Box::new(Var(String::from(x)))
    }

    let cases = vec![
        ("~o++", BitwiseNot(Box::new(PostIncr(String::from("o"))))),
        ("~(o+p)", BitwiseNot(Box::new(Add(var("o"), var("p"))))),
        ("-o++", UnaryMinus(Box::new(PostIncr(String::from("o"))))),
        ("-(o+p)", UnaryMinus(Box::new(Add(var("o"), var("p"))))),
        ("++o", PreIncr(String::from("o"))),
    ];

    for (s, end) in cases.into_iter() {
        let correct = word_subst(Arith(Some(Sequence(vec![
            *var("x"),
            Assign(
                String::from("a"),
                Box::new(Ternary(
                    var("b"),
                    var("c"),
                    Box::new(LogicalOr(
                        var("d"),
                        Box::new(LogicalAnd(
                            var("e"),
                            Box::new(BitwiseOr(
                                var("f"),
                                Box::new(BitwiseXor(
                                    var("g"),
                                    Box::new(BitwiseAnd(
                                        var("h"),
                                        Box::new(Eq(
                                            var("i"),
                                            Box::new(Less(
                                                var("j"),
                                                Box::new(ShiftLeft(
                                                    var("k"),
                                                    Box::new(Add(
                                                        var("l"),
                                                        Box::new(Mult(
                                                            var("m"),
                                                            Box::new(Pow(var("n"), Box::new(end))),
                                                        )),
                                                    )),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            )),
                        )),
                    )),
                )),
            ),
        ]))));

        let src = format!(
            "$(( x , a = b?c: d || e && f | g ^ h & i == j < k << l + m * n ** {} ))",
            s
        );
        match make_parser(&src).parameter() {
            Ok(w) => {
                if w != correct {
                    panic!(
                        "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                        src, w, correct
                    )
                }
            }
            Err(err) => panic!("Failed to parse the source \"{}\": {}", src, err),
        }
    }
}

#[test]
fn test_arithmetic_substitution_operators_of_equal_precedence() {
    fn x() -> Box<Arithmetic> {
        Box::new(Var(String::from("x")))
    }
    fn y() -> Box<Arithmetic> {
        Box::new(Var(String::from("y")))
    }
    fn z() -> Box<Arithmetic> {
        Box::new(Var(String::from("z")))
    }
    fn w() -> Box<Arithmetic> {
        Box::new(Var(String::from("w")))
    }

    let cases = vec![
        ("$(( x != y == z ))", Eq(Box::new(NotEq(x(), y())), z())),
        ("$(( x == y != z ))", NotEq(Box::new(Eq(x(), y())), z())),
        ("$(( x <  y < z ))", Less(Box::new(Less(x(), y())), z())),
        ("$(( x <= y < z ))", Less(Box::new(LessEq(x(), y())), z())),
        ("$(( x >  y < z ))", Less(Box::new(Great(x(), y())), z())),
        ("$(( x >= y < z ))", Less(Box::new(GreatEq(x(), y())), z())),
        (
            "$(( x << y >> z ))",
            ShiftRight(Box::new(ShiftLeft(x(), y())), z()),
        ),
        (
            "$(( x >> y << z ))",
            ShiftLeft(Box::new(ShiftRight(x(), y())), z()),
        ),
        ("$(( x + y - z ))", Sub(Box::new(Add(x(), y())), z())),
        ("$(( x - y + z ))", Add(Box::new(Sub(x(), y())), z())),
        (
            "$(( x * y / z % w ))",
            Modulo(Box::new(Div(Box::new(Mult(x(), y())), z())), w()),
        ),
        (
            "$(( x * y % z / w ))",
            Div(Box::new(Modulo(Box::new(Mult(x(), y())), z())), w()),
        ),
        (
            "$(( x / y * z % w ))",
            Modulo(Box::new(Mult(Box::new(Div(x(), y())), z())), w()),
        ),
        (
            "$(( x / y % z * w ))",
            Mult(Box::new(Modulo(Box::new(Div(x(), y())), z())), w()),
        ),
        (
            "$(( x % y * z / w ))",
            Div(Box::new(Mult(Box::new(Modulo(x(), y())), z())), w()),
        ),
        (
            "$(( x % y / z * w ))",
            Mult(Box::new(Div(Box::new(Modulo(x(), y())), z())), w()),
        ),
        (
            "$(( +!~x ))",
            UnaryPlus(Box::new(LogicalNot(Box::new(BitwiseNot(x()))))),
        ),
        (
            "$(( +~!x ))",
            UnaryPlus(Box::new(BitwiseNot(Box::new(LogicalNot(x()))))),
        ),
        (
            "$(( !+~x ))",
            LogicalNot(Box::new(UnaryPlus(Box::new(BitwiseNot(x()))))),
        ),
        (
            "$(( !~+x ))",
            LogicalNot(Box::new(BitwiseNot(Box::new(UnaryPlus(x()))))),
        ),
        (
            "$(( ~+!x ))",
            BitwiseNot(Box::new(UnaryPlus(Box::new(LogicalNot(x()))))),
        ),
        (
            "$(( ~!+x ))",
            BitwiseNot(Box::new(LogicalNot(Box::new(UnaryPlus(x()))))),
        ),
        (
            "$(( -!~x ))",
            UnaryMinus(Box::new(LogicalNot(Box::new(BitwiseNot(x()))))),
        ),
        (
            "$(( -~!x ))",
            UnaryMinus(Box::new(BitwiseNot(Box::new(LogicalNot(x()))))),
        ),
        (
            "$(( !-~x ))",
            LogicalNot(Box::new(UnaryMinus(Box::new(BitwiseNot(x()))))),
        ),
        (
            "$(( !~-x ))",
            LogicalNot(Box::new(BitwiseNot(Box::new(UnaryMinus(x()))))),
        ),
        (
            "$(( ~-!x ))",
            BitwiseNot(Box::new(UnaryMinus(Box::new(LogicalNot(x()))))),
        ),
        (
            "$(( ~!-x ))",
            BitwiseNot(Box::new(LogicalNot(Box::new(UnaryMinus(x()))))),
        ),
        (
            "$(( !~++x ))",
            LogicalNot(Box::new(BitwiseNot(Box::new(PreIncr(String::from("x")))))),
        ),
        (
            "$(( ~!++x ))",
            BitwiseNot(Box::new(LogicalNot(Box::new(PreIncr(String::from("x")))))),
        ),
        (
            "$(( !~--x ))",
            LogicalNot(Box::new(BitwiseNot(Box::new(PreDecr(String::from("x")))))),
        ),
        (
            "$(( ~!--x ))",
            BitwiseNot(Box::new(LogicalNot(Box::new(PreDecr(String::from("x")))))),
        ),
        ("$(( -+x ))", UnaryMinus(Box::new(UnaryPlus(x())))),
        ("$(( +-x ))", UnaryPlus(Box::new(UnaryMinus(x())))),
    ];

    for (s, a) in cases.into_iter() {
        let correct = word_subst(Arith(Some(a)));
        match make_parser(s).parameter() {
            Ok(w) => {
                if w != correct {
                    panic!(
                        "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                        s, w, correct
                    )
                }
            }
            Err(err) => panic!("Failed to parse the source \"{}\": {}", s, err),
        }
    }
}
