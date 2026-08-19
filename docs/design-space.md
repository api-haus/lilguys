# lilguys — design space

An always-on desktop buddy for Hyprland. It watches, it remembers, it emotes. It speaks rarely.
It is the topmost surface of a hermes agent that also answers on Telegram.

All numbers below are measured on this machine (Hyprland 0.56.2, DP-1 @ 2560x1440), not recalled.

## 1. The introspection ladder

Wayland gives more than its reputation suggests, but it gives it in tiers. The design rule is one
sentence: **never climb a rung when a lower rung answers the question.**

### Rung 0 — push, free

No polling. The kernel wakes the process when something happens. Everything here is live on this
box today.

| source | what it tells you |
|---|---|
| `ext_workspace_manager_v1` | workspace list, which is active, switch events |
| `zwlr_foreign_toplevel_manager_v1` | every window: title, app_id, state (activated/minimised/maximised/fullscreen), output |
| `ext_foreign_toplevel_list_v1` | the sanctioned successor: title + app_id + identifier, no state |
| `hyprland_toplevel_mapping_manager_v1` | maps a foreign-toplevel handle to a Hyprland window address |
| `ext_idle_notifier_v1` | user went idle after N seconds; user came back |
| `ext_data_control_manager_v1` | clipboard contents on every copy, without focus |
| MPRIS over D-Bus | what is playing, and its URL |
| `org.a11y.Bus` (AT-SPI2) | text content of GTK/Qt widget trees, no pixels |

The MPRIS result is the important one. Measured, live:

```
firefox xesam:title  Mediocre Movies That Changed My Life
firefox xesam:artist Like Stories of Old
firefox xesam:url    https://www.youtube.com/watch?v=I0CpxSfg-Vg
```

The video ID arrives as a D-Bus `PropertiesChanged` signal. No screen capture, no OCR, no browser
extension, no polling. The transcript is one HTTP call away from a free push event.

### Rung 1 — poll, near-free

Cursor position has no Wayland protocol: a surface gets pointer events only inside its own input
region. Hyprland's IPC socket answers instead.

```
$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock  <- write "cursorpos"
```

Measured over 500 round trips: **14.6 us per query**. At 60 Hz that is 0.09% of one core. The
`hyprctl` binary costs 2.9 ms for the same answer — 200x worse, because it is process spawn. Never
shell out in the hot path.

`hyprctl clients` (same socket) is also the only source of another window's geometry; no Wayland
protocol exposes a rectangle for a window you do not own.

### Rung 2 — pixels, on demand

| protocol | scope |
|---|---|
| `ext_image_copy_capture_manager_v1` | the modern one: capture with **damage regions**, so the compositor tells you what changed |
| `ext_foreign_toplevel_image_capture_source_manager_v1` | point that at one named window |
| `ext_output_image_capture_source_manager_v1` | point it at a whole output |
| `hyprland_toplevel_export_manager_v1` | Hyprland's older per-window export |
| `zwlr_screencopy_manager_v1` | legacy whole-output |

None of these raise a portal prompt, unlike the ScreenCast portal. Damage regions are what make
this affordable: you are told when a region changed and can ignore frames that do not matter.

Measured: `grim -o DP-1` on the full 2560x1440 output is **64 ms wall**, almost entirely PNG
encode. A raw dmabuf grab is 1-2 ms. Never encode PNG on the sensing path.

### Rung 3 — expensive

OCR (`tesseract` is installed) or a VLM over a captured buffer. This is the rung that costs money
and latency. In the YouTube case it is never reached, because rung 0 already knew.

## 2. What Wayland refuses

| want | status | route |
|---|---|---|
| global cursor position | no protocol | Hyprland IPC, rung 1 |
| global key events | deliberately blocked | `hyprland_global_shortcuts_manager_v1` for registered hotkeys only |
| another window's geometry | not in foreign-toplevel | `hyprctl clients` |
| place yourself at an arbitrary screen coordinate | xdg-shell cannot | `zwlr_layer_shell_v1` |
| read text from any window | partial | AT-SPI covers GTK/Qt/Electron-with-a11y; games and custom text renderers are pixels-only |

The keylogging gap is a feature. The positioning gap is the one structural constraint on a
wandering desktop pet, and it shapes the whole render layer.

## 3. Placement

One full-output layer surface on the `overlay` layer, `keyboard-interactivity: none`,
`exclusive_zone: -1`. Then:

- **Input region is the buddy's silhouette.** `wl_surface.set_input_region` with the character's
  bounding box or a real alpha-derived region. Everything outside is click-through, so the desktop
  underneath behaves exactly as if the buddy were not there. This is what makes "click on it to
  message it" cost nothing everywhere else.
- **Movement is an offset inside that one surface**, or `wl_subsurface.set_position` on a child. No
  compositor round trip per step.
- **The overlay layer draws above fullscreen windows.** So the buddy survives a fullscreen game or
  video — which is exactly when it should be quietest. Make "fullscreen focused, go still" an
  explicit policy, not an accident of stacking.
- **Layer surfaces are not on workspaces.** The buddy does not get hidden by a workspace switch.
  Workspace awareness is therefore a thing it chooses to react to, never something that happens
  to it.

### The rejected alternative

A Hyprland C++ plugin gets cursor position for free, real window rectangles, and render hooks
inside the compositor. It also has to be recompiled against Hyprland's headers on every 0.x bump,
and one segfault takes the session down with it. `hyprland.pc` is installed, so the door is open —
but not for a thing that has to still be running in six months.

## 4. Rendering

The seam is one interface. Above it is the buddy's mind; below it is a puppet.

```rust
trait Avatar {
    fn load(&mut self, path: &Path) -> Result<()>;
    fn set_param(&mut self, id: ParamId, v: f32);   // gaze_x, gaze_y, blink, mouth, mood_*
    fn play(&mut self, motion: MotionId, prio: u8);
    fn advance(&mut self, dt: f32);
    fn draw(&mut self, frame: &mut Frame);
    fn hit_test(&self, x: f32, y: f32) -> bool;     // feeds set_input_region
    fn bounds(&self) -> Rect;
}
```

**The parameter vocabulary is the contract, not the file format.** Borrow Live2D's names
(`ParamAngleX/Y/Z`, `ParamEyeLOpen`, `ParamMouthOpenY`) and the entire VTuber model ecosystem
becomes an asset library for free. Those same parameters map onto glTF morph-target weights and
onto "which of twelve sprites" without translation.

| adapter | how | effort | role |
|---|---|---|---|
| sprite sheet | `tiny-skia`, or a textured quad in wgpu. Reads Shimeji XML if you want the existing corpus. | days | ship first, it proves the plumbing |
| Live2D Cubism | Cubism SDK for Native is C++/OpenGL, Linux x86_64 Core supported. FFI or a C++ shim. Proprietary blob; free licence under a revenue threshold. | ~2 weeks | what people actually picture |
| skinned mesh (glTF / VRM) | wgpu + glTF loader + morph targets and joints. All-Rust, no blob. VRM is glTF plus a blendshape spec — VRoid corpus is free. | ~2 weeks | same endpoint, no licence |
| rive | `rive-rs`, vector, state machines built in | ~1 week | dark horse; state-machine emotes are exactly this shape |

Plumbing: **Rust + smithay-client-toolkit + wgpu**, both already in the local cargo cache (sctk
0.19.2, wgpu 29.x). Roughly 10 MB RSS idle. Not web, not Unity, and not Qt either.

The single biggest lever on cost: **idle submits zero frames.** Not 60 fps of an unchanged
character — nothing at all, until a sensor or an animation says otherwise. A buddy that only
renders when it moves costs less than the cursor does.

## 5. Pace — the three clocks

This is what makes it autistic rather than a chatbot in a costume. Model attention as a budget,
not a loop.

1. **Reflex** — microseconds to milliseconds, no model. Cursor follow, gaze, blink, idle sway,
   workspace-switch reaction, click response. A local state machine. It never asks anyone anything.
2. **Notice** — seconds, no model. Push events land in a ring buffer. Cheap local rules decide
   whether anything is worth a thought. Most events die here, and that is the point.
3. **Think** — minutes, model. Only when Notice raises a flag *and* the budget allows.

The gate between 2 and 3 is the whole design:

- **Novelty** — same app and same title as ten minutes ago produces no thought.
- **Dwell** — a window holds focus for 90 seconds before it is worth reading.
- **Budget** — N thoughts per hour, decaying. Spend drops during flow (fast switching, no idle) and
  rises when the user is idle but present.
- **Silence by default** — a thought does not imply an utterance. Most thoughts write a memory and
  at most shift an expression. Speaking is the rare branch, not the default one.

The YouTube case, end to end: MPRIS `PropertiesChanged` fires (rung 0, free) → new `xesam:url`
carries a `youtube.com/watch` id not seen before → dwell 60 s to confirm it was not skipped past →
*then* one transcript fetch and one summarise call → memory write, "watching Like Stories of Old on
mediocre movies" → mood shifts, an ear twitches. Zero pixels. Zero words. He just knows now.

## 6. Integration with hermes

The agent does not live in the buddy process. The buddy is a client of hermes, exactly as Telegram
is.

- **Reuse what exists.** hermes has a gateway with a platform registry and channel directory
  (`gateway/platform_registry.py`, `gateway/channel_directory.py`) plus per-platform plugins. The
  buddy is one more channel adapter on the same session, so a conversation opened by clicking the
  buddy continues in Telegram and back again.
- **Add what does not.** Ambient observation is a direction hermes has no shape for: input that is
  not a message. That belongs in a plugin writing to memory, not in a channel and definitely not in
  a core model tool — their AGENTS.md is explicit that every core tool ships on every API call.
- **The prompt cache is the real constraint.** hermes treats per-conversation caching as sacred:
  anything that mutates past context invalidates it and multiplies cost. An always-on buddy
  splicing "user is now watching X" into a live conversation would nuke that cache every few
  minutes. So ambient observations go to memory and state, read at conversation start or on
  explicit recall — never injected mid-context. Noticing and talking are two different write paths.

```
lilguysd  (Rust, ~10 MB)              hermes  (Python, existing)
├─ surface/   layer-shell + wgpu      ├─ gateway/platforms/lilguys.py   <- new channel adapter
├─ sensors/                           └─ plugins/ambient/               <- new observation sink
│   ├─ toplevel    (wayland)
│   ├─ workspace   (wayland)
│   ├─ idle        (wayland)
│   ├─ clipboard   (wayland)
│   ├─ mpris       (dbus)
│   ├─ atspi       (dbus)
│   └─ capture     (wayland, on demand)
├─ attention/  three-clock budget gate
├─ avatar/     the trait: sprite | live2d | gltf | rive
└─ link/       unix socket: observations up, utterances and emotes down
```

Sensors are a trait too — `Sensor { fn subscribe(&mut self, tx: Sender<Observation>) }` — so
pluggable IO is real. Adding "what is on my calendar" or "what is playing on the Deck" is a new
file, not a refactor.

## 7. Prior art

- **wl_shimeji** (CluelessCatBurger, C) — the only Wayland-native Shimeji that works. wlr-layer-shell
  plus `wl_subcompositor`. Its placement and subsurface handling is the reference.
- **Shijima-Qt** (pixelomer) — the Qt route. Its `feature/wayland-layer-shell` branch diverged too
  far from main to merge back, and the repo was archived in April 2026. A cautionary tale about
  bolting layer-shell onto a toolkit built for toplevels.
- **Shimeji-EE** (Java, X11) — the content corpus. Thousands of existing character sets in a
  documented XML behaviour format; free assets if the sprite adapter reads it.
- **Live2D Cubism / VTube Studio** — the model corpus and the parameter vocabulary worth adopting.
- **VRM / VRoid** — the glTF-based free equivalent for the mesh adapter.

Every one of them is a puppet with physics. None of them sense anything. The introspection layer is
the novel part of this, and Wayland supports it better than its reputation implies.

## 8. Build order

Each step de-risks the next.

1. `lilguysd` skeleton — layer-shell overlay, wgpu, a flat quad that follows the cursor at
   14.6 us per poll and accepts a click. Proves placement, input region, click-through, idle
   render. ~2 days.
2. Sensor bus and the three-clock attention loop, sensors printing to stdout. No avatar, no model.
   Proves the economy — watch it hold 0% CPU for an hour. ~2 days.
3. Sprite adapter behind the `Avatar` trait. Now it is a desktop pet. ~2 days.
4. hermes link — channel adapter for clicks, ambient plugin for observations. ~3 days.
5. Live2D or VRM adapter. ~2 weeks.

## Sources

- [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji)
- [Shijima-Qt](https://github.com/pixelomer/Shijima-Qt)
- [Shijima-Qt Wayland backend issue](https://github.com/pixelomer/Shijima-Qt/issues/75)
- [Live2D Cubism SDK](https://www.live2d.com/en/sdk/about/)
- [Cubism SDK for Native manual](https://docs.live2d.com/en/cubism-sdk-manual/cubism-sdk-for-native/)
- [Live2D Linux native sample](https://github.com/sawaYch/Live2D-Linux-Native-Sample)
