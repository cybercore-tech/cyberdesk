//! CYBERGRID theme -> `:root{}` CSS custom properties, from the shared
//! `cybercore` schema (v2). `CYBERGRID_THEME` env picks the active one.

use cybercore::schema::{self, Palette};

pub fn css() -> String {
    css_for(schema::load().active_theme())
}

pub fn css_for(p: &Palette) -> String {
    format!(
        ":root{{\
--bg:#{bg};--fg:#{white};\
--acid:#{acid};--pink:#{pink};--purple:#{purple};--cyan:#{cyan};\
--orange:#{orange};--red:#{red};\
--panel:#{panel};--line:#{line};--muted:#{muted};\
}}",
        bg = p.bg,
        white = p.white,
        acid = p.acid_green,
        pink = p.hot_pink,
        purple = p.purple,
        cyan = p.cyan,
        orange = p.orange,
        red = p.red,
        panel = p.panel,
        line = p.line,
        muted = p.muted,
    )
}

pub fn active_name() -> &'static str {
    schema::load().active.as_str()
}
