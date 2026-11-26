//! The native menu bar, rebuilt whenever an agent's setup or a ready update changes.

use gpui::{Action, App, KeyBinding, Menu, MenuItem, actions};
use taco_protocol::HarnessId;
use taco_services::{APPLICATION_NAME, AgentMenuAction, AgentMenuEntry};

use crate::window::Shown;

const AGENTS: &str = "Agents";
const SHOW_HOOK_SCRIPTS: &str = "Show Hook Scripts in Finder";

actions!(
    taco_shells,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        RestartToUpdate,
        ShowHookScripts,
        // Carried by the menu's caption lines, which are drawn disabled and never run.
        Caption,
    ]
);

/// An agent's setup action chosen from its submenu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = taco_shells, no_json, no_register)]
pub struct AgentMenuClicked {
    pub harness: HarnessId,
    pub action: AgentMenuAction,
}

/// The app's own actions, which run with or without a window, and the keys macOS gives them.
pub fn install(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| crate::quit(cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
    ]);
}

pub fn set_menus(shown: &Shown, cx: &mut App) {
    let update = shown.notices.update.as_deref();
    let menus = menus_of(&shown.setups.menus, update);
    cx.set_menus(menus);
}

fn menus_of(agents: &[AgentMenuEntry], update: Option<&str>) -> Vec<Menu> {
    let app_items = [
        MenuItem::action("Hide", Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action("Quit", Quit),
    ];
    let app_menu = Menu::new(APPLICATION_NAME).items(app_items);
    let mut menus = vec![app_menu];
    if !agents.is_empty() {
        let submenus = agents.iter().map(agent_submenu);
        let agents_menu = Menu::new(AGENTS).items(submenus);
        menus.push(agents_menu);
    }
    let window_items = [
        MenuItem::action("Minimize", Minimize),
        MenuItem::action("Zoom", Zoom),
    ];
    let window_menu = Menu::new("Window").items(window_items);
    menus.push(window_menu);
    if let Some(version) = update {
        let label = format!("Restart to Update ({version})");
        let restart = MenuItem::action(label, RestartToUpdate);
        let update_menu = Menu::new("Update").items([restart]);
        menus.push(update_menu);
    }
    menus
}

// gpui menus have no sublabel, so the action's note is a disabled line of its own.
fn agent_submenu(entry: &AgentMenuEntry) -> MenuItem {
    let menu = &entry.menu;
    let status = MenuItem::action(menu.status.clone(), Caption).disabled(true);
    let clicked = AgentMenuClicked {
        harness: entry.harness.clone(),
        action: menu.action,
    };
    let action = MenuItem::action(menu.action_label.clone(), clicked).disabled(!menu.enabled);
    let mut items = vec![status, action];
    if let Some(note) = &menu.note {
        let note_line = MenuItem::action(note.clone(), Caption).disabled(true);
        items.push(note_line);
    }
    let show_scripts = MenuItem::action(SHOW_HOOK_SCRIPTS, ShowHookScripts);
    items.push(MenuItem::separator());
    items.push(show_scripts);
    let submenu = Menu::new(entry.product_name.clone()).items(items);
    MenuItem::submenu(submenu)
}
