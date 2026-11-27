use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};
use taco_services::IS_STABLE;

pub const MARK: &str = "brand/mark.svg";
pub const TERMINALS_ICON: &str = "icons/terminals.svg";
pub const AGENT_ACTIVITY_ICON: &str = "icons/agent-activity.svg";
pub const SETTINGS_ICON: &str = "icons/settings.svg";
pub const CLOSE_ICON: &str = "icons/close.svg";
pub const PLUS_ICON: &str = "icons/plus.svg";
pub const TRASH_ICON: &str = "icons/trash.svg";
pub const WORKING_ICON: &str = "icons/working.svg";
pub const DONE_ICON: &str = "icons/done.svg";
/// A dot 1/2.8 the width of its box, so it can grow to fill the box without leaving it.
pub const RIPPLE_ICON: &str = "icons/ripple.svg";
pub const TEXT_ICON: &str = "icons/text.svg";
pub const DOCUMENT_ICON: &str = "icons/document.svg";
pub const IMAGE_ICON: &str = "icons/image.svg";
pub const LINK_ICON: &str = "icons/link.svg";
pub const PULL_REQUEST_ICON: &str = "icons/pull-request.svg";

const FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/Figtree-Regular.ttf"),
    include_bytes!("../assets/fonts/Figtree-Medium.ttf"),
    include_bytes!("../assets/fonts/Figtree-SemiBold.ttf"),
];

const FILES: [(&str, &[u8]); 15] = [
    (
        MARK,
        include_bytes!("../../../branding/icons/taco-shells-mark.svg"),
    ),
    (
        TERMINALS_ICON,
        include_bytes!("../assets/icons/terminals.svg"),
    ),
    (
        AGENT_ACTIVITY_ICON,
        include_bytes!("../assets/icons/agent-activity.svg"),
    ),
    (
        SETTINGS_ICON,
        include_bytes!("../assets/icons/settings.svg"),
    ),
    (CLOSE_ICON, include_bytes!("../assets/icons/close.svg")),
    (PLUS_ICON, include_bytes!("../assets/icons/plus.svg")),
    (TRASH_ICON, include_bytes!("../assets/icons/trash.svg")),
    (WORKING_ICON, include_bytes!("../assets/icons/working.svg")),
    (DONE_ICON, include_bytes!("../assets/icons/done.svg")),
    (RIPPLE_ICON, include_bytes!("../assets/icons/ripple.svg")),
    (TEXT_ICON, include_bytes!("../assets/icons/text.svg")),
    (
        DOCUMENT_ICON,
        include_bytes!("../assets/icons/document.svg"),
    ),
    (IMAGE_ICON, include_bytes!("../assets/icons/image.svg")),
    (LINK_ICON, include_bytes!("../assets/icons/link.svg")),
    (
        PULL_REQUEST_ICON,
        include_bytes!("../assets/icons/pull-request.svg"),
    ),
];

// The development build's mark: the stable mark's colors, turned round the hue wheel as the dev Dock icon is.
const DEVELOPMENT_COLORS: [(&str, &str); 5] = [
    ("#5E8C28", "#6B77D1"),
    ("#7FB239", "#879BFF"),
    ("#C81E5B", "#006D00"),
    ("#F0902F", "#1FBED4"),
    ("#FFC24A", "#57DFFF"),
];

/// The files the window draws from, compiled into the app.
pub struct Assets {
    development_mark: bool,
}

impl Assets {
    pub fn for_this_build() -> Self {
        Self {
            development_mark: !IS_STABLE,
        }
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let found = FILES.iter().find(|(name, _)| *name == path);
        let Some((_, bytes)) = found else {
            return Ok(None);
        };
        if path == MARK && self.development_mark {
            let turned = development_mark(bytes);
            return Ok(Some(Cow::Owned(turned)));
        }
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let under = FILES.iter().filter(|(name, _)| name.starts_with(path));
        let names = under.map(|(name, _)| SharedString::from(*name));
        let listed = names.collect();
        Ok(listed)
    }
}

pub fn load_fonts(cx: &App) -> Result<()> {
    let borrowed = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes));
    let fonts = borrowed.collect();
    cx.text_system().add_fonts(fonts)
}

fn development_mark(bytes: &[u8]) -> Vec<u8> {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for (stable, development) in DEVELOPMENT_COLORS {
        text = text.replace(stable, development);
    }
    text.into_bytes()
}
