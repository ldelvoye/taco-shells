//! A URL written in a row's text, found under the cell clicked.

use taco_ghostty::render::{Row, Width};

const SCHEME_MARK: &str = "://";
/// Characters that end a URL written in prose, such as the full stop after it.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"', '>'];

/// The URL written across `column` in `row`, such as `https://example.com/path`.
pub fn url_at(row: &Row, column: u16) -> Option<String> {
    let (line, starts) = line_of(row);
    let clicked = *starts.get(usize::from(column))?;
    let before = &line[..clicked];
    let mut token_start = 0;
    if let Some(space) = before.rfind(char::is_whitespace) {
        token_start = space + 1;
    }
    let after = &line[clicked..];
    let mut token_end = line.len();
    if let Some(space) = after.find(char::is_whitespace) {
        token_end = clicked + space;
    }
    let token = &line[token_start..token_end];

    let mark = token.find(SCHEME_MARK)?;
    let before_mark = &token[..mark];
    let mut scheme_start = 0;
    if let Some(boundary) = before_mark.rfind(|character: char| !is_scheme_character(character)) {
        scheme_start = boundary + 1;
    }
    let scheme = &token[scheme_start..mark];
    if scheme.is_empty() {
        return None;
    }
    let from_scheme = &token[scheme_start..];
    let url = from_scheme.trim_end_matches(TRAILING);
    if url.len() <= scheme.len() + SCHEME_MARK.len() {
        return None;
    }
    let url_start = token_start + scheme_start;
    let url_end = url_start + url.len();
    if clicked < url_start || clicked >= url_end {
        return None;
    }
    Some(url.to_string())
}

fn is_scheme_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '+' || character == '-'
}

/// The row as one line of text, a space for each blank cell, with where each column's text starts.
fn line_of(row: &Row) -> (String, Vec<usize>) {
    let mut line = String::new();
    let mut starts = Vec::with_capacity(row.cells.len());
    for cell in &row.cells {
        starts.push(line.len());
        if cell.width == Width::SpacerTail {
            continue;
        }
        let text = row.cell_text(cell);
        if text.is_empty() {
            line.push(' ');
        } else {
            line.push_str(text);
        }
    }
    (line, starts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_ghostty::Terminal;
    use taco_ghostty::render::RenderState;
    use taco_protocol::TerminalSize;

    fn first_row(text: &str) -> Row {
        let mut terminal = Terminal::new(TerminalSize { cols: 60, rows: 1 }).unwrap();
        terminal.write(text.as_bytes());
        let mut state = RenderState::new().unwrap();
        let mut frame = state.update(&terminal).unwrap();
        let mut row = Row::new();
        frame.next_row(&mut row, false).unwrap();
        row
    }

    #[test]
    fn a_url_is_found_under_any_of_its_cells_without_the_prose_around_it() {
        let row = first_row("see (https://example.com/a?b=1), then 日 ok");
        assert_eq!(
            url_at(&row, 5).as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(
            url_at(&row, 20).as_deref(),
            Some("https://example.com/a?b=1")
        );
        // The bracket and comma around it are not part of it, and nor is plain text.
        assert_eq!(url_at(&row, 4), None);
        assert_eq!(url_at(&row, 31), None);
        assert_eq!(url_at(&row, 0), None);

        // Columns after a wide character still land on the right text.
        let wide = first_row("日本 http://x.y/z");
        assert_eq!(url_at(&wide, 6).as_deref(), Some("http://x.y/z"));
    }
}
