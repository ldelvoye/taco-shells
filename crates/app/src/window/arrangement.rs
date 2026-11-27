//! The window's arrangement: panels, popups and drags of terminals and tab groups.

use std::collections::HashMap;
use std::time::Instant;

use gpui::{App, AppContext, Bounds, Context, Pixels, Point, Size, Window, point, px, size};
use taco_protocol::{
    Dragged, DropPlace, Edge, Panel, RequestReply, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_rules::{DropPoint, drop_for};

use super::{Awaited, WindowView};
use crate::look::{ROW_HEIGHT, STRIP_WIDTH, TITLE_BAR_HEIGHT};
use crate::page::PageMove;
use crate::panels::{PanelDrag, ResizeDrag, edge_near, size_for_pointer};
use crate::popups::{Popup, RenameStep, rename_step};
use crate::slot::{Slot, SlotLine, opened_height};
use crate::terminal_list::ListDrag;
use crate::text_field::{TextField, TextFieldEvent};

/// The gestures in progress on the list and the panels, and the popup and requests they wait on.
pub(super) struct ArrangementState {
    pub(super) drop_place: Option<DropPlace>,
    pub(super) popup: Option<Popup>,
    /// A panel being resized, and the size it is drawn at until the release sends it.
    pub(super) resizing: Option<(Panel, f32)>,
    /// A panel being moved by its title, and the edge it would go to with the pointer where it is.
    pub(super) panel_target: Option<(Panel, Edge)>,
    pub(super) last_token: u64,
    pub(super) awaiting: HashMap<u64, Awaited>,
    /// Where the landing slot is and when it started opening there.
    pub(super) slot_opened: Option<(SlotLine, Instant)>,
    /// The slot as drawn this frame.
    pub(super) slot: Option<Slot>,
}

impl WindowView {
    pub(super) fn replied(&mut self, token: u64, reply: RequestReply, cx: &mut Context<Self>) {
        let Some(awaited) = self.arrangement.awaiting.remove(&token) else {
            return;
        };
        match awaited {
            Awaited::NewGroup { at } => {
                let Some(tab_group) = reply.tab_group else {
                    return;
                };
                // A popup opened since the request is the user's more recent choice.
                if self.arrangement.popup.is_some() {
                    return;
                }
                self.open_group_popup(tab_group, at, cx);
            }
        }
    }

    fn send_awaiting(&mut self, request: WorkspaceRequest, awaited: Awaited) {
        self.arrangement.last_token += 1;
        let token = self.arrangement.last_token;
        self.arrangement.awaiting.insert(token, awaited);
        self.port.send(request, Some(token));
    }

    pub(crate) fn open_row_popup(
        &mut self,
        terminal: TerminalId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.arrangement.popup = Some(Popup::Row { terminal, at });
        cx.notify();
    }

    pub(crate) fn open_group_popup(
        &mut self,
        tab_group: TabGroupId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let name = self.tab_group_name(&tab_group);
        let look = self.look.clone();
        let field = cx.new(|cx| TextField::new(&name, look, cx));
        let renamed = tab_group.clone();
        let opening_name = name.clone();
        let edits = cx.subscribe(&field, move |view, _, event: &TextFieldEvent, cx| {
            let step = rename_step(event, &opening_name);
            match step {
                RenameStep::Rename(text) => view.rename(&renamed, &text),
                RenameStep::Close => view.close_popup(cx),
                RenameStep::RenameAndClose(text) => {
                    view.rename(&renamed, &text);
                    view.close_popup(cx);
                }
            }
        });
        self.arrangement.popup = Some(Popup::Group {
            tab_group,
            at,
            field,
            needs_focus: true,
            _edits: edits,
        });
        cx.notify();
    }

    fn tab_group_name(&self, tab_group: &TabGroupId) -> String {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return String::new();
        };
        let mut tab_groups = arrangement.tab_groups.iter();
        let Some(found) = tab_groups.find(|group| &group.id == tab_group) else {
            return String::new();
        };
        found.name.clone()
    }

    /// A panel's strip button: over a page, it closes the pages and shows the window as it was, toggling nothing unseen.
    pub(crate) fn strip_panel_clicked(&mut self, panel: Panel, cx: &mut Context<Self>) {
        if !self.pages.open.is_empty() {
            self.move_pages(PageMove::CloseAll, cx);
            return;
        }
        self.toggle_panel(panel);
    }

    pub(crate) fn toggle_panel(&self, panel: Panel) {
        let drawn = self.drawn();
        let place = drawn.panels.place(panel);
        if place.open {
            self.close_panel(panel);
            return;
        }
        let request = WorkspaceRequest::PanelOpened { panel };
        self.port.send(request, None);
    }

    pub(crate) fn close_panel(&self, panel: Panel) {
        let request = WorkspaceRequest::PanelClosed { panel };
        self.port.send(request, None);
    }

    /// The slot to draw this frame, opening from when it last moved; it opens on this clock rather than gpui's so the rows' hit test subtracts exactly the height drawn.
    pub(super) fn open_slot(
        &mut self,
        line: Option<SlotLine>,
        window: &mut Window,
        cx: &App,
    ) -> Option<Slot> {
        let Some(line) = line else {
            self.arrangement.slot_opened = None;
            return None;
        };
        let now = Instant::now();
        let mut opened = now;
        if let Some((open_line, since)) = self.arrangement.slot_opened
            && open_line == line
        {
            opened = since;
        }
        self.arrangement.slot_opened = Some((line, opened));
        let mut height = ROW_HEIGHT;
        if !cx.reduce_motion() {
            height = opened_height(now - opened, ROW_HEIGHT);
        }
        if height < ROW_HEIGHT {
            window.request_animation_frame();
        }
        Some(Slot { line, height })
    }

    /// Notes the edge a dragged panel would go to with the pointer here; true when it changed.
    pub(super) fn aim_panel(
        &mut self,
        panel: Panel,
        pointer: Point<Pixels>,
        window: Size<Pixels>,
    ) -> bool {
        let origin = point(px(STRIP_WIDTH), px(TITLE_BAR_HEIGHT));
        let width = window.width - px(STRIP_WIDTH);
        let height = window.height - px(TITLE_BAR_HEIGHT);
        let area = Bounds::new(origin, size(width, height));
        let edge = edge_near(pointer, area);
        let mut target = None;
        if let Some(edge) = edge {
            target = Some((panel, edge));
        }
        if target == self.arrangement.panel_target {
            return false;
        }
        self.arrangement.panel_target = target;
        true
    }

    pub(super) fn drop_panel(&mut self, drag: &PanelDrag, cx: &mut Context<Self>) {
        let target = self.arrangement.panel_target.take();
        cx.notify();
        let Some((panel, edge)) = target else {
            return;
        };
        if panel != drag.panel {
            return;
        }
        let request = WorkspaceRequest::PanelMoved { panel, edge };
        self.port.send(request, None);
    }

    pub(super) fn resize_to(
        &mut self,
        drag: &ResizeDrag,
        pointer: Point<Pixels>,
        window: Size<Pixels>,
    ) -> bool {
        let size = size_for_pointer(drag.edge, pointer, window);
        let live = Some((drag.panel, size));
        if self.arrangement.resizing == live {
            return false;
        }
        self.arrangement.resizing = live;
        true
    }

    // The size goes to the workspace once, on release, so a resize is one save and one terminal resize.
    pub(super) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        let Some((panel, size)) = self.arrangement.resizing.take() else {
            return;
        };
        cx.notify();
        let request = WorkspaceRequest::PanelResized { panel, size };
        self.port.send(request, None);
    }

    pub(crate) fn close_popup(&mut self, cx: &mut Context<Self>) {
        if self.arrangement.popup.is_none() {
            return;
        }
        self.arrangement.popup = None;
        cx.notify();
    }

    pub(crate) fn fork(&mut self, terminal: &TerminalId, cx: &mut Context<Self>) {
        let request = WorkspaceRequest::ForkTerminal {
            terminal: terminal.clone(),
        };
        self.port.send(request, None);
        self.close_popup(cx);
    }

    pub(crate) fn new_group(
        &mut self,
        terminal: &TerminalId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let request = WorkspaceRequest::TabGroupCreated {
            terminal: terminal.clone(),
        };
        self.send_awaiting(request, Awaited::NewGroup { at });
        self.close_popup(cx);
    }

    pub(crate) fn ungroup(&mut self, tab_group: &TabGroupId, cx: &mut Context<Self>) {
        let request = WorkspaceRequest::TabGroupUngrouped {
            id: tab_group.clone(),
        };
        self.port.send(request, None);
        self.close_popup(cx);
    }

    fn rename(&self, tab_group: &TabGroupId, name: &str) {
        let request = WorkspaceRequest::TabGroupRenamed {
            id: tab_group.clone(),
            name: name.to_string(),
        };
        self.port.send(request, None);
    }

    pub(crate) fn leave_list(&mut self, cx: &mut Context<Self>) {
        if self.arrangement.drop_place.is_none() {
            return;
        }
        self.arrangement.drop_place = None;
        cx.notify();
    }

    pub(crate) fn drag_over(
        &mut self,
        dragged: &Dragged,
        point: DropPoint,
        cx: &mut Context<Self>,
    ) {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return;
        };
        let place = drop_for(arrangement, dragged, &point);
        if place == self.arrangement.drop_place {
            return;
        }
        self.arrangement.drop_place = place;
        cx.notify();
    }

    pub(crate) fn drop_on_list(&mut self, drag: &ListDrag, cx: &mut Context<Self>) {
        let place = self.arrangement.drop_place.take();
        cx.notify();
        let Some(place) = place else {
            return;
        };
        let request = WorkspaceRequest::Dropped {
            dragged: drag.dragged.clone(),
            place,
        };
        self.port.send(request, None);
    }
}
