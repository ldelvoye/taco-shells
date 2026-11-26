const FORK_GLYPH: char = '⑂';

struct ForkMark<'a> {
    number: u64,
    base: &'a str,
}

// A bare glyph is fork 1, and a name with no marker is fork 0: the original.
fn fork_mark_of(name: &str) -> ForkMark<'_> {
    let original = ForkMark {
        number: 0,
        base: name,
    };
    let Some(after_glyph) = name.strip_prefix(FORK_GLYPH) else {
        return original;
    };

    let digit_count = after_glyph.bytes().take_while(u8::is_ascii_digit).count();
    let (digits, rest) = after_glyph.split_at(digit_count);
    let base;
    if rest.is_empty() {
        base = rest;
    } else if let Some(stripped) = rest.strip_prefix(' ') {
        base = stripped;
    } else {
        return original;
    }

    if digits.is_empty() {
        return ForkMark { number: 1, base };
    }
    match digits.parse() {
        Ok(number) => ForkMark { number, base },
        Err(_) => original,
    }
}

/// The name for a fork of a row, numbered past the forks already open: "Tacos fact" gives "⑂ Tacos fact".
pub fn fork_name_for(source_name: &str, open_names: &[&str]) -> String {
    let source = fork_mark_of(source_name);

    let marks = open_names.iter().map(|open| fork_mark_of(open));
    let same_base = marks.filter(|open| open.base == source.base);
    let highest = same_base.map(|open| open.number).max();
    let next = match highest {
        Some(number) => number.saturating_add(1),
        None => 1,
    };

    let mut marker = FORK_GLYPH.to_string();
    if next > 1 {
        marker.push_str(&next.to_string());
    }
    if source.base.is_empty() {
        return marker;
    }
    format!("{marker} {}", source.base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_fork_takes_the_glyph_and_later_ones_are_numbered_past_the_open_ones() {
        assert_eq!(fork_name_for("Tacos fact", &["Tacos fact"]), "⑂ Tacos fact");
        assert_eq!(
            fork_name_for("Tacos fact", &["Tacos fact", "⑂ Tacos fact"]),
            "⑂2 Tacos fact"
        );
        assert_eq!(
            fork_name_for("⑂ Tacos fact", &["⑂3 Tacos fact", "⑂9 Other"]),
            "⑂4 Tacos fact"
        );
        assert_eq!(fork_name_for("⑂2Tacos", &[]), "⑂ ⑂2Tacos");
        assert_eq!(fork_name_for("", &[""]), "⑂");
    }
}
