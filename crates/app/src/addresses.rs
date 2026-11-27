//! Which addresses the window hands to the system to open.

const HTTP: Scheme = Scheme {
    prefix: "http://",
    has_authority: true,
};
const HTTPS: Scheme = Scheme {
    prefix: "https://",
    has_authority: true,
};
const MAILTO: Scheme = Scheme {
    prefix: "mailto:",
    has_authority: false,
};

/// What a link clicked in a terminal may open: the web and mail, never a file or an app's own scheme.
pub const TERMINAL_SCHEMES: [Scheme; 3] = [HTTP, HTTPS, MAILTO];
/// What a shared link may open: https only.
pub const SHARED_SCHEMES: [Scheme; 1] = [HTTPS];

#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    prefix: &'static str,
    /// Whether a host follows the prefix, which must then have no user part.
    has_authority: bool,
}

/// Whether an address may be handed to the system: an allowed scheme, a host with no user part, and no space,
/// control or direction character to make it read as another address.
pub fn may_open(url: &str, schemes: &[Scheme]) -> bool {
    let found = schemes
        .iter()
        .find(|scheme| starts_with_scheme(url, scheme));
    let Some(scheme) = found else {
        return false;
    };
    let mut characters = url.chars();
    let disguised = characters.any(could_disguise);
    if disguised {
        return false;
    }
    let rest = &url[scheme.prefix.len()..];
    if !scheme.has_authority {
        return !rest.is_empty();
    }
    let mut authority = rest;
    if let Some(authority_end) = rest.find(['/', '?', '#']) {
        authority = &rest[..authority_end];
    }
    let has_user = authority.contains('@');
    let has_host = !authority.is_empty() && !authority.starts_with(':');
    has_host && !has_user
}

fn starts_with_scheme(url: &str, scheme: &Scheme) -> bool {
    let Some(start) = url.get(..scheme.prefix.len()) else {
        return false;
    };
    start.eq_ignore_ascii_case(scheme.prefix)
}

fn could_disguise(character: char) -> bool {
    let direction_mark = matches!(character, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    character.is_whitespace() || character.is_control() || direction_mark
}

#[cfg(test)]
mod tests {
    use super::{SHARED_SCHEMES, TERMINAL_SCHEMES, may_open};

    #[test]
    fn only_a_plain_address_of_an_allowed_scheme_opens() {
        let both = [
            "https://github.com/ldelvoye/taco-shells/pull/155",
            "HTTPS://claude.ai/artifact/x",
            "https://github.com?from=me@example.com",
            "https://github.com#me@example.com",
        ];
        for url in both {
            assert!(may_open(url, &SHARED_SCHEMES), "{url}");
            assert!(may_open(url, &TERMINAL_SCHEMES), "{url}");
        }

        let terminal_only = ["HTTP://example.com", "mailto:someone@example.com"];
        for url in terminal_only {
            assert!(may_open(url, &TERMINAL_SCHEMES), "{url}");
            assert!(!may_open(url, &SHARED_SCHEMES), "{url}");
        }

        let neither = [
            "https://",
            "https:///etc/passwd",
            "https://github.com@evil.example/pull/1",
            "http://github.com@evil.example/pull/1",
            "https://:443/x",
            "https://git hub.com/x",
            "https://github.com/x\n",
            "https://github.com/\u{7}x",
            "https://\u{202E}moc.buhtig/x",
            "https://github.com/x\u{2069}",
            "mailto:",
            "mailto:someone@example.com\u{202E}",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "vscode://open",
        ];
        for url in neither {
            assert!(!may_open(url, &TERMINAL_SCHEMES), "{url:?}");
            assert!(!may_open(url, &SHARED_SCHEMES), "{url:?}");
        }
    }
}
