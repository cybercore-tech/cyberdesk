//! CYBERGRID theme -> `:root{}` CSS custom properties, from the shared
//! `cybercore` schema (v2). `CYBERGRID_THEME` env picks the compile-time
//! default; the in-app switcher overrides it per browser via a cookie
//! (see `routes::theme_css`).

use cybercore::schema::{self, Palette};

pub fn css() -> String {
    css_for(schema::load().active_theme())
}

/// `:root{}` for a theme chosen by slug, or `None` if the slug is unknown.
pub fn css_for_slug(slug: &str) -> Option<String> {
    schema::load().theme(slug).map(css_for)
}

/// Every theme slug the schema defines, in sorted order.
pub fn names() -> Vec<&'static str> {
    schema::load().theme_names().collect()
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
