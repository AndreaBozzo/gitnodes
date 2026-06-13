#[cfg(feature = "hydrate")]
pub(crate) fn render_brain_mermaid() {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(value) = js_sys::Reflect::get(window.as_ref(), &"renderBrainMermaid".into()) else {
        return;
    };
    let Ok(render) = value.dyn_into::<js_sys::Function>() else {
        return;
    };
    let _ = render.call0(window.as_ref());
}
