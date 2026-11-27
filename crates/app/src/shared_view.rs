//! A shared thing opened in the middle, in place of the terminal: what it shows, read off the main thread, and its drawing.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, StatefulInteractiveElement, Styled, StyledImage, div, img, px,
};
use taco_protocol::{SharedThing, TerminalId};

use crate::look::{FONT_FAMILY, Look, SMALL_TEXT_SIZE, TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::window::WindowView;

const MAX_TEXT_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const SNIFFED_BYTES: usize = 8 * 1024;
/// A text past this is cut where it is drawn; laying out a whole megabyte would stall every frame.
const SHOWN_CHARACTERS: usize = 100_000;
const HTTPS: &str = "https://";
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
const BODY_FONT: &str = "Menlo";
const BODY_TEXT_SIZE: f32 = 12.;
const BODY_PADDING: f32 = 20.;
const HEADER_GAP: f32 = 8.;
const SCROLL_ID: &str = "shared-view-scroll";
const BACK_ID: &str = "shared-view-back";
const REVEAL_ID: &str = "shared-view-reveal";
const BACK_LABEL: &str = "‹ Terminal";
const REVEAL_LABEL: &str = "Show in Finder";
const LOADING: &str = "Loading…";
const NOT_HTTPS: &str = "Only https links open from here.";
const NO_LONGER_KEPT: &str = "This is no longer kept.";
const NOT_REGULAR: &str = "Not a regular file, so it is not read.";
const TOO_LARGE_TEXT: &str = "Over 1 MB, so it is not shown here.";
const TOO_LARGE_IMAGE: &str = "Over 20 MB, so it is not drawn here.";
const NOT_TEXT: &str = "Not text, so it is not shown here.";

/// A shared thing shown in the middle, for the terminal whose log it came from.
pub struct OpenedThing {
    pub terminal: TerminalId,
    /// As it was when clicked, so it stays readable after it leaves Shared.
    pub thing: SharedThing,
    pub content: Content,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Loading,
    Text(String),
    Image(PathBuf),
    /// Shown only by its path, with a way to find it in the Finder.
    Reveal {
        path: PathBuf,
        reason: String,
    },
    /// Not there, or no longer kept.
    Gone(String),
    /// An address that is not opened from here.
    Address(String),
}

pub fn is_image(path: &str) -> bool {
    let Some((_, extension)) = path.rsplit_once('.') else {
        return false;
    };
    let lowered = extension.to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&lowered.as_str())
}

/// Whether an address may be handed to the browser: `https://` in any case, then a host.
pub fn is_https(url: &str) -> bool {
    let Some(scheme) = url.get(..HTTPS.len()) else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case(HTTPS) {
        return false;
    }
    let rest = &url[HTTPS.len()..];
    let mut host = rest;
    if let Some(host_end) = rest.find(['/', '?', '#']) {
        host = &rest[..host_end];
    }
    !host.is_empty()
}

/// What a shared file shows as. Only a regular file is read, and no more than the text it would show.
pub fn file_content(path: &str) -> Content {
    let candidate = Path::new(path);
    let not_found = Content::Gone(format!("Not found at {path}"));
    if !candidate.is_absolute() {
        return not_found;
    }
    let Ok(metadata) = fs::metadata(candidate) else {
        return not_found;
    };
    let shown_path = candidate.to_path_buf();
    if !metadata.is_file() {
        return reveal(shown_path, NOT_REGULAR);
    }
    if is_image(path) {
        if metadata.len() > MAX_IMAGE_BYTES {
            return reveal(shown_path, TOO_LARGE_IMAGE);
        }
        return Content::Image(shown_path);
    }
    if metadata.len() > MAX_TEXT_BYTES {
        return reveal(shown_path, TOO_LARGE_TEXT);
    }
    let Ok(file) = File::open(candidate) else {
        return not_found;
    };
    let mut bytes = Vec::new();
    let mut limited = file.take(MAX_TEXT_BYTES);
    if limited.read_to_end(&mut bytes).is_err() {
        return not_found;
    }
    let sniffed_length = bytes.len().min(SNIFFED_BYTES);
    let sniffed = &bytes[..sniffed_length];
    if sniffed.contains(&0) {
        return reveal(shown_path, NOT_TEXT);
    }
    match String::from_utf8(bytes) {
        Ok(text) => Content::Text(text),
        Err(_) => reveal(shown_path, NOT_TEXT),
    }
}

/// A text body the workspace sent, or none when the thing is gone.
pub fn body_content(body: Option<String>) -> Content {
    match body {
        Some(text) => Content::Text(text),
        None => Content::Gone(NO_LONGER_KEPT.to_string()),
    }
}

fn reveal(path: PathBuf, reason: &str) -> Content {
    let reason = reason.to_string();
    Content::Reveal { path, reason }
}

/// The text as drawn, and whether it was cut.
fn shown_text(text: &str) -> (String, bool) {
    let cut_at = text.char_indices().nth(SHOWN_CHARACTERS);
    match cut_at {
        Some((at, _)) => (text[..at].to_string(), true),
        None => (text.to_string(), false),
    }
}

pub fn shared_view(opened: &OpenedThing, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let header = header(&opened.thing, look, cx);
    let body = body(&opened.content, look, cx);
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(look.panels)
        .text_color(look.text)
        .font_family(FONT_FAMILY)
        .text_size(px(TEXT_SIZE))
        .child(header)
        .child(body)
}

fn header(thing: &SharedThing, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let veil = look.strong_veil;
    let back = div()
        .id(BACK_ID)
        .flex_none()
        .px(px(7.))
        .py(px(3.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(look.secondary_text)
        .hover(move |style| style.bg(veil))
        .child(BACK_LABEL)
        .on_click(cx.listener(|view, _, _, cx| view.close_opened(cx)));
    let kind = div()
        .flex_none()
        .text_size(px(SMALL_TEXT_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(look.secondary_text)
        .child(thing.kind.to_uppercase());
    let title = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .font_weight(FontWeight::SEMIBOLD)
        .child(thing.title.clone());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(HEADER_GAP))
        .h(px(TITLE_BAR_HEIGHT))
        .pl(px(8.))
        .pr(px(12.))
        .border_b_1()
        .border_color(look.hairline)
        .child(back)
        .child(kind)
        .child(title)
}

fn body(content: &Content, look: &Look, cx: &mut Context<WindowView>) -> AnyElement {
    let scroll = div()
        .id(SCROLL_ID)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(BODY_PADDING));
    match content {
        Content::Loading => scroll.child(quiet(LOADING, look)).into_any_element(),
        Content::Text(text) => {
            let (shown, was_cut) = shown_text(text);
            let words = div()
                .font_family(BODY_FONT)
                .text_size(px(BODY_TEXT_SIZE))
                .child(shown);
            let mut column = div().flex().flex_col().gap(px(10.)).child(words);
            if was_cut {
                let note = format!("Showing the first {SHOWN_CHARACTERS} characters.");
                column = column.child(quiet(&note, look));
            }
            scroll.child(column).into_any_element()
        }
        Content::Image(path) => {
            let picture = img(path.clone()).size_full().object_fit(ObjectFit::Contain);
            div()
                .flex_1()
                .min_h_0()
                .p(px(BODY_PADDING))
                .child(picture)
                .into_any_element()
        }
        Content::Reveal { path, reason } => {
            let place = path_line(&path.to_string_lossy());
            let why = quiet(reason, look);
            let button = reveal_button(path, look, cx);
            let column = div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(10.))
                .child(place)
                .child(why)
                .child(button);
            scroll.child(column).into_any_element()
        }
        Content::Gone(reason) => scroll.child(quiet(reason, look)).into_any_element(),
        Content::Address(url) => {
            let place = path_line(url);
            let why = quiet(NOT_HTTPS, look);
            let column = div().flex().flex_col().gap(px(10.)).child(place).child(why);
            scroll.child(column).into_any_element()
        }
    }
}

fn quiet(text: &str, look: &Look) -> Div {
    div()
        .text_color(look.secondary_text)
        .child(text.to_string())
}

fn path_line(text: &str) -> Div {
    div()
        .min_w_0()
        .font_family(BODY_FONT)
        .text_size(px(BODY_TEXT_SIZE))
        .child(text.to_string())
}

fn reveal_button(path: &Path, look: &Look, cx: &mut Context<WindowView>) -> impl IntoElement {
    let revealed = path.to_path_buf();
    div()
        .id(REVEAL_ID)
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .child(REVEAL_LABEL)
        .on_click(cx.listener(move |_, _, _, cx| cx.reveal_path(&revealed)))
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use super::*;

    #[test]
    fn an_image_is_known_by_its_extension_in_any_case() {
        assert!(is_image("/Users/me/view-check/logo-after.PNG"));
        assert!(!is_image("/Users/me/PLAN-sigkill.md"));
        assert!(!is_image("/Users/me/pictures.png/notes"));
    }

    #[test]
    fn only_an_https_address_with_a_host_goes_to_the_browser() {
        assert!(is_https("https://github.com/ldelvoye/taco-shells/pull/155"));
        assert!(is_https("HTTPS://claude.ai/artifact/x"));
        assert!(!is_https("http://example.com"));
        assert!(!is_https("https://"));
        assert!(!is_https("https:///etc/passwd"));
        assert!(!is_https("file:///Users/me/notes.md"));
        assert!(!is_https("javascript:alert(1)"));
    }

    #[test]
    fn a_shared_file_is_read_only_when_it_is_a_regular_text_file_small_enough_to_show() {
        let folder = std::env::temp_dir().join(format!("taco-shared-view-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let at = |name: &str| folder.join(name).to_string_lossy().into_owned();

        fs::write(at("plan.md"), "# Plan\n").unwrap();
        assert_eq!(
            file_content(&at("plan.md")),
            Content::Text("# Plan\n".to_string())
        );

        fs::write(at("binary.bin"), [b'a', 0, b'b']).unwrap();
        let binary = file_content(&at("binary.bin"));
        assert!(matches!(binary, Content::Reveal { .. }), "{binary:?}");

        let large = File::create(at("large.log")).unwrap();
        large.set_len(MAX_TEXT_BYTES + 1).unwrap();
        let too_large = file_content(&at("large.log"));
        assert!(matches!(too_large, Content::Reveal { .. }), "{too_large:?}");

        let picture = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(at("logo.PNG"))
            .unwrap();
        picture.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        let too_large_picture = file_content(&at("logo.PNG"));
        assert!(
            matches!(too_large_picture, Content::Reveal { .. }),
            "{too_large_picture:?}"
        );
        picture.set_len(10).unwrap();
        assert!(matches!(file_content(&at("logo.PNG")), Content::Image(_)));

        let not_regular = file_content(&folder.to_string_lossy());
        assert!(
            matches!(not_regular, Content::Reveal { .. }),
            "{not_regular:?}"
        );
        assert!(matches!(file_content(&at("missing.md")), Content::Gone(_)));
        assert!(matches!(file_content("notes/plan.md"), Content::Gone(_)));

        fs::remove_dir_all(&folder).unwrap();
    }
}
