use gpui::{rgb, Rgba};

pub const SANS: &str = "Inter";

fn pick(dark: u32, light: u32) -> Rgba {
    rgb(if crate::is_dark() { dark } else { light })
}

pub fn surface() -> Rgba {
    pick(0x161618, 0xffffff)
}
pub fn faint() -> Rgba {
    pick(0x7c7c86, 0xb4b4bb)
}
pub fn accent() -> Rgba {
    rgb(0x7c3aed)
}
