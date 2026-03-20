pub mod latex;
pub mod tables;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Render a markdown string into ratatui `Text` with syntax highlighting.
///
/// Uses `tui_markdown` for full CommonMark parsing with code block
/// syntax highlighting via syntect.
pub fn render_markdown(input: &str) -> Text<'_> {
    if input.is_empty() {
        return Text::default();
    }
    tui_markdown::from_str(input)
}

/// Options controlling which preprocessing steps to apply.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub latex: bool,
    pub tables: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            latex: true,
            tables: true,
        }
    }
}

/// Preprocess and render markdown, returning fully owned lines.
///
/// Applies LaTeX→Unicode and table→box-drawing conversion (based on options),
/// then renders the result through `tui_markdown`. All strings are owned so
/// the result has a `'static` lifetime.
pub fn render_markdown_preprocessed(input: &str, opts: RenderOptions) -> Vec<Line<'static>> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut processed = input.to_string();
    if opts.latex {
        processed = latex::convert_latex(&processed);
    }
    if opts.tables {
        processed = tables::convert_tables(&processed);
    }
    let rendered = tui_markdown::from_str(&processed);

    // Deep-clone lines to own all string data
    rendered
        .lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.to_string(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect()
}

/// Render plain text (no markdown parsing) into ratatui `Text`.
///
/// Used for user messages where markdown parsing isn't needed.
pub fn render_plain(input: &str) -> Text<'static> {
    let lines: Vec<Line<'static>> = input
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    Text::from(lines)
}

/// Create a styled role label line (e.g., "You:", "Assistant:").
pub fn role_label(label: &str, color: Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("{label}:"),
        Style::default()
            .fg(color)
            .add_modifier(Modifier::BOLD),
    )])
}

/// A separator line between messages, using the given colour.
pub fn separator(color: Color) -> Line<'static> {
    Line::from(Span::styled(
        "\u{2500}".repeat(40),
        Style::default().fg(color),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures empty input produces no rendered lines.
    #[test]
    fn render_markdown_empty_returns_default() {
        let text = render_markdown("");
        assert!(text.lines.is_empty());
    }

    /// Verifies plain text renders without any markdown styling applied.
    #[test]
    fn render_markdown_plain_text() {
        let text = render_markdown("Hello world");
        assert!(!text.lines.is_empty());
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("Hello world"));
    }

    /// Ensures **bold** markdown syntax applies the BOLD modifier to the span.
    #[test]
    fn render_markdown_bold_text() {
        let text = render_markdown("**bold**");
        assert!(!text.lines.is_empty());
        let has_bold = text.lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content.contains("bold")
                    && span.style.add_modifier.contains(Modifier::BOLD)
            })
        });
        assert!(has_bold, "Expected bold styling on 'bold'");
    }

    /// Verifies fenced code blocks render their content in the output.
    #[test]
    fn render_markdown_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let text = render_markdown(md);
        assert!(!text.lines.is_empty());
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            content.contains("fn") || content.contains("main"),
            "Expected code content, got: {content}"
        );
    }

    /// Ensures inline `code` renders the code text in the output.
    #[test]
    fn render_markdown_inline_code() {
        let text = render_markdown("Use `cargo build` to compile");
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("cargo build"));
    }

    /// Verifies heading levels (# and ##) render their text content.
    #[test]
    fn render_markdown_headings() {
        let text = render_markdown("# Heading 1\n## Heading 2");
        assert!(!text.lines.is_empty());
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("Heading 1"));
        assert!(content.contains("Heading 2"));
    }

    /// Ensures bullet list items render all list text.
    #[test]
    fn render_markdown_bullet_list() {
        let text = render_markdown("- item 1\n- item 2\n- item 3");
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("item 1"));
        assert!(content.contains("item 2"));
        assert!(content.contains("item 3"));
    }

    /// Ensures numbered list items render their content.
    #[test]
    fn render_markdown_numbered_list() {
        let text = render_markdown("1. first\n2. second\n3. third");
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("first"));
        assert!(content.contains("second"));
    }

    /// Verifies markdown links render the link text in the output.
    #[test]
    fn render_markdown_links() {
        let text = render_markdown("[click here](https://example.com)");
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("click here"));
    }

    /// Ensures render_plain splits text into lines without any styling.
    #[test]
    fn render_plain_basic() {
        let text = render_plain("line1\nline2");
        assert_eq!(text.lines.len(), 2);
    }

    /// Ensures render_plain with empty input produces no lines.
    #[test]
    fn render_plain_empty() {
        let text = render_plain("");
        assert!(text.lines.is_empty());
    }

    /// Verifies role_label creates a bold, colored span with the role name.
    #[test]
    fn role_label_creates_styled_line() {
        let line = role_label("You", Color::Green);
        assert_eq!(line.spans.len(), 1);
        assert!(line.spans[0].content.contains("You"));
        assert_eq!(line.spans[0].style.fg, Some(Color::Green));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    /// Ensures separator creates a line with horizontal box-drawing characters.
    #[test]
    fn separator_creates_line() {
        let line = separator(Color::DarkGray);
        assert!(!line.spans.is_empty());
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains("\u{2500}"));
    }

    /// Verifies the separator line uses the specified foreground color.
    #[test]
    fn separator_uses_given_color() {
        let line = separator(Color::Red);
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
    }

    /// Ensures multi-paragraph markdown renders both paragraphs in the output.
    #[test]
    fn render_markdown_multiline_preserves_structure() {
        let md = "First paragraph.\n\nSecond paragraph.";
        let text = render_markdown(md);
        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("First paragraph"));
        assert!(content.contains("Second paragraph"));
    }

    /// Verifies complex markdown with headings, formatting, code, and lists all render.
    #[test]
    fn render_markdown_mixed_content() {
        let md = r#"# Title

Some **bold** and *italic* text.

```python
print("hello")
```

- item 1
- item 2
"#;
        let text = render_markdown(md);
        assert!(!text.lines.is_empty());

        let content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("Title"));
        assert!(content.contains("bold"));
        assert!(content.contains("print"));
        assert!(content.contains("item 1"));
    }
}
