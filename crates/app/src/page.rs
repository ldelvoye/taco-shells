//! The stack of pages drawn in the middle in place of the terminal, and how each move changes it.

/// A page drawn in the middle in place of the terminal; the window keeps a stack of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Settings,
    Shortcuts,
}

/// A way of moving through the pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageMove {
    /// Opens Settings, or closes every page when one is open.
    ToggleSettings,
    /// Opens the shortcuts page over Settings, or closes every page when it is on top.
    ToggleShortcuts,
    /// Puts the shortcuts page on top, with Settings under it.
    OpenShortcuts,
    /// Closes the page on top.
    Back,
    /// Closes every page.
    CloseAll,
}

/// The pages open after a move.
pub fn pages_after(pages: &[Page], page_move: PageMove) -> Vec<Page> {
    let mut after = pages.to_vec();
    let top = pages.last().copied();
    match page_move {
        PageMove::ToggleSettings => {
            if pages.is_empty() {
                after.push(Page::Settings);
            } else {
                after.clear();
            }
        }
        PageMove::ToggleShortcuts if top == Some(Page::Shortcuts) => after.clear(),
        PageMove::ToggleShortcuts | PageMove::OpenShortcuts => {
            if top == Some(Page::Shortcuts) {
                return after;
            }
            if pages.is_empty() {
                after.push(Page::Settings);
            }
            after.push(Page::Shortcuts);
        }
        PageMove::Back => {
            after.pop();
        }
        PageMove::CloseAll => after.clear(),
    }
    after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_stack_after_each_move() {
        use Page::{Settings, Shortcuts};
        let cases: [(&[Page], PageMove, &[Page]); 9] = [
            (&[], PageMove::ToggleSettings, &[Settings]),
            (&[Settings, Shortcuts], PageMove::ToggleSettings, &[]),
            (&[], PageMove::ToggleShortcuts, &[Settings, Shortcuts]),
            (
                &[Settings],
                PageMove::ToggleShortcuts,
                &[Settings, Shortcuts],
            ),
            (&[Settings, Shortcuts], PageMove::ToggleShortcuts, &[]),
            (
                &[Settings, Shortcuts],
                PageMove::OpenShortcuts,
                &[Settings, Shortcuts],
            ),
            (&[Settings, Shortcuts], PageMove::Back, &[Settings]),
            (&[Settings], PageMove::Back, &[]),
            (&[Settings, Shortcuts], PageMove::CloseAll, &[]),
        ];
        for (before, page_move, expected) in cases {
            let after = pages_after(before, page_move);
            assert_eq!(after, expected, "{before:?} {page_move:?}");
        }
    }
}
