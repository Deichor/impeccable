use gpui::*;

use crate::theme;

pub fn root() -> impl IntoElement {
    div().font_family(theme::SANS).bg(theme::surface()).child(card())
}

fn card() -> impl IntoElement {
    div()
        .p_4()
        .rounded_lg()
        .bg(theme::surface())
        .border_l_4()
        .border_color(theme::accent())
        .child(div().text_color(theme::faint()).text_size(px(10.)).child("Updated 2m ago"))
}

// A selected row: conditional styles are state, not a side tab.
fn row(selected: bool) -> impl IntoElement {
    div().rounded_md().when(selected, |d| d.border_l_2().border_color(theme::accent()))
}
