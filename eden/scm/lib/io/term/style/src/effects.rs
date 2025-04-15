/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::io::Write;

use termwiz::caps::Capabilities;
pub use termwiz::caps::ColorLevel;
use termwiz::caps::ProbeHints;
use termwiz::cell::CellAttributes;
use termwiz::cell::Intensity;
use termwiz::cell::Underline;
use termwiz::color::AnsiColor;
use termwiz::color::ColorSpec;
use termwiz::color::RgbColor;
use termwiz::render::RenderTty;
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::surface::Change;

/// Evaluate style specs given supported color level, yielding a
/// CellAttributes object with corresponding fields filled in.
///
/// Style spec format:
///    - effect: a concrete text modifier (e.g. "green" or "bold").
///              Various color formats are supported depending on color level:
///                 4-bit: green, red_background, etc.
///                 8-bit: color123, color100_background, etc
///                 24-bit: #FFF, #A1B2C3, DarkOrange2 (and more - see termwiz)
///    - style: effect(+effect)*
///             A list of effects separated by "+". Effects are only applied
///             if all effects in the list are valid.
///    - spec: style(:style)*
///            Priority order list of styles. First valid style wins.
///    - specs: spec( spec)*
///             Space separated list of specs. All specs are applied in order.
///
/// Examples:
///
/// Pick one from "color214" and "yellow", then combined with "bold":
///
/// ```text
/// color214:yellow bold
/// ```
///
/// Pick one from "color214" and "yellow+bold":
///
/// ```text
/// color214:yellow+bold
/// ```
pub fn eval_style(level: ColorLevel, style_specs: &str) -> CellAttributes {
    let mut attrs = CellAttributes::blank();

    'specs: for spec in style_specs.split_whitespace() {
        for style in spec.split(':') {
            if eval_effects(level, style.split('+'), &mut attrs) {
                continue 'specs;
            }
        }

        tracing::warn!(spec, "couldn't apply style spec");
    }

    attrs
}

struct DumbTty<'a> {
    w: &'a mut dyn Write,
}

impl RenderTty for DumbTty<'_> {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        Ok((80, 26))
    }
}

impl io::Write for DumbTty<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.w.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

// The main purpose of this object is to cache the Capabilities.
pub struct Styler {
    level: ColorLevel,
    renderer: TerminfoRenderer,
}

fn enable_compat_mode() -> bool {
    std::env::var("SL_DISABLE_TERMINFO_COMPAT").is_err()
}

impl Styler {
    /// Initialize a styler, letting termwiz sniff the supported color level.
    pub fn new() -> termwiz::Result<Styler> {
        let mut hints = ProbeHints::new_from_env()
            .force_terminfo_render_to_use_ansi_sgr(Some(enable_compat_mode()));
        if cfg!(test) || std::env::var("TESTTMP").is_ok() {
            // Use a consistent (and non-existent) terminal to avoid differences in tests.
            hints = hints.term(Some("fake-term".to_string()));
        }
        let caps = Capabilities::new_with_hints(hints)?;
        Ok(Styler {
            level: caps.color_level(),
            renderer: TerminfoRenderer::new(caps),
        })
    }

    /// Initialize a styler using specified color level. Python uses
    /// this for now to keep closer compatibility with the Python
    /// color support detection.
    pub fn from_level(level: ColorLevel) -> termwiz::Result<Styler> {
        let mut hints = ProbeHints::default()
            .color_level(Some(level))
            .force_terminfo_render_to_use_ansi_sgr(Some(enable_compat_mode()));
        if cfg!(test) || std::env::var("TESTTMP").is_ok() {
            // Use a consistent (and non-existent) terminal to avoid differences in tests.
            hints = hints.term(Some("fake-term".to_string()));
        }
        let caps = Capabilities::new_with_hints(hints)?;
        let renderer = TerminfoRenderer::new(caps);
        Ok(Styler { level, renderer })
    }

    pub fn render_bytes(&mut self, style_specs: &str, text: &str) -> termwiz::Result<Vec<u8>> {
        let mut buf: Vec<u8> = Vec::new();
        self.render(&mut buf, style_specs, text)?;
        Ok(buf)
    }

    pub fn render(
        &mut self,
        w: &mut dyn Write,
        style_specs: &str,
        text: &str,
    ) -> termwiz::Result<()> {
        if style_specs.is_empty() {
            w.write_all(text.as_bytes())?;
            return Ok(());
        }

        let mut tty = DumbTty { w };

        // Line breaks within escape sequences don't look right, so
        // process each line's contents separately.
        for (idx, line) in text.split('\n').enumerate() {
            if idx > 0 {
                tty.write_all(b"\n")?;
            }

            if line.is_empty() {
                continue;
            }

            self.renderer.render_to(
                &[
                    Change::AllAttributes(eval_style(self.level, style_specs)),
                    Change::Text(line.to_string()),
                    Change::AllAttributes(CellAttributes::blank()),
                ],
                &mut tty,
            )?;
        }

        Ok(())
    }
}

/// Apply given effects to attrs iff all effects are valid.
fn eval_effects<'a>(
    level: ColorLevel,
    effects: impl IntoIterator<Item = &'a str>,
    attrs: &mut CellAttributes,
) -> bool {
    let mut tentative_attrs = attrs.clone();

    for mut effect in effects {
        if eval_non_color(effect, &mut tentative_attrs) {
            continue;
        }

        let mut is_bg = false;
        if let Some(bg_name) = effect.strip_suffix("_background") {
            effect = bg_name;
            is_bg = true;
        }

        if let Some(ansi) = ansi_color(effect) {
            set_color(is_bg, &mut tentative_attrs, ansi);
            continue;
        }

        if let Some(color_256) = effect.strip_prefix("color") {
            if level == ColorLevel::Sixteen {
                return false;
            }

            if let Ok(idx) = color_256.parse::<u8>() {
                set_color(is_bg, &mut tentative_attrs, ColorSpec::PaletteIndex(idx));
                continue;
            }
        }

        // Supports various 24 bit color formats including our
        // standard #FFF or #FFFFFF.
        let rgb = if effect.starts_with('#') && effect.len() == 4 {
            // termwiz converts #FFF to #F0F0F0, but we want #FFFFFF (a la CSS).
            RgbColor::from_named_or_rgb_string(&format!(
                "#{0}{0}{1}{1}{2}{2}",
                &effect[1..2],
                &effect[2..3],
                &effect[3..4]
            ))
        } else {
            RgbColor::from_named_or_rgb_string(effect)
        };
        if let Some(rgb) = rgb {
            if level != ColorLevel::TrueColor {
                return false;
            }

            set_color(is_bg, &mut tentative_attrs, rgb);
            continue;
        }

        tracing::warn!(effect, "unknown style effect");
        return false;
    }

    // Only updated attrs if all effects were valid.
    *attrs = tentative_attrs;
    true
}

fn eval_non_color(effect: &str, attrs: &mut CellAttributes) -> bool {
    match effect {
        "none" => {
            *attrs = CellAttributes::blank();
        }
        "bold" => {
            attrs.set_intensity(Intensity::Bold);
        }
        "italic" => {
            attrs.set_italic(true);
        }
        "underline" => {
            attrs.set_underline(Underline::Single);
        }
        "inverse" => {
            attrs.set_reverse(true);
        }
        "dim" => {
            attrs.set_intensity(Intensity::Half);
        }
        _ => {
            return false;
        }
    }

    true
}

fn set_color(is_bg: bool, attrs: &mut CellAttributes, color: impl Into<ColorSpec>) {
    if is_bg {
        attrs.set_background(color.into());
    } else {
        attrs.set_foreground(color.into());
    }
}

fn ansi_color(name: &str) -> Option<AnsiColor> {
    Some(match name {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Maroon,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Olive,
        "blue" => AnsiColor::Navy,
        "magenta" => AnsiColor::Purple,
        "cyan" => AnsiColor::Teal,
        "white" => AnsiColor::Silver,
        "brightblack" => AnsiColor::Grey,
        "brightred" => AnsiColor::Red,
        "brightgreen" => AnsiColor::Lime,
        "brightyellow" => AnsiColor::Yellow,
        "brightblue" => AnsiColor::Blue,
        "brightmagenta" => AnsiColor::Fuchsia,
        "brightcyan" => AnsiColor::Aqua,
        "brightwhite" => AnsiColor::White,
        _ => None?,
    })
}

#[cfg(test)]
mod test {
    use termwiz::caps::Capabilities;
    use termwiz::caps::ColorLevel::*;
    use termwiz::caps::ProbeHints;
    use termwiz::render::terminfo::TerminfoRenderer;
    use termwiz::surface::Change;

    use super::*;

    macro_rules! assert_spec {
        ($level:ident, $specs:tt, $want:expr $(,)?) => {
            let bytes = specs_to_bytes($level, $specs);
            assert_eq!(bytes, $want, $specs);
        };
    }

    #[test]
    fn test_eval_style() {
        // refer to https://en.wikipedia.org/wiki/ANSI_escape_code

        assert_spec!(Sixteen, "green", b"\x1B[32m");
        assert_spec!(Sixteen, "green_background", b"\x1B[42m");
        assert_spec!(
            Sixteen,
            "red blue_background italic underline inverse dim",
            b"\x1B[0m\x1B[2m\x1B[4m\x1B[7m\x1B[3m\x1B[31m\x1B[44m",
        );

        assert_spec!(Sixteen, "color231", b"");
        assert_spec!(Sixteen, "color231:red", b"\x1B[31m");
        assert_spec!(Sixteen, "color231:red:green", b"\x1B[31m");
        assert_spec!(TwoFiftySix, "color231:red:green", b"\x1B[38:5:231m");
        assert_spec!(
            TwoFiftySix,
            "color231_background:red:green",
            b"\x1B[48:5:231m"
        );

        assert_spec!(Sixteen, "bold+#FFF:green", b"\x1B[32m");
        assert_spec!(TwoFiftySix, "bold+#FFF:green", b"\x1B[32m");
        assert_spec!(
            TrueColor,
            "bold+#FFF:green",
            b"\x1B[0m\x1B[1m\x1B[38:2::255:255:255m",
        );
    }

    #[test]
    fn test_render_bytes() {
        let mut styler = Styler::from_level(TwoFiftySix).unwrap();

        assert_eq!(
            styler.render_bytes("red", "hello\nthere\n").unwrap(),
            b"\x1B[31mhello\x1B[39m\n\x1B[31mthere\x1B[39m\n"
        );
    }

    fn specs_to_bytes(level: ColorLevel, specs: &str) -> Vec<u8> {
        let cell_attrs = eval_style(level, specs);
        let mut buf: Vec<u8> = Vec::new();
        let hints = ProbeHints::default()
            .color_level(Some(level))
            // Use a consistent (and non-existent) terminal to avoid differences in tests.
            .term(Some("fake-term".to_string()));
        let caps = Capabilities::new_with_hints(hints).unwrap();
        let mut renderer = TerminfoRenderer::new(caps);
        let mut tty = DumbTty { w: &mut buf };
        renderer
            .render_to(&[Change::AllAttributes(cell_attrs)], &mut tty)
            .unwrap();

        buf
    }
}
