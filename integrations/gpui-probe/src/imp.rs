use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, rgb, AnyElement, App, AsyncApp, Context, DivInspectorState, Empty, Global,
    Inspector, InspectorElementId, KeyBinding, SharedString, StyleRefinement, Window,
};
use serde_json::{json, Map, Value};

use crate::server::{self, Reply, Request};
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
            let _ = apply(cx, None);
        })
    });

    cx.set_inspector_renderer(Box::new(render_panel));
    cx.register_inspector_element(record_selection);

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
        ("GET", "/status") => cx.update(status).unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/resize") => {
            let (w, h) = (body["width"].as_f64(), body["height"].as_f64());
            match (w, h) {
                (Some(w), Some(h)) => {
                    let done = cx.update(|cx| with_window(cx, |window, _| window.resize(gpui::size(px(w as f32), px(h as f32)))));
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
                let r = cx.update(|cx| {
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
            let r = cx.update(|cx| with_window(cx, |window, _| crate::capture::window_png(window)));
            match r {
                Ok(Some(Ok(png))) => Reply::png(png),
                Ok(Some(Err(e))) => Reply::error(500, &e),
                _ => Reply::error(409, "no window"),
            }
        }
        ("POST", "/inspector") => {
            let open = body["open"].as_bool();
            match cx.update(|cx| set_inspector(cx, open)) {
                Ok(open) => {
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    Reply::json(200, &json!({ "ok": true, "open": open }))
                }
                Err(_) => Reply::error(500, "app gone"),
            }
        }
        ("POST", "/pick") => match (body["x"].as_f64(), body["y"].as_f64()) {
            (Some(x), Some(y)) => {
                let _ = cx.update(|cx| set_inspector(cx, Some(true)));
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let moved = cx.update(|cx| pointer(cx, x as f32, y as f32, false));
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let clicked = cx.update(|cx| pointer(cx, x as f32, y as f32, true));
                // The picked element records its state on the next frame.
                cx.background_executor().timer(Duration::from_millis(250)).await;
                match (moved, clicked) {
                    (Ok(Ok(())), Ok(Ok(()))) => cx.update(selection).unwrap_or_else(|_| Reply::error(500, "app gone")),
                    (Ok(Err(e)), _) | (_, Ok(Err(e))) => Reply::error(500, &e),
                    _ => Reply::error(500, "app gone"),
                }
            }
            _ => Reply::error(400, "body needs x and y in window points"),
        },
        ("GET", "/selection") => cx.update(selection).unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/variants") => cx
            .update(|cx| set_variants(cx, &body))
            .unwrap_or_else(|_| Reply::error(500, "app gone")),
        ("POST", "/apply") => {
            let index = body["index"].as_u64().map(|i| i as usize);
            match cx.update(|cx| apply(cx, index)) {
                Ok(Ok(())) => Reply::json(200, &json!({ "ok": true, "index": index })),
                Ok(Err(e)) => Reply::error(409, &e),
                Err(_) => Reply::error(500, "app gone"),
            }
        }
        ("GET", "/decision") => cx
            .update(|cx| {
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
