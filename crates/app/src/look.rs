use gpui::{BoxShadow, Hsla, Rgba, black, px, rgb};
use taco_protocol::Rgb;
use taco_settings::{Appearance, palette_for};

pub const FONT_FAMILY: &str = "Figtree";
pub const TEXT_SIZE: f32 = 12.5;
pub const SMALL_TEXT_SIZE: f32 = 11.5;
pub const TITLE_BAR_HEIGHT: f32 = 38.;
pub const STRIP_WIDTH: f32 = 36.;
pub const STRIP_BUTTON_SIZE: f32 = 28.;
pub const ICON_SIZE: f32 = 16.;
pub const PANEL_TITLE_HEIGHT: f32 = 30.;
pub const ROW_HEIGHT: f32 = 30.;
pub const ROW_RADIUS: f32 = 8.;
pub const TRAY_RADIUS: f32 = 11.;
pub const MARK_SIZE: f32 = 9.;
// Each author's label colour in the agent log, light then dark, all at least 4.5:1 on the panel ground.
const AGENT_INK: [u32; 2] = [0xA8570A, 0xF0A04A];
const USER_INK: [u32; 2] = [0x4F7A1F, 0x7FB239];
const OTHER_AGENT_INK: [u32; 2] = [0x8E4A9E, 0xC08AD6];

const VEIL_ALPHA: f32 = 0.05;
const STRONG_VEIL_ALPHA: f32 = 0.09;

/// The window's colors for one appearance: the roles `settings` gives, and tints made from them.
#[derive(Clone, Debug)]
pub struct Look {
    pub appearance: Appearance,
    pub panels: Hsla,
    pub terminal_background: Hsla,
    pub text: Hsla,
    pub secondary_text: Hsla,
    pub hairline: Hsla,
    pub selected_card: Hsla,
    pub working: Hsla,
    pub done: Hsla,
    pub needs: Hsla,
    /// A wash of the text color: a tray, a hovered row.
    pub veil: Hsla,
    /// A stronger wash: a pressed or open strip button.
    pub strong_veil: Hsla,
    /// The agent log's label colours for the agent, the user and another agent; the app's is `secondary_text`.
    pub agent_ink: Hsla,
    pub user_ink: Hsla,
    pub other_agent_ink: Hsla,
}

impl Look {
    pub fn new(appearance: Appearance) -> Self {
        let colors = palette_for(appearance).window;
        let text = hsla(colors.text);
        let ink_index = match appearance {
            Appearance::Light => 0,
            Appearance::Dark => 1,
        };
        Self {
            appearance,
            panels: hsla(colors.panels),
            terminal_background: hsla(colors.terminal_background),
            text,
            secondary_text: hsla(colors.secondary_text),
            hairline: hsla(colors.hairline),
            selected_card: hsla(colors.selected_card),
            working: hsla(colors.working),
            done: hsla(colors.done),
            needs: hsla(colors.needs),
            veil: text.opacity(VEIL_ALPHA),
            strong_veil: text.opacity(STRONG_VEIL_ALPHA),
            agent_ink: rgb(AGENT_INK[ink_index]).into(),
            user_ink: rgb(USER_INK[ink_index]).into(),
            other_agent_ink: rgb(OTHER_AGENT_INK[ink_index]).into(),
        }
    }

    /// The lift under the one raised thing: the selected row.
    pub fn card_shadow(&self) -> Vec<BoxShadow> {
        let shade = match self.appearance {
            Appearance::Light => self.text.opacity(0.13),
            Appearance::Dark => black().opacity(0.45),
        };
        let lift = BoxShadow::new(px(0.), px(1.), shade).blur_radius(px(3.));
        vec![lift]
    }

    /// The selected card's edge; in dark it has to stand off a lighter card.
    pub fn card_edge(&self) -> Hsla {
        match self.appearance {
            Appearance::Light => self.hairline,
            Appearance::Dark => self.text.opacity(0.12),
        }
    }
}

fn hsla(color: Rgb) -> Hsla {
    let channels = Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: 1.,
    };
    channels.into()
}
