use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, rgb, AnyElement, App, AsyncApp, Context, DivInspectorState, Empty, EntityId, Global,
    Inspector, InspectorElementId, KeyBinding, SharedString, StyleRefinement, Window,
};
use serde_json::{json, Map, Value};

use crate::server::{self, Reply, Request};

/// `AsyncApp::update` returns a `Result` on gpui 0.2 and the value on gpui-pre.
trait Up {
    fn up<R>(&self, f: impl FnOnce(&mut App) -> R) -> Result<R, ()>;
}

impl Up for AsyncApp {
    #[cfg(feature = "gpui-0-2")]
    fn up<R>(&self, f: impl FnOnce(&mut App) -> R) -> Result<R, ()> {
        self.update(f).map_err(|_| ())
    }

    #[cfg(feature = "gpui-pre")]
    fn up<R>(&self, f: impl FnOnce(&mut App) -> R) -> Result<R, ()> {
        Ok(self.update(f))
    }
}
use crate::Options;

actions!(impeccable_probe, [ToggleInspector, NextVariant, PreviousVariant, AcceptVariant, RevertVariant]);

#[derive(Default)]
struct Selection {
    id: Option<InspectorElementId>,
    source: Option<String>,
    /// The element's own style when it was picked, before any variant.
    original: Option<Value>,
    bounds: Option<[f32; 4]>,
    seq: u64,
}

struct Variant {
    label: String,
    patch: Value,
}

#[derive(Default)]
struct Live {
    variants: Vec<Variant>,
    current: Option<usize>,
    decision: Option<Value>,
    seq: u64,
}

struct Shared {
    selection: Selection,
    live: Live,
    on_appearance: Option<crate::AppearanceHook>,
    dark: Option<bool>,
    inspector_open: bool,
    port: u16,
    slots: Vec<Slot>,
    /// The slot the keyboard switches when no style variants are shown.
    active_slot: Option<usize>,
}

/// A structure variant point: one `match` in the app's render code.
struct Slot {
    id: &'static str,
    labels: Vec<&'static str>,
    current: usize,
    source: String,
    entities: Vec<EntityId>,
}

#[derive(Clone)]
struct Probe(Rc<RefCell<Shared>>);
impl Global for Probe {}

fn probe(cx: &App) -> Rc<RefCell<Shared>> {
    cx.global::<Probe>().0.clone()
}

pub fn attach(cx: &mut App, options: Options) {
    let (tx, rx) = mpsc::channel::<Request>();
    let port = match server::start(options.port, tx) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("impeccable-gpui-probe: could not bind 127.0.0.1:{}: {e}", options.port);
            return;
        }
    };
    let shared = Rc::new(RefCell::new(Shared {
        selection: Selection::default(),
        live: Live::default(),
        on_appearance: options.on_appearance,
        dark: None,
        inspector_open: false,
        port,
        slots: Vec::new(),
        active_slot: None,
    }));
    cx.set_global(Probe(shared));
    write_discovery(port);

    cx.bind_keys([
        KeyBinding::new("ctrl-alt-i", ToggleInspector, None),
        KeyBinding::new("ctrl-alt-right", NextVariant, None),
        KeyBinding::new("ctrl-alt-left", PreviousVariant, None),
        KeyBinding::new("ctrl-alt-enter", AcceptVariant, None),
        KeyBinding::new("ctrl-alt-backspace", RevertVariant, None),
    ]);
    cx.on_action(|_: &ToggleInspector, cx| {
        cx.defer(|cx| {
            set_inspector(cx, None);
        });
    });
    cx.on_action(|_: &NextVariant, cx| cx.defer(|cx| step(cx, 1)));
    cx.on_action(|_: &PreviousVariant, cx| cx.defer(|cx| step(cx, -1)));
    cx.on_action(|_: &AcceptVariant, cx| cx.defer(accept));
    cx.on_action(|_: &RevertVariant, cx| {
        cx.defer(|cx| {
            let styles = !probe(cx).borrow().live.variants.is_empty();
            if styles {
                let _ = apply(cx, None);
            } else {
                let active = probe(cx).borrow().active_slot;
                if let Some(i) = active {
                    set_slot(cx, i, 0);
                }
            }
        })
    });

    cx.set_inspector_renderer(Box::new(render_panel));
    #[cfg(feature = "gpui-0-2")]
    cx.register_inspector_element(record_selection);
    #[cfg(feature = "gpui-pre")]
    cx.register_inspector_element(|_, _| record_selection);

    cx.spawn(async move |cx: &mut AsyncApp| loop {
        while let Ok(req) = rx.try_recv() {
            handle(req, cx).await;
        }
        cx.background_executor().timer(Duration::from_millis(25)).await;
    })
    .detach();
    eprintln!("impeccable-gpui-probe: listening on 127.0.0.1:{port} (ctrl-alt-i opens the inspector)");
}

/// `.impeccable/gpui-probe.json` in the working directory, so
/// `impeccable gpui` finds the port without being told.
fn write_discovery(port: u16) {
    let dir = std::path::Path::new(".impeccable");
    let _ = std::fs::create_dir_all(dir);
    let body = json!({ "port": port, "pid": std::process::id() });
    let _ = std::fs::write(dir.join("gpui-probe.json"), format!("{body}\n"));
}

/// Open, close (`Some`) or toggle (`None`) the inspector; returns whether it is open.
fn set_inspector(cx: &mut App, open: Option<bool>) -> bool {
    let shared = probe(cx);
    let is_open = shared.borrow().inspector_open;
    if open == Some(is_open) {
        return is_open;
    }
    if with_window(cx, |window, cx| window.toggle_inspector(cx)).is_some() {
        shared.borrow_mut().inspector_open = !is_open;
    }
    let open = shared.borrow().inspector_open;
    open
}

/// Move the pointer to a window point (logical pixels), then click, as a
/// picking user would. The events go through the OS so GPUI treats them as
/// real input.
fn pointer(cx: &mut App, x: f32, y: f32, click: bool) -> Result<(), String> {
    with_window(cx, |window, _| crate::capture::post_pointer(window, x, y, click)).ok_or("no window")?
}

fn with_window<R>(cx: &mut App, f: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
    let handle = cx.active_window().or_else(|| cx.windows().first().copied())?;
    handle.update(cx, |_, window, cx| f(window, cx)).ok()
}

// ─── inspector ─────────────────────────────────────────────────────────────

fn without_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let m: Map<String, Value> = m
                .into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .filter(|(_, v)| !matches!(v, Value::Object(o) if o.is_empty()))
                .collect();
            Value::Object(m)
        }
        other => other,
    }
}

fn record_selection(id: InspectorElementId, state: &DivInspectorState, _window: &mut Window, cx: &mut App) -> Empty {
    let shared = probe(cx);
    let mut s = shared.borrow_mut();
    if s.selection.id.as_ref() != Some(&id) {
        let loc = id.path.source_location;
        s.selection = Selection {
            source: Some(format!("{}:{}:{}", loc.file(), loc.line(), loc.column())),
            original: serde_json::to_value(&*state.base_style).ok().map(without_nulls),
            id: Some(id),
            bounds: None,
            seq: s.selection.seq + 1,
        };
        s.live = Live { seq: s.live.seq, ..Live::default() };
    }
    let b = state.bounds;
    s.selection.bounds = Some([b.origin.x.into(), b.origin.y.into(), b.size.width.into(), b.size.height.into()]);
    Empty
}

fn render_panel(inspector: &mut Inspector, window: &mut Window, cx: &mut Context<Inspector>) -> AnyElement {
    let states = inspector.render_inspector_states(window, cx);
    let picking = inspector.is_picking();
    let shared = probe(cx);
    let s = shared.borrow();
    let muted = rgb(0xa0a4ab);
    let source: SharedString = match (&s.selection.source, picking) {
        (_, true) => "Hover an element, click to pick it.".into(),
        (Some(src), false) => src.clone().into(),
        (None, false) => "Nothing picked.".into(),
    };
    let mut list = div().flex().flex_col().gap_1();
    if s.live.variants.is_empty() {
        list = list.child(div().text_color(muted).child(if s.selection.source.is_some() && !picking {
            "Waiting for variants from the agent."
        } else {
            "Pick an element to ask for variants."
        }));
    }
    for (i, v) in s.live.variants.iter().enumerate() {
        let active = s.live.current == Some(i);
        list = list.child(
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .when(active, |d| d.bg(rgb(0x2f3a4a)))
                .child(format!("{} {}", if active { "▸" } else { " " }, v.label)),
        );
    }
    let decided = s.live.decision.is_some();
    div()
        .id("impeccable-probe-panel")
        .size_full()
        .p_3()
        .flex()
        .flex_col()
        .gap_3()
        .bg(rgb(0x17191d))
        .text_color(rgb(0xe8e8e4))
        .text_size(px(13.))
        .child(div().text_size(px(15.)).child("Impeccable"))
        .child(div().text_color(muted).child(source))
        .children(states)
        .child(list)
        .when(decided, |d| d.child(div().child("Accepted. The agent writes it to the source line above.")))
        .child(
            div()
                .text_color(muted)
                .text_size(px(12.))
                .child("ctrl-alt-← / → switch · ctrl-alt-enter accept · ctrl-alt-backspace original · ctrl-alt-i close"),
        )
        .child(
            div()
                .id("impeccable-probe-pick")
                .px_2()
                .py_1()
                .rounded_sm()
                .border_1()
                .border_color(rgb(0x3a3f47))
                .cursor_pointer()
                .child("Pick another element")
                .on_click(cx.listener(|this: &mut Inspector, _, _, _| this.start_picking())),
        )
        .into_any_element()
}

// ─── variants ──────────────────────────────────────────────────────────────

/// `"background": "#rrggbbaa"` stands for a solid fill; GPUI's own form is a
/// whole `Background` object.
fn expand_shorthand(mut patch: Value) -> Value {
    if let Some(color) = patch.get("background").and_then(Value::as_str).map(str::to_string) {
        patch["background"] = json!({ "Color": {
            "tag": "Solid",
            "color_space": "Srgb",
            "solid": color,
            "gradient_angle_or_pattern_height": 0.0,
            "colors": [{ "color": "#00000000", "percentage": 0.0 }, { "color": "#00000000", "percentage": 0.0 }],
            "pad": 0
        }});
    }
    patch
}

fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else {
                    merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Show variant `index` (or the original style) on the picked element.
fn apply(cx: &mut App, index: Option<usize>) -> Result<(), String> {
    let shared = probe(cx);
    let (id, style) = {
        let s = shared.borrow();
        let id = s.selection.id.clone().ok_or("nothing is picked")?;
        let mut style = s.selection.original.clone().ok_or("the picked element has no style")?;
        if let Some(i) = index {
            let v = s.live.variants.get(i).ok_or("no such variant")?;
            merge(&mut style, &v.patch);
        }
        (id, style)
    };
    let refinement: StyleRefinement = serde_json::from_value(style).map_err(|e| format!("invalid style: {e}"))?;
    with_window(cx, |window, cx| {
        window.with_inspector_state::<DivInspectorState, _>(Some(&id), cx, |state, _| {
            if let Some(state) = state {
                state.base_style = Box::new(refinement);
            }
        });
        window.refresh();
    })
    .ok_or("no window")?;
    shared.borrow_mut().live.current = index;
    Ok(())
}

fn step(cx: &mut App, delta: i64) {
    let (count, current) = {
        let s = probe(cx);
        let s = s.borrow();
        (s.live.variants.len() as i64, s.live.current)
    };
    if count == 0 {
        step_slot(cx, delta);
        return;
    }
    let next = match current {
        Some(i) => (i as i64 + delta).rem_euclid(count),
        None if delta > 0 => 0,
        None => count - 1,
    };
    let _ = apply(cx, Some(next as usize));
}

fn accept(cx: &mut App) {
    let shared = probe(cx);
    let mut s = shared.borrow_mut();
    if s.live.variants.is_empty() {
        if let Some(slot) = s.active_slot.and_then(|i| s.slots.get(i)) {
            let decision = json!({
                "slot": slot.id,
                "index": slot.current,
                "label": slot.labels.get(slot.current),
                "source": slot.source,
            });
            s.live.decision = Some(decision);
            s.live.seq += 1;
            return;
        }
    }
    let index = s.live.current;
    let decision = json!({
        "index": index,
        "label": index.and_then(|i| s.live.variants.get(i)).map(|v| v.label.clone()),
        "patch": index.and_then(|i| s.live.variants.get(i)).map(|v| v.patch.clone()),
        "source": s.selection.source,
        "original": s.selection.original,
    });
    s.live.decision = Some(decision);
    s.live.seq += 1;
    drop(s);
    cx.refresh_windows();
}

// ─── requests ──────────────────────────────────────────────────────────────

async fn handle(req: Request, cx: &mut AsyncApp) {
    let route = req.path.split('?').next().unwrap_or("").to_string();
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    let reply = match (req.method.as_str(), route.as_str()) {
        ("GET", "/status") => cx.up(status).unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/resize") => {
            let (w, h) = (body["width"].as_f64(), body["height"].as_f64());
            match (w, h) {
                (Some(w), Some(h)) => {
                    let done = cx.up(|cx| with_window(cx, |window, _| window.resize(gpui::size(px(w as f32), px(h as f32)))));
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                    match done {
                        Ok(Some(())) => Reply::json(200, &json!({ "ok": true })),
                        _ => Reply::error(409, "no window"),
                    }
                }
                _ => Reply::error(400, "body needs width and height"),
            }
        }
        ("POST", "/appearance") => match body["dark"].as_bool() {
            Some(dark) => {
                let r = cx.up(|cx| {
                    let shared = probe(cx);
                    let hook = shared.borrow_mut().on_appearance.take();
                    let Some(hook) = hook else { return false };
                    hook(dark, cx);
                    let mut s = shared.borrow_mut();
                    s.on_appearance = Some(hook);
                    s.dark = Some(dark);
                    drop(s);
                    cx.refresh_windows();
                    true
                });
                cx.background_executor().timer(Duration::from_millis(250)).await;
                match r {
                    Ok(true) => Reply::json(200, &json!({ "ok": true, "dark": dark })),
                    _ => Reply::error(409, "the app did not pass Options::on_appearance"),
                }
            }
            None => Reply::error(400, "body needs dark: true|false"),
        },
        ("GET", "/capture") => {
            let r = cx.up(|cx| with_window(cx, |window, _| crate::capture::window_png(window)));
            match r {
                Ok(Some(Ok(png))) => Reply::png(png),
                Ok(Some(Err(e))) => Reply::error(500, &e),
                _ => Reply::error(409, "no window"),
            }
        }
        ("POST", "/inspector") => {
            let open = body["open"].as_bool();
            match cx.up(|cx| set_inspector(cx, open)) {
                Ok(open) => {
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    Reply::json(200, &json!({ "ok": true, "open": open }))
                }
                Err(_) => Reply::error(500, "app gone"),
            }
        }
        ("POST", "/pick") => match (body["x"].as_f64(), body["y"].as_f64()) {
            (Some(x), Some(y)) => {
                let _ = cx.up(|cx| set_inspector(cx, Some(true)));
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let moved = cx.up(|cx| pointer(cx, x as f32, y as f32, false));
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let clicked = cx.up(|cx| pointer(cx, x as f32, y as f32, true));
                // The picked element records its state on the next frame.
                cx.background_executor().timer(Duration::from_millis(250)).await;
                match (moved, clicked) {
                    (Ok(Ok(())), Ok(Ok(()))) => cx.up(selection).unwrap_or_else(|_| Reply::error(500, "app gone")),
                    (Ok(Err(e)), _) | (_, Ok(Err(e))) => Reply::error(500, &e),
                    _ => Reply::error(500, "app gone"),
                }
            }
            _ => Reply::error(400, "body needs x and y in window points"),
        },
        #[cfg(feature = "snapshot")]
        ("GET", "/snapshot") => snapshot(cx).await,
        #[cfg(not(feature = "snapshot"))]
        ("GET", "/snapshot") => Reply::error(
            501,
            "this probe was built without the `snapshot` feature; it needs gpui-pre with the new-horizon-code-repo/gpui patch",
        ),
        ("GET", "/slots") => cx.up(slots).unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/slot") => {
            let id = body["id"].as_str().map(str::to_string);
            let index = body["index"].as_u64().map(|i| i as usize);
            match cx.up(|cx| arm_slot(cx, id.as_deref(), index)) {
                Ok(Ok(v)) => Reply::json(200, &v),
                Ok(Err(e)) => Reply::error(404, &e),
                Err(_) => Reply::error(500, "app gone"),
            }
        }
        ("GET", "/selection") => cx.up(selection).unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/variants") => cx
            .up(|cx| set_variants(cx, &body))
            .unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/apply") => {
            let index = body["index"].as_u64().map(|i| i as usize);
            match cx.up(|cx| apply(cx, index)) {
                Ok(Ok(())) => Reply::json(200, &json!({ "ok": true, "index": index })),
                Ok(Err(e)) => Reply::error(409, &e),
                Err(_) => Reply::error(500, "app gone"),
            }
        }
        ("GET", "/decision") => cx
            .up(|cx| {
                let s = probe(cx);
                let s = s.borrow();
                Reply::json(200, &json!({ "seq": s.live.seq, "decision": s.live.decision }))
            })
            .unwrap_or_else(|_| Reply::error(500, "app gone")),
        _ => Reply::error(404, "unknown route"),
    };
    let _ = req.reply.send(reply);
}

fn status(cx: &mut App) -> Reply {
    let shared = probe(cx);
    let windows: Vec<Value> = cx
        .windows()
        .into_iter()
        .filter_map(|h| {
            h.update(cx, |_, window, _| {
                let size = window.viewport_size();
                json!({
                    "id": h.window_id().as_u64(),
                    "title": window.window_title(),
                    "width": f32::from(size.width),
                    "height": f32::from(size.height),
                    "scale": window.scale_factor(),
                    "osAppearance": format!("{:?}", window.appearance()),
                })
            })
            .ok()
        })
        .collect();
    let s = shared.borrow();
    Reply::json(
        200,
        &json!({
            "probe": env!("CARGO_PKG_VERSION"),
            "port": s.port,
            "windows": windows,
            "appearanceHook": s.on_appearance.is_some(),
            "dark": s.dark,
            "inspectorOpen": s.inspector_open,
            "selection": s.selection.source,
            "variants": s.live.variants.len(),
            "slots": s.slots.len(),
        }),
    )
}

fn selection(cx: &mut App) -> Reply {
    let picking = with_window(cx, |window, cx| window.is_inspector_picking(cx)).unwrap_or(false);
    let s = probe(cx);
    let s = s.borrow();
    Reply::json(
        200,
        &json!({
            "seq": s.selection.seq,
            "picking": picking,
            "source": if picking { None } else { s.selection.source.clone() },
            "style": s.selection.original,
            "bounds": s.selection.bounds,
        }),
    )
}

fn set_variants(cx: &mut App, body: &Value) -> Reply {
    let Some(list) = body["variants"].as_array() else {
        return Reply::error(400, "body needs variants: [{label, style}]");
    };
    let variants: Vec<Variant> = list
        .iter()
        .enumerate()
        .map(|(i, v)| Variant {
            label: v["label"].as_str().map(str::to_string).unwrap_or_else(|| format!("Variant {}", i + 1)),
            patch: expand_shorthand(v["style"].clone()),
        })
        .collect();
    // Reject a patch that does not make a valid style before showing any.
    {
        let s = probe(cx);
        let s = s.borrow();
        let Some(original) = s.selection.original.clone() else {
            return Reply::error(409, "nothing is picked");
        };
        for v in &variants {
            let mut style = original.clone();
            merge(&mut style, &v.patch);
            if let Err(e) = serde_json::from_value::<StyleRefinement>(style) {
                return Reply::error(400, &format!("{}: invalid style: {e}", v.label));
            }
        }
    }
    let count = variants.len();
    {
        let s = probe(cx);
        let mut s = s.borrow_mut();
        s.live.variants = variants;
        s.live.current = None;
        s.live.decision = None;
    }
    let shown = if count > 0 { apply(cx, Some(0)).err() } else { None };
    Reply::json(200, &json!({ "ok": true, "count": count, "warning": shown }))
}

// ─── snapshot ──────────────────────────────────────────────────────────────

#[cfg(feature = "snapshot")]
async fn snapshot(cx: &mut AsyncApp) -> Reply {
    let requested = cx.up(|cx| with_window(cx, |window, _| window.request_ui_snapshot()));
    if !matches!(requested, Ok(Some(()))) {
        return Reply::error(500, "no window");
    }
    for _ in 0..150 {
        cx.background_executor().timer(Duration::from_millis(20)).await;
        if let Ok(Some(Some(snap))) = cx.up(|cx| with_window(cx, |window, _| window.take_ui_snapshot())) {
            return Reply::json(200, &snapshot_json(&snap));
        }
    }
    Reply::error(504, "the window drew no frame in 3 seconds")
}

#[cfg(feature = "snapshot")]
fn snapshot_json(snap: &gpui::UiSnapshot) -> Value {
    use gpui::{Bounds, Pixels, UiNodeKind};

    let rect = |b: &Bounds<Pixels>| json!([f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)]);
    let own_file = file!().replace('\\', "/");
    // The probe's own inspector panel is not part of the app.
    let is_probe = |loc: &std::panic::Location| loc.file().replace('\\', "/").ends_with(&own_file);

    let len = |l: gpui::AbsoluteLength| f32::from(l.to_pixels(snap.rem_size));

    let mut index = vec![None; snap.nodes.len()];
    let mut nodes = Vec::new();
    for (i, node) in snap.nodes.iter().enumerate() {
        let parent = match node.parent {
            Some(p) => match index[p] {
                Some(p) => Some(p),
                None => continue,
            },
            None => None,
        };
        if node.source.is_some_and(is_probe) {
            continue;
        }
        index[i] = Some(nodes.len());
        let mut v = json!({
            "parent": parent,
            "source": node.source.map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            "id": node.id.as_ref().map(|id| id.to_string()),
            "bounds": rect(&node.bounds),
            "clip": rect(&node.clip),
        });
        let extra = match &node.kind {
            UiNodeKind::Element { style, content_size, states, interaction } => json!({
                "kind": "element",
                "states": without_nulls(json!({
                    "hover": states.hover.as_ref().and_then(|r| serde_json::to_value(r).ok()),
                    "active": states.active.as_ref().and_then(|r| serde_json::to_value(r).ok()),
                    "focus": states.focus.as_ref().and_then(|r| serde_json::to_value(r).ok()),
                    "focusVisible": states.focus_visible.as_ref().and_then(|r| serde_json::to_value(r).ok()),
                })),
                "input": {
                    "clickable": interaction.clickable,
                    "focusable": interaction.focusable,
                    "focused": interaction.focused,
                    "hovered": interaction.hovered,
                    "tooltip": interaction.tooltip,
                    "tabIndex": interaction.tab_index,
                    "keyContext": interaction.key_context,
                    "actions": interaction.actions,
                    "label": interaction.label.as_ref().map(|l| l.to_string()),
                },
                "content": [f32::from(content_size.width), f32::from(content_size.height)],
                "style": without_nulls(json!({
                    "background": serde_json::to_value(&style.background).ok(),
                    "borderColor": serde_json::to_value(&style.border_color).ok(),
                    "borderWidths": [len(style.border_widths.top), len(style.border_widths.right), len(style.border_widths.bottom), len(style.border_widths.left)],
                    "cornerRadii": [len(style.corner_radii.top_left), len(style.corner_radii.top_right), len(style.corner_radii.bottom_right), len(style.corner_radii.bottom_left)],
                    "boxShadow": serde_json::to_value(&style.box_shadow).ok(),
                    "opacity": style.opacity,
                    "overflow": serde_json::to_value(&style.overflow).ok(),
                    "text": serde_json::to_value(&style.text).ok(),
                })),
            }),
            UiNodeKind::Animation { duration, oneshot, easing } => json!({
                "kind": "animation",
                "durationMs": duration.as_millis() as u64,
                "oneshot": oneshot,
                "easing": easing,
            }),
            UiNodeKind::Text { text, shown, style, font_size, line_height, size, runs } => json!({
                "kind": "text",
                "text": text.to_string(),
                "shown": shown.to_string(),
                "fontSize": f32::from(*font_size),
                "lineHeight": f32::from(*line_height),
                "size": [f32::from(size.width), f32::from(size.height)],
                "fontFamily": style.font_family.to_string(),
                "fontWeight": style.font_weight.0,
                "runs": runs.iter().map(|(len, color, bg)| json!({ "len": len, "color": color, "background": bg })).collect::<Vec<_>>(),
            }),
        };
        if let (Value::Object(v), Value::Object(extra)) = (&mut v, extra) {
            v.extend(extra);
        }
        nodes.push(v);
    }
    json!({
        "viewport": [f32::from(snap.viewport.width), f32::from(snap.viewport.height)],
        "scale": snap.scale_factor,
        "rem": f32::from(snap.rem_size),
        "nodes": nodes,
    })
}

// ─── structure slots ───────────────────────────────────────────────────────

pub(crate) fn slot(
    id: &'static str,
    labels: &[&'static str],
    entity: EntityId,
    source: &'static std::panic::Location<'static>,
    cx: &mut App,
) -> usize {
    if labels.is_empty() || !cx.has_global::<Probe>() {
        return 0;
    }
    let shared = probe(cx);
    let mut s = shared.borrow_mut();
    let index = match s.slots.iter().position(|slot| slot.id == id) {
        Some(i) => i,
        None => {
            s.slots.push(Slot {
                id,
                labels: labels.to_vec(),
                current: 0,
                source: format!("{}:{}:{}", source.file(), source.line(), source.column()),
                entities: Vec::new(),
            });
            s.slots.len() - 1
        }
    };
    if s.active_slot.is_none() {
        s.active_slot = Some(index);
    }
    let slot = &mut s.slots[index];
    slot.labels = labels.to_vec();
    if !slot.entities.contains(&entity) {
        slot.entities.push(entity);
    }
    slot.current.min(labels.len() - 1)
}

fn set_slot(cx: &mut App, index: usize, current: usize) {
    let entities = {
        let shared = probe(cx);
        let mut s = shared.borrow_mut();
        let Some(slot) = s.slots.get_mut(index) else { return };
        slot.current = current.min(slot.labels.len().saturating_sub(1));
        slot.entities.clone()
    };
    for entity in entities {
        cx.notify(entity);
    }
    cx.refresh_windows();
}

fn step_slot(cx: &mut App, delta: i64) {
    let target = {
        let shared = probe(cx);
        let s = shared.borrow();
        s.active_slot.and_then(|i| s.slots.get(i).map(|slot| (i, slot.current as i64, slot.labels.len() as i64)))
    };
    if let Some((i, current, count)) = target.filter(|t| t.2 > 0) {
        set_slot(cx, i, (current + delta).rem_euclid(count) as usize);
    }
}

fn slot_json(slot: &Slot, active: bool) -> Value {
    json!({
        "id": slot.id,
        "labels": slot.labels,
        "current": slot.current,
        "source": slot.source,
        "active": active,
    })
}

fn slots(cx: &mut App) -> Reply {
    let shared = probe(cx);
    let s = shared.borrow();
    let list: Vec<Value> = s.slots.iter().enumerate().map(|(i, slot)| slot_json(slot, s.active_slot == Some(i))).collect();
    Reply::json(200, &json!({ "slots": list }))
}

/// Make a slot the keyboard target, and optionally show one of its arms.
fn arm_slot(cx: &mut App, id: Option<&str>, current: Option<usize>) -> Result<Value, String> {
    let index = {
        let shared = probe(cx);
        let mut s = shared.borrow_mut();
        let index = match id {
            Some(id) => s.slots.iter().position(|slot| slot.id == id).ok_or_else(|| format!("no slot `{id}` has rendered yet"))?,
            None => s.active_slot.ok_or("no slot has rendered yet")?,
        };
        if let Some(n) = current {
            let count = s.slots[index].labels.len();
            if n >= count {
                return Err(format!("slot `{}` has {count} arms", s.slots[index].id));
            }
        }
        s.active_slot = Some(index);
        index
    };
    if let Some(n) = current {
        set_slot(cx, index, n);
    }
    let shared = probe(cx);
    let s = shared.borrow();
    Ok(slot_json(&s.slots[index], true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nulls_and_empty_objects_are_stripped() {
        let v = without_nulls(json!({ "a": null, "b": { "c": null }, "d": { "e": 1, "f": null } }));
        assert_eq!(v, json!({ "d": { "e": 1 } }));
    }

    #[test]
    fn patches_merge_and_null_removes_a_key() {
        let mut base = json!({ "opacity": 1.0, "text": { "color": "#ffffffff", "font_size": "14px" } });
        merge(&mut base, &json!({ "opacity": null, "text": { "color": "#000000ff" } }));
        assert_eq!(base, json!({ "text": { "color": "#000000ff", "font_size": "14px" } }));
    }

    #[test]
    fn a_background_shorthand_becomes_a_real_style_refinement() {
        let mut style = json!({});
        merge(&mut style, &expand_shorthand(json!({ "background": "#ff0000ff", "opacity": 0.5 })));
        let refinement: StyleRefinement = serde_json::from_value(style).expect("a valid StyleRefinement");
        assert_eq!(refinement.opacity, Some(0.5));
        let round = serde_json::to_value(&refinement).unwrap();
        assert_eq!(round["background"]["Color"]["solid"], "#ff0000ff");
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn snapshot_json_drops_the_probe_panel_and_remaps_parents() {
        use gpui::{point, px, size, Bounds, Style, UiNode, UiNodeKind, UiSnapshot};

        let b = Bounds { origin: point(px(0.), px(0.)), size: size(px(100.), px(40.)) };
        let element = |parent: Option<usize>, source| UiNode {
            parent,
            source,
            id: None,
            bounds: b,
            clip: b,
            kind: UiNodeKind::Element {
                style: Box::new(Style::default()),
                content_size: b.size,
                states: Default::default(),
                interaction: Default::default(),
            },
        };
        // A Location from this file stands in for the probe's own inspector panel.
        let here = std::panic::Location::caller();
        let snap = UiSnapshot {
            nodes: vec![element(None, None), element(None, Some(here)), element(Some(1), None), element(Some(0), None)],
            viewport: b.size,
            scale_factor: 2.0,
            rem_size: px(16.),
        };
        let v = snapshot_json(&snap);
        let nodes = v["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "{v:#}");
        assert_eq!(nodes[1]["parent"], 0);
        assert_eq!(nodes[0]["style"]["borderWidths"], json!([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(v["rem"], 16.0);
    }
}
