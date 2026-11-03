//! A small GPUI window wired to the probe: `cargo run --example demo`.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Rgba, Window, WindowBounds, WindowOptions};

static DARK: AtomicBool = AtomicBool::new(true);

fn pick(dark: u32, light: u32) -> Rgba {
    rgb(if DARK.load(Ordering::Relaxed) { dark } else { light })
}
fn ground() -> Rgba {
    pick(0x111214, 0xf4f4f5)
}
fn card() -> Rgba {
    pick(0x1c1d21, 0xffffff)
}
fn text() -> Rgba {
    pick(0xececee, 0x18181b)
}
fn faint() -> Rgba {
    pick(0x7c7c86, 0xa1a1aa)
}
fn accent() -> Rgba {
    rgb(0x7c3aed)
}

struct Demo;

impl Render for Demo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let item = |title: &'static str, meta: &'static str| {
            div()
                .p_4()
                .rounded_lg()
                .bg(card())
                .border_l_4()
                .border_color(accent())
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_size(px(15.)).text_color(text()).child(title))
                .child(div().text_size(px(10.)).text_color(faint()).child(meta))
        };
        div()
            .size_full()
            .bg(ground())
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_size(px(22.)).text_color(text()).child("Builds"))
            .child(item("Release 0.4.2", "Finished 2 minutes ago"))
            .child(item("Nightly", "Running for 14 minutes"))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |_, cx| cx.new(|_| Demo),
        )
        .unwrap();
        impeccable_gpui_probe::attach(
            cx,
            impeccable_gpui_probe::Options {
                on_appearance: Some(Box::new(|dark, _| DARK.store(dark, Ordering::Relaxed))),
                ..Default::default()
            },
        );
        cx.activate(true);
    });
}
