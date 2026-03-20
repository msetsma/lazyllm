/// Convert markdown tables to box-drawing character tables.
///
/// Detects markdown table syntax and replaces it with Unicode box-drawing
/// characters for proper terminal rendering.
pub fn convert_tables(input: &str) -> String {
    let mut result = Vec::new();
    let lines: Vec<&str> = input.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Detect a table: at least a header row + separator row
        if let Some(table_end) = detect_table(&lines, i) {
            let table_lines = &lines[i..table_end];
            let rendered = render_table(table_lines);
            result.push(rendered);
            i = table_end;
        } else {
            result.push(lines[i].to_string());
            i += 1;
        }
    }

    result.join("\n")
}

/// Detect a markdown table starting at line `start`.
/// Returns the exclusive end index if a table is found.
fn detect_table(lines: &[&str], start: usize) -> Option<usize> {
    if start + 1 >= lines.len() {
        return None;
    }

    // First line must have pipes
    if !is_table_row(lines[start]) {
        return None;
    }

    // Second line must be a separator row (e.g., |---|---|)
    if !is_separator_row(lines[start + 1]) {
        return None;
    }

    // Collect remaining data rows
    let mut end = start + 2;
    while end < lines.len() && is_table_row(lines[end]) {
        end += 1;
    }

    // Need at least header + separator + 1 data row (but header + separator is also valid)
    if end > start + 1 {
        Some(end)
    } else {
        None
    }
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !is_separator_row(trimmed)
}

fn is_separator_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    // After removing pipes, colons, dashes, and spaces, should be empty
    trimmed
        .chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
        && trimmed.contains('-')
}

/// Parse cells from a table row.
fn parse_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    trimmed.split('|').map(|s| s.trim().to_string()).collect()
}

/// Parse alignment from a separator row.
fn parse_alignments(line: &str) -> Vec<Alignment> {
    parse_cells(line)
        .iter()
        .map(|cell| {
            let trimmed = cell.trim();
            let left = trimmed.starts_with(':');
            let right = trimmed.ends_with(':');
            match (left, right) {
                (true, true) => Alignment::Center,
                (false, true) => Alignment::Right,
                _ => Alignment::Left,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum Alignment {
    Left,
    Center,
    Right,
}

/// Render a markdown table using box-drawing characters.
fn render_table(lines: &[&str]) -> String {
    if lines.len() < 2 {
        return lines.join("\n");
    }

    let header_cells = parse_cells(lines[0]);
    let alignments = parse_alignments(lines[1]);
    let data_rows: Vec<Vec<String>> = lines[2..]
        .iter()
        .map(|l| parse_cells(l))
        .collect();

    let num_cols = header_cells.len();

    // Calculate column widths
    let mut widths: Vec<usize> = header_cells.iter().map(|c| c.len()).collect();
    for row in &data_rows {
        for (j, cell) in row.iter().enumerate() {
            if j < widths.len() {
                widths[j] = widths[j].max(cell.len());
            }
        }
    }

    // Ensure minimum width of 3
    for w in &mut widths {
        *w = (*w).max(3);
    }

    let mut out = String::new();

    // Top border
    out.push_str(&horizontal_line(&widths, '┌', '┬', '┐'));
    out.push('\n');

    // Header row
    out.push_str(&format_row(&header_cells, &widths, &alignments, num_cols));
    out.push('\n');

    // Header separator
    out.push_str(&horizontal_line(&widths, '├', '┼', '┤'));
    out.push('\n');

    // Data rows
    for row in &data_rows {
        out.push_str(&format_row(row, &widths, &alignments, num_cols));
        out.push('\n');
    }

    // Bottom border
    out.push_str(&horizontal_line(&widths, '└', '┴', '┘'));

    out
}

fn horizontal_line(widths: &[usize], left: char, mid: char, right: char) -> String {
    let mut line = String::new();
    line.push(left);
    for (i, &w) in widths.iter().enumerate() {
        for _ in 0..w + 2 {
            line.push('─');
        }
        if i < widths.len() - 1 {
            line.push(mid);
        }
    }
    line.push(right);
    line
}

fn format_row(cells: &[String], widths: &[usize], alignments: &[Alignment], num_cols: usize) -> String {
    let mut row = String::new();
    row.push('│');
    for j in 0..num_cols {
        let cell = cells.get(j).map(|s| s.as_str()).unwrap_or("");
        let width = widths.get(j).copied().unwrap_or(3);
        let align = alignments.get(j).copied().unwrap_or(Alignment::Left);
        let formatted = align_cell(cell, width, align);
        row.push(' ');
        row.push_str(&formatted);
        row.push(' ');
        row.push('│');
    }
    row
}

fn align_cell(content: &str, width: usize, alignment: Alignment) -> String {
    let content_len = content.len();
    if content_len >= width {
        return content[..width].to_string();
    }
    let padding = width - content_len;
    match alignment {
        Alignment::Left => format!("{content}{}", " ".repeat(padding)),
        Alignment::Right => format!("{}{content}", " ".repeat(padding)),
        Alignment::Center => {
            let left_pad = padding / 2;
            let right_pad = padding - left_pad;
            format!(
                "{}{content}{}",
                " ".repeat(left_pad),
                " ".repeat(right_pad)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures a standard markdown table renders with box-drawing characters and cell content.
    #[test]
    fn basic_table() {
        let input = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let result = convert_tables(input);
        assert!(result.contains('┌'));
        assert!(result.contains('│'));
        assert!(result.contains('┘'));
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
    }

    /// Verifies text without table syntax passes through unchanged.
    #[test]
    fn no_table_passthrough() {
        let input = "Just some text\nwith multiple lines";
        assert_eq!(convert_tables(input), input);
    }

    /// Ensures tables with alignment markers (:---:, ---:, :---) render all cell content.
    #[test]
    fn table_with_alignment() {
        let input = "| Left | Center | Right |\n|:-----|:------:|------:|\n| a | b | c |";
        let result = convert_tables(input);
        assert!(result.contains("Left"));
        assert!(result.contains("Center"));
        assert!(result.contains("Right"));
    }

    /// Verifies a table embedded in surrounding text preserves both the table and surrounding content.
    #[test]
    fn table_surrounded_by_text() {
        let input = "Before table\n\n| H1 | H2 |\n|----|----|\n| a | b |\n\nAfter table";
        let result = convert_tables(input);
        assert!(result.contains("Before table"));
        assert!(result.contains("After table"));
        assert!(result.contains('┌'));
    }

    /// Ensures separator row detection correctly identifies --- patterns and rejects data rows.
    #[test]
    fn is_separator_row_detection() {
        assert!(is_separator_row("|---|---|"));
        assert!(is_separator_row("| --- | --- |"));
        assert!(is_separator_row("|:---:|---:|"));
        assert!(!is_separator_row("| Name | Age |"));
        assert!(!is_separator_row("no pipes here"));
    }

    /// Verifies parse_cells splits pipe-delimited rows into trimmed cell values.
    #[test]
    fn parse_cells_basic() {
        let cells = parse_cells("| Name | Age | City |");
        assert_eq!(cells, vec!["Name", "Age", "City"]);
    }

    /// Ensures parse_cells handles rows without leading/trailing pipes.
    #[test]
    fn parse_cells_no_outer_pipes() {
        let cells = parse_cells("Name | Age");
        assert_eq!(cells, vec!["Name", "Age"]);
    }

    /// Verifies a table with only a header and separator (no data rows) still renders.
    #[test]
    fn header_only_table() {
        // header + separator with no data rows should still render
        let input = "| Col1 | Col2 |\n|------|------|";
        let result = convert_tables(input);
        assert!(result.contains('┌'));
        assert!(result.contains("Col1"));
    }

    /// Ensures parse_alignments correctly identifies Center, Right, and Left alignment markers.
    #[test]
    fn alignment_parsing() {
        let aligns = parse_alignments("|:---:|---:|:---|");
        assert!(matches!(aligns[0], Alignment::Center));
        assert!(matches!(aligns[1], Alignment::Right));
        assert!(matches!(aligns[2], Alignment::Left));
    }

    /// Verifies single-column tables render correctly with box-drawing borders.
    #[test]
    fn single_column_table() {
        let input = "| Item |\n|------|\n| one |\n| two |";
        let result = convert_tables(input);
        assert!(result.contains("one"));
        assert!(result.contains("two"));
        assert!(result.contains('│'));
    }
}
