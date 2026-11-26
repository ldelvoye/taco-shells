use taco_protocol::{Rgb, TerminalColors};

const LIGHT_PALETTE: Palette = Palette {
    terminal: TerminalColors {
        foreground: rgb(0x33291F),
        background: rgb(0xFFFFFF),
        cursor: rgb(0x2B211C),
        cursor_text: rgb(0xFFFFFF),
        selection: rgb(0xE4E2DF),
        ansi: [
            rgb(0x2B211C),
            rgb(0xC8323C),
            rgb(0x4F8A1E),
            rgb(0xA8740A),
            rgb(0x2F6FB5),
            rgb(0xA8358F),
            rgb(0x1D8A99),
            rgb(0x6F6862),
            rgb(0x77706A),
            rgb(0xE04852),
            rgb(0x5E9C28),
            rgb(0xC98A12),
            rgb(0x3D83CF),
            rgb(0xC94AA3),
            rgb(0x26A3B3),
            rgb(0xA8A29A),
        ],
    },
    window: WindowColors {
        panels: rgb(0xF5F5F4),
        terminal_background: rgb(0xFFFFFF),
        text: rgb(0x2B211C),
        secondary_text: rgb(0x77706A),
        hairline: rgb(0xE4E2DF),
        selected_card: rgb(0xFFFFFF),
        working: rgb(0xD9741A),
        done: rgb(0x5E8C28),
        needs: rgb(0xC81E5B),
    },
};

const DARK_PALETTE: Palette = Palette {
    terminal: TerminalColors {
        foreground: rgb(0xD6D0C9),
        background: rgb(0x191918),
        cursor: rgb(0xECE7E1),
        cursor_text: rgb(0x191918),
        selection: rgb(0x3A3836),
        ansi: [
            rgb(0x2F2E2C),
            rgb(0xF0646C),
            rgb(0x8FC04A),
            rgb(0xF0B64A),
            rgb(0x6AA7E8),
            rgb(0xE07CC0),
            rgb(0x5CC8D4),
            rgb(0xD6D0C9),
            rgb(0x6B6661),
            rgb(0xFF8088),
            rgb(0xA6D66B),
            rgb(0xFFD06A),
            rgb(0x8FC1FF),
            rgb(0xF09AD4),
            rgb(0x82DDE6),
            rgb(0xFFFFFF),
        ],
    },
    window: WindowColors {
        panels: rgb(0x1F1F1E),
        terminal_background: rgb(0x191918),
        text: rgb(0xECE7E1),
        secondary_text: rgb(0x9A948D),
        hairline: rgb(0x333231),
        selected_card: rgb(0x3A3836),
        working: rgb(0xF0902F),
        done: rgb(0x7FB239),
        needs: rgb(0xEE5A8F),
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowColors {
    pub panels: Rgb,
    pub terminal_background: Rgb,
    pub text: Rgb,
    pub secondary_text: Rgb,
    pub hairline: Rgb,
    pub selected_card: Rgb,
    pub working: Rgb,
    pub done: Rgb,
    pub needs: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub terminal: TerminalColors,
    pub window: WindowColors,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

pub fn palette_for(appearance: Appearance) -> Palette {
    match appearance {
        Appearance::Light => LIGHT_PALETTE,
        Appearance::Dark => DARK_PALETTE,
    }
}
