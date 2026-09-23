use console::{Style, colors_enabled, colors_enabled_stderr};
use indicatif::ProgressBar;
use std::fmt::Display;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum Tone {
    Heading,
    Accent,
    Success,
    Warning,
    Error,
    Info,
}

pub fn paint(message: impl Display, tone: Tone) -> String {
    paint_with_colors(message, tone, colors_enabled())
}

pub fn paint_stderr(message: impl Display, tone: Tone) -> String {
    paint_with_colors_for_stderr(message, tone, colors_enabled_stderr())
}

fn paint_with_colors(message: impl Display, tone: Tone, enabled: bool) -> String {
    style_for(tone)
        .for_stdout()
        .force_styling(enabled)
        .apply_to(message)
        .to_string()
}

fn paint_with_colors_for_stderr(message: impl Display, tone: Tone, enabled: bool) -> String {
    style_for(tone)
        .for_stderr()
        .force_styling(enabled)
        .apply_to(message)
        .to_string()
}

fn style_for(tone: Tone) -> Style {
    match tone {
        Tone::Heading => Style::new().cyan().bold(),
        Tone::Accent => Style::new().blue().bold(),
        Tone::Success => Style::new().green(),
        Tone::Warning => Style::new().yellow(),
        Tone::Error => Style::new().red().bold(),
        Tone::Info => Style::new().cyan(),
    }
}

pub fn run_with_spinner<T>(message: &'static str, operation: impl FnOnce() -> T) -> T {
    let spinner = ProgressBar::new_spinner();
    spinner.set_message(message);
    spinner.enable_steady_tick(Duration::from_millis(90));
    let result = operation();
    spinner.finish_and_clear();
    result
}

#[cfg(test)]
mod tests {
    use super::{Tone, paint_with_colors, paint_with_colors_for_stderr, run_with_spinner};
    use console::strip_ansi_codes;

    #[test]
    fn styling_can_be_disabled_for_plain_terminal_output() {
        assert_eq!(paint_with_colors("Done", Tone::Success, false), "Done");
    }

    #[test]
    fn semantic_styles_preserve_text_and_can_emit_terminal_colors() {
        let rendered = paint_with_colors("Build completed", Tone::Success, true);

        assert!(rendered.contains("\u{1b}["));
        assert_eq!(strip_ansi_codes(&rendered), "Build completed");
    }

    #[test]
    fn stderr_styles_are_applied_to_the_stderr_stream() {
        let rendered = paint_with_colors_for_stderr("Failure", Tone::Error, true);

        assert!(rendered.contains("\u{1b}["));
        assert_eq!(strip_ansi_codes(&rendered), "Failure");
    }

    #[test]
    fn spinner_wrapper_returns_the_compiler_result_unchanged() {
        let output = run_with_spinner("Compiling", || Ok::<_, &'static str>("compiled"));

        assert_eq!(output, Ok("compiled"));
    }
}
