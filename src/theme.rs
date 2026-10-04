//! CYBERGRID theme adapters backed by the shared Cybercore theme engine.

use cybercore::theme::{Appearance, ThemeCatalog};

pub fn css() -> String {
    if let Ok(catalog) = ThemeCatalog::load() {
        if let Some(entry) = catalog.get(catalog.active_id()) {
            return entry.document.to_css(catalog.active_appearance());
        }
    }
    let schema = cybercore::schema::load();
    let fallback = cybercore::theme::ThemeDocument::new(
        schema.active.clone(),
        schema.active.clone(),
        schema.active_theme().clone(),
    );
    fallback.to_css(Appearance::Dark)
}

pub fn css_for_slug(slug: &str) -> Option<String> {
    let catalog = ThemeCatalog::load().ok()?;
    catalog
        .get(slug)
        .map(|entry| entry.document.to_css(catalog.active_appearance()))
}

pub fn names() -> Vec<String> {
    ThemeCatalog::load()
        .map(|catalog| catalog.iter().map(|(id, _)| id.to_string()).collect())
        .unwrap_or_default()
}

pub fn active_name() -> String {
    ThemeCatalog::load()
        .map(|catalog| catalog.active_id().to_string())
        .unwrap_or_else(|_| cybercore::schema::load().active.clone())
}

pub fn select(slug: &str) -> bool {
    ThemeCatalog::load()
        .and_then(|mut catalog| catalog.select(slug))
        .is_ok()
}
