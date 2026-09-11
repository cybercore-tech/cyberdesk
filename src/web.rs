//! MiniJinja environment (templates embedded) + a small page renderer.

use axum::response::Html;
use minijinja::{context, Environment, Value};

macro_rules! tpl {
    ($env:expr, $name:literal) => {
        $env.add_template($name, include_str!(concat!("../templates/", $name)))
            .expect(concat!("template ", $name, " failed to parse"));
    };
}

pub fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    tpl!(env, "base.html");
    tpl!(env, "portal.html");
    tpl!(env, "view.html");
    tpl!(env, "edit.html");
    tpl!(env, "search.html");
    tpl!(env, "list.html");
    tpl!(env, "templates.html");
    tpl!(env, "folders.html");
    tpl!(env, "lint.html");
    tpl!(env, "repo.html");
    tpl!(env, "api.html");
    env
}

#[derive(Clone)]
pub struct Renderer {
    env: std::sync::Arc<Environment<'static>>,
    pub css: String,
    pub theme: String,
    pub site_name: String,
}

impl Renderer {
    pub fn new(site_name: String) -> Self {
        Self {
            env: std::sync::Arc::new(build_env()),
            css: crate::theme::css(),
            theme: crate::theme::active_name().to_string(),
            site_name,
        }
    }

    pub fn page(&self, name: &str, ctx: Value, nav: &str) -> Result<Html<String>, minijinja::Error> {
        let t = self.env.get_template(name)?;
        let merged = context! { ..ctx, ..context! {
            css => self.css, theme => self.theme, nav => nav, site_name => self.site_name,
        }};
        Ok(Html(t.render(merged)?))
    }
}
