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
        token_start = after_character_at(before, space);
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
        scheme_start = after_character_at(before_mark, boundary);
    }
    let scheme = &token[scheme_start..mark];
    if scheme.is_empty() {
        return None;
    }
    let from_scheme = &token[scheme_start..];
    let url = without_trailing_punctuation(from_scheme);
    let nothing_after_the_scheme = url.len() <= scheme.len() + SCHEME_MARK.len();
    if nothing_after_the_scheme {
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

/// The byte just past the character starting at `index`.
fn after_character_at(text: &str, index: usize) -> usize {
    let mut rest = text[index..].chars();
    let mut length = 1;
    if let Some(character) = rest.next() {
        length = character.len_utf8();
    }
    index + length
}

/// `url` without the punctuation of the prose around it. A closing bracket the URL opened itself, as in `.../Foo_(bar)`, stays.
fn without_trailing_punctuation(url: &str) -> &str {
    let mut kept = url;
    while let Some(last) = kept.chars().last() {
        if !TRAILING.contains(&last) {
            break;
        }
        let opening = opening_of(last);
        if let Some(open) = opening {
            let opened = kept.matches(open).count();
            let closed = kept.matches(last).count();
            if opened >= closed {
                break;
            }
        }
        let end = kept.len() - last.len_utf8();
        kept = &kept[..end];
    }
    kept
}

fn opening_of(closing: char) -> Option<char> {
    match closing {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// The row as one line of text, a space for each blank cell, with where each column's text starts.
/// A wide character's second column starts where its first does.
fn line_of(row: &Row) -> (String, Vec<usize>) {
    let mut line = String::new();
    let mut starts = Vec::with_capacity(row.cells.len());
    let mut last_start = 0;
    for cell in &row.cells {
        if cell.width == Width::SpacerTail {
            starts.push(last_start);
            continue;
        }
        last_start = line.len();
        starts.push(last_start);
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
        let link = Some("https://example.com/a?b=1");
        let row = first_row("see (https://example.com/a?b=1), then 日 ok");
        assert_eq!(url_at(&row, 5).as_deref(), link);
        assert_eq!(url_at(&row, 29).as_deref(), link);
        // The bracket and comma around it are not part of it, and nor is plain text.
        assert_eq!(url_at(&row, 4), None);
        assert_eq!(url_at(&row, 30), None);
        assert_eq!(url_at(&row, 0), None);
        assert_eq!(url_at(&first_row("http:// alone"), 2), None);

        // A bracket the URL opened itself is part of it.
        let wiki = first_row("https://en.wikipedia.org/wiki/Foo_(bar).");
        assert_eq!(
            url_at(&wiki, 3).as_deref(),
            Some("https://en.wikipedia.org/wiki/Foo_(bar)")
        );
    }

    #[test]
    fn wide_and_multi_byte_text_around_a_url_lands_on_the_right_cells() {
        // Columns after a wide character still land on the right text.
        let wide = first_row("日本 http://x.y/z");
        assert_eq!(url_at(&wide, 6).as_deref(), Some("http://x.y/z"));
        // Text written right before the scheme, and an ideographic space, are not part of it.
        let joined = first_row("見てhttps://x.y/a");
        assert_eq!(url_at(&joined, 6).as_deref(), Some("https://x.y/a"));
        let ideographic = first_row("a\u{3000}https://x.y/b");
        assert_eq!(url_at(&ideographic, 5).as_deref(), Some("https://x.y/b"));
        // Either half of a wide character at the end of a URL is part of it.
        let ending = first_row("http://a.jp/日 x");
        assert_eq!(url_at(&ending, 13).as_deref(), Some("http://a.jp/日"));
    }
}
