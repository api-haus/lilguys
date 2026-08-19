//! Push-only sensors that ride the connection the surface already opened. No polling, no pixels.

use super::{Bus, Observation};
use crate::app::App;
use crate::config::Senses;
use std::collections::HashMap;
use wayland_client::{
    globals::{BindError, GlobalList},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, State as WsState},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, State as TopState, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

#[derive(Default)]
struct Toplevel {
    app_id: String,
    title: String,
    activated: bool,
}

#[derive(Default)]
pub struct Sensors {
    pub bus: Bus,
    toplevels: HashMap<u32, Toplevel>,
    /// Pending fields, applied on `done` — the protocol sends title and app_id separately.
    staged: HashMap<u32, Toplevel>,
    focused: Option<u32>,
    workspaces: HashMap<u32, String>,
    active_workspace: Option<u32>,
    pub present: bool,
    idle_notifier: Option<ExtIdleNotifierV1>,
    idle_notification: Option<ExtIdleNotificationV1>,
    away_after_ms: u32,
}

impl Sensors {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>, senses: &Senses) -> Self {
        let mut me = Self {
            present: true,
            away_after_ms: senses.away_after.as_millis().min(u32::MAX as u128) as u32,
            ..Default::default()
        };
        // Every one of these is optional: a compositor without them costs the buddy a sense, not a
        // crash. Hyprland 0.56 carries all three.
        if senses.windows {
            if let Err(e) = globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(qh, 1..=3, ()) {
                warn("wlr-foreign-toplevel", e);
            }
        }
        if senses.workspaces {
            if let Err(e) = globals.bind::<ExtWorkspaceManagerV1, _, _>(qh, 1..=1, ()) {
                warn("ext-workspace", e);
            }
        }
        if senses.idle {
            match globals.bind::<ExtIdleNotifierV1, _, _>(qh, 1..=1, ()) {
                // The seat arrives on a later roundtrip, so `arm_idle` finishes the wiring.
                Ok(notifier) => me.idle_notifier = Some(notifier),
                Err(e) => warn("ext-idle-notify", e),
            }
        }
        me
    }

    /// `app_id — title` of whatever holds focus, for the model's framing line.
    pub fn focused_title(&self) -> Option<String> {
        let t = self.toplevels.get(&self.focused?)?;
        Some(if t.title.is_empty() {
            t.app_id.clone()
        } else {
            format!("{} — {}", t.app_id, super::clip(&t.title, 60))
        })
    }

    pub fn workspace_name(&self) -> Option<String> {
        self.workspaces.get(&self.active_workspace?).cloned()
    }

    fn focus_changed(&mut self, id: u32) {
        if self.focused == Some(id) {
            return;
        }
        self.focused = Some(id);
        if let Some(t) = self.toplevels.get(&id) {
            self.bus.push(Observation::Focus { app_id: t.app_id.clone(), title: t.title.clone() });
        }
    }
}

fn warn(what: &str, e: BindError) {
    eprintln!("sensor {what} unavailable: {e}");
}

// The idle notifier needs a seat, which binds on a later roundtrip than the globals do.
impl Sensors {
    pub fn arm_idle(&mut self, seat: &wayland_client::protocol::wl_seat::WlSeat, qh: &QueueHandle<App>) {
        if let Some(n) = self.idle_notifier.as_ref() {
            if self.idle_notification.is_none() {
                self.idle_notification = Some(n.get_idle_notification(self.away_after_ms, seat, qh, ()));
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        state: &mut Self, _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event, _: &(), _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let id = toplevel.id().protocol_id();
            state.sensors.staged.insert(id, Toplevel::default());
        }
    }

    wayland_client::event_created_child!(App, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        state: &mut Self, handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = handle.id().protocol_id();
        let s = &mut state.sensors;
        let staged = s.staged.entry(id).or_default();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => staged.title = title,
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => staged.app_id = app_id,
            zwlr_foreign_toplevel_handle_v1::Event::State { state: bytes } => {
                staged.activated = bytes
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .any(|v| v == TopState::Activated as u32);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                let next = std::mem::take(s.staged.entry(id).or_default());
                let renamed = s
                    .toplevels
                    .get(&id)
                    .is_some_and(|old| old.title != next.title && s.focused == Some(id));
                let activated = next.activated;
                let (app_id, title) = (next.app_id.clone(), next.title.clone());
                s.toplevels.insert(id, next);
                s.staged.insert(id, Toplevel { app_id: app_id.clone(), title: title.clone(), activated });
                if renamed {
                    s.bus.push(Observation::Title { app_id, title });
                } else if activated {
                    s.focus_changed(id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                s.toplevels.remove(&id);
                s.staged.remove(&id);
                if s.focused == Some(id) {
                    s.focused = None;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for App {
    fn event(
        state: &mut Self, _: &ExtWorkspaceManagerV1, event: ext_workspace_manager_v1::Event, _: &(),
        _: &Connection, _: &QueueHandle<Self>,
    ) {
        if let ext_workspace_manager_v1::Event::Done = event {
            if let Some(id) = state.sensors.active_workspace {
                if let Some(name) = state.sensors.workspaces.get(&id).cloned() {
                    state.sensors.bus.push(Observation::Workspace { name });
                }
            }
        }
    }

    wayland_client::event_created_child!(App, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for App {
    fn event(
        _: &mut Self, _: &ExtWorkspaceGroupHandleV1, _: ext_workspace_group_handle_v1::Event, _: &(),
        _: &Connection, _: &QueueHandle<Self>,
    ) {
    }

    wayland_client::event_created_child!(App, ExtWorkspaceGroupHandleV1, []);
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for App {
    fn event(
        state: &mut Self, handle: &ExtWorkspaceHandleV1, event: ext_workspace_handle_v1::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>,
    ) {
        let id = handle.id().protocol_id();
        match event {
            ext_workspace_handle_v1::Event::Name { name } => {
                state.sensors.workspaces.insert(id, name);
            }
            ext_workspace_handle_v1::Event::State { state: flags } => {
                let active = flags.into_result().is_ok_and(|f| f.contains(WsState::Active));
                if active {
                    state.sensors.active_workspace = Some(id);
                } else if state.sensors.active_workspace == Some(id) {
                    state.sensors.active_workspace = None;
                }
            }
            ext_workspace_handle_v1::Event::Removed => {
                state.sensors.workspaces.remove(&id);
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for App {
    fn event(
        _: &mut Self, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for App {
    fn event(
        state: &mut Self, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &(),
        _: &Connection, _: &QueueHandle<Self>,
    ) {
        let present = match event {
            ext_idle_notification_v1::Event::Idled => false,
            ext_idle_notification_v1::Event::Resumed => true,
            _ => return,
        };
        if state.sensors.present != present {
            state.sensors.present = present;
            state.sensors.bus.push(Observation::Presence { present });
        }
    }
}
