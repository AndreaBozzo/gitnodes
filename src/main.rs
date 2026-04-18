use leptos::*;

mod app;
mod knowledge;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <app::App/> });
}
