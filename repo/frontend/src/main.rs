mod api;
mod app;
mod auth;
mod components;
mod offline;
mod pages;
mod routes;
mod types;

fn main() {
    let root = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app-root"))
        .expect("#app-root element must exist in index.html");
    yew::Renderer::<app::App>::with_root(root).render();
}
