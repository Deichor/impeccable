//! A small GPUI window wired to the probe: `cargo run --example demo`
//! (gpui 0.2), or `cargo run --example demo --no-default-features --features snapshot`
//! (gpui-pre with the snapshot patch).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{div, prelude::*, px, rgb, size, App, Bounds, Context, Rgba, Window, WindowBounds, WindowOptions};

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A structure variant: switch with ctrl-alt-left/right while no style variants are shown.
        let layout = impeccable_gpui_probe::slot("build-item", &["Card", "Row"], cx);
        let item = move |title: &'static str, meta: &'static str| {
            if layout == 1 {
                return div()
                    .py_2()
                    .border_b_1()
                    .border_color(faint())
                    .flex()
                    .justify_between()
                    .child(div().text_size(px(15.)).text_color(text()).child(title))
                    .child(div().text_size(px(13.)).text_color(text()).child(meta));
            }
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
            .child(
                div()
                    .w(px(160.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .text_color(text())
                    .child("Deploy to eu-west-1 failed after 3 retries"),
            )
    }
}

#[cfg(feature = "gpui-0-2")]
fn application() -> gpui::Application {
    gpui::Application::new()
}

#[cfg(feature = "gpui-pre")]
fn application() -> gpui::Application {
    gpui_platform::application()
}

fn main() {
    application().run(|cx: &mut App| {
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
