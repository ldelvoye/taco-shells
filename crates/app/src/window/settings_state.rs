//! The window's settings: the typed fields, writing a setting and moving a control from its old value.

use std::collections::HashMap;
use std::time::Instant;

use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};
use taco_settings::{SettingChange, Settings};

use super::{PageKeyboard, WindowView};
use crate::look::Look;
use crate::motion::{
    DrawnBounds, LONGEST_SETTING_MOVE, Moved, SWITCH_TIME, card_name, progress, switch_amount,
    switch_value,
};
use crate::settings_page::{SettingsFields, Typed, TypedSetting, published_text, typed};
use crate::text_field::{TextField, TextFieldEvent};

const NOT_WHOLE: &str = "Scrollback must be a whole number.";
const TYPED_SETTINGS: [TypedSetting; 2] = [TypedSetting::Font, TypedSetting::Scrollback];

/// The settings page's typed fields and what it last wrote or was refused.
pub(super) struct SettingsState {
    pub(super) fields: SettingsFields,
    /// The typed setting whose field has the keyboard; a new config leaves its text alone.
    pub(super) editing: Option<TypedSetting>,
    /// Watches the fields' focus, set up on the first frame since it needs the window.
    pub(super) field_focus: Vec<Subscription>,
    /// Why each setting's last write was refused, by its name in settings.json, until it is written again.
    pub(super) refused: HashMap<&'static str, String>,
    /// The switches and segments a config changed, each with the value it is moving from.
    pub(super) moved: HashMap<&'static str, Moved>,
    pub(super) drawn_bounds: DrawnBounds,
}

impl WindowView {
    pub(crate) fn change_setting(&mut self, change: SettingChange) {
        let key = change.key();
        self.settings.refused.remove(key);
        self.services.write_setting(change);
    }

    fn field(&self, setting: TypedSetting) -> &Entity<TextField> {
        match setting {
            TypedSetting::Font => &self.settings.fields.font,
            TypedSetting::Scrollback => &self.settings.fields.scrollback,
        }
    }

    fn field_said(
        &mut self,
        setting: TypedSetting,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Edited(_) => {}
            TextFieldEvent::Submitted => {
                self.send_typed(setting, cx);
            }
            TextFieldEvent::Cancelled => {
                // The field gives the keyboard to the page, so the blur that follows finds nothing new to write and a second Esc closes the page.
                self.show_published(setting, cx);
                self.keyboard.page_wants_keyboard = Some(PageKeyboard::Page);
                cx.notify();
            }
        }
    }

    /// Writes the field's value on losing focus, as Enter would.
    fn left_field(&mut self, setting: TypedSetting, cx: &mut Context<Self>) {
        if self.settings.editing == Some(setting) {
            self.settings.editing = None;
        }
        let sent = self.send_typed(setting, cx);
        if !sent {
            self.show_published(setting, cx);
        }
    }

    /// Writes what the field holds when it is a new, valid value; says whether it did.
    fn send_typed(&mut self, setting: TypedSetting, cx: &mut Context<Self>) -> bool {
        let text = self.field(setting).read(cx).text().to_string();
        let key = setting.key();
        let result = typed(setting, &text, &self.shown.settings);
        cx.notify();
        match result {
            Typed::Change(change) => {
                self.change_setting(change);
                true
            }
            Typed::Unchanged => {
                self.settings.refused.remove(key);
                false
            }
            Typed::NotWhole => {
                self.settings.refused.insert(key, NOT_WHOLE.to_string());
                false
            }
        }
    }

    fn show_published(&self, setting: TypedSetting, cx: &mut Context<Self>) {
        let text = published_text(setting, &self.shown.settings);
        let field = self.field(setting);
        field.update(cx, |field, cx| field.set_text(&text, cx));
    }

    /// Shows the published values and the look in every field but the one being typed in.
    pub(super) fn refresh_fields(&self, cx: &mut Context<Self>) {
        for setting in TYPED_SETTINGS {
            let look = self.look.clone();
            let field = self.field(setting);
            field.update(cx, |field, cx| field.set_look(look, cx));
            if self.settings.editing != Some(setting) {
                self.show_published(setting, cx);
            }
        }
        let look = self.look.clone();
        self.shortcuts
            .search
            .update(cx, |field, cx| field.set_look(look, cx));
    }

    pub(super) fn watch_field_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.field_focus.is_empty() {
            return;
        }
        for setting in TYPED_SETTINGS {
            let handle = self.field(setting).focus_handle(cx);
            let entered = cx.on_focus(&handle, window, move |view, _, _| {
                view.settings.editing = Some(setting);
            });
            let left = cx.on_blur(&handle, window, move |view, _, cx| {
                view.left_field(setting, cx);
            });
            self.settings.field_focus.push(entered);
            self.settings.field_focus.push(left);
        }
    }

    /// A move away from `from`; one landing while the last is under way starts where the control is drawn now.
    pub(super) fn move_from(
        &self,
        from: SettingChange,
        now: Instant,
        reduce_motion: bool,
    ) -> Moved {
        let key = from.key();
        let mut start_amount = None;
        let mut start_bounds = None;
        let mut under_way = None;
        if let Some(previous) = self.settings.moved.get(key) {
            let done = progress(previous.since, now, LONGEST_SETTING_MOVE, reduce_motion);
            if done < 1. {
                under_way = Some(previous);
            }
        }
        if let Some(previous) = under_way {
            if let Some(was_heading_to) = switch_value(&from) {
                let done = progress(previous.since, now, SWITCH_TIME, reduce_motion);
                let drawn = switch_amount(was_heading_to, previous.start_amount, done);
                start_amount = Some(drawn);
            }
            let card = card_name(key);
            let table = self.settings.drawn_bounds.borrow();
            start_bounds = table.get(&card).copied();
        }
        Moved {
            from,
            since: now,
            start_amount,
            start_bounds,
        }
    }
}

pub(super) fn settings_fields(
    settings: &Settings,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> SettingsFields {
    let font = typed_field_for(TypedSetting::Font, settings, look, cx);
    let scrollback = typed_field_for(TypedSetting::Scrollback, settings, look, cx);
    SettingsFields { font, scrollback }
}

fn typed_field_for(
    setting: TypedSetting,
    settings: &Settings,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Entity<TextField> {
    let text = published_text(setting, settings);
    let look = look.clone();
    let field = cx.new(|cx| TextField::new(&text, look, cx).with_shell_focus());
    let heard = cx.subscribe(&field, move |view, _, event: &TextFieldEvent, cx| {
        view.field_said(setting, event, cx);
    });
    heard.detach();
    field
}
