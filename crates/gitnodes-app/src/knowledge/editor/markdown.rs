use leptos::prelude::*;

use gitnodes_domain::TargetRef;

/// Edit / preview toggle for the markdown body.
#[component]
pub(super) fn MarkdownPreview(
    active_target: TargetRef,
    node_type: Signal<String>,
    body: RwSignal<String>,
    file_path: Signal<Option<String>>,
    config: gitnodes_domain::BrainConfig,
) -> impl IntoView {
    let show_preview = RwSignal::new(false);
    let target_for_preview = active_target.clone();
    let target_for_upload = active_target.clone();
    let preview_html = Memo::new(move |_| {
        let b = body.get();
        let target_config: gitnodes_domain::TargetConfig = target_for_preview.clone().into();
        match (file_path.get(), target_config) {
            (Some(path), cfg) => crate::markdown::render_for_file(&b, &path, &cfg),
            _ => crate::markdown::render(&b),
        }
    });
    let upload_status = RwSignal::new(String::new());
    let dragging = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    {
        Effect::new(move |_| {
            if show_preview.get() {
                let _ = preview_html.get(); // track preview_html updates
                crate::knowledge::mermaid::render_brain_mermaid();
            }
        });
    }

    view! {
        <div>
            <div class="flex items-center justify-between mb-1">
                <label class="text-[10px] uppercase tracking-widest text-slate-500">
                    {move || {
                        let t = node_type.get();
                        config
                            .lookup(&t)
                            .and_then(|s| s.body_label.clone())
                            .unwrap_or_else(|| "Description".to_string())
                    }}
                </label>
                <button
                    class="text-[10px] uppercase tracking-widest text-slate-400 hover:text-teal-300 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1"
                    on:click=move |_| show_preview.update(|v| *v = !*v)
                >
                    {move || if show_preview.get() { "Edit" } else { "Preview" }}
                </button>
            </div>
            <Show
                when=move || show_preview.get()
                fallback=move || {
                    let drop_target = target_for_upload.clone();
                    #[cfg(not(feature = "hydrate"))]
                    let _ = &drop_target;
                    view! {
                        <textarea
                        class="w-full px-3 py-2 rounded-md bg-slate-800 border text-slate-100 text-sm focus:border-teal-400 focus:outline-none min-h-[180px] resize-y font-mono transition-colors"
                        class=("bg-slate-800", move || !dragging.get())
                        class=("border-slate-700", move || !dragging.get())
                        class=("bg-teal-500/10", move || dragging.get())
                        class=("border-teal-400", move || dragging.get())
                        placeholder="Write the main content here (Markdown supported). Drop images to upload."
                        prop:value=move || body.get()
                        on:input=move |ev| body.set(event_target_value(&ev))
                        on:dragover=move |ev| {
                            ev.prevent_default();
                            dragging.set(true);
                        }
                        on:dragleave=move |_| dragging.set(false)
                        on:drop=move |ev| {
                            ev.prevent_default();
                            dragging.set(false);
                            #[cfg(feature = "hydrate")]
                            handle_image_drop(
                                ev,
                                body,
                                upload_status,
                                file_path.get_untracked(),
                                drop_target.clone(),
                            );
                            #[cfg(not(feature = "hydrate"))]
                            { let _ = (upload_status, &body, file_path); }
                        }
                        />
                    }
                }
            >
                <div class="px-3 py-2 rounded-md bg-slate-950 border border-slate-800 min-h-[180px]">
                    {move || {
                        let b = body.get();
                        if b.trim().is_empty() {
                            view! {
                                <div class="text-slate-600 text-xs italic">"Nothing to preview yet."</div>
                            }.into_any()
                        } else {
                            view! {
                                <article
                                    class="prose prose-invert max-w-prose"
                                    inner_html=preview_html.get()
                                ></article>
                            }.into_any()
                        }
                    }}
                </div>
            </Show>
            <Show when=move || !upload_status.get().is_empty()>
                <p class="text-[10px] text-teal-300 mt-1">{move || upload_status.get()}</p>
            </Show>
        </div>
    }
}

/// Upload every image file from a drop event, inserting a markdown image tag
/// into the body for each one as it completes. Non-image files are skipped
/// silently; per-file errors surface in `status` but don't abort siblings.
#[cfg(feature = "hydrate")]
fn handle_image_drop(
    ev: leptos::ev::DragEvent,
    body: RwSignal<String>,
    status: RwSignal<String>,
    file_path: Option<String>,
    active_target: TargetRef,
) {
    use crate::api::upload_asset;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let Some(dt) = ev.data_transfer() else {
        return;
    };
    let Some(files) = dt.files() else {
        return;
    };
    let count = files.length();
    if count == 0 {
        return;
    }
    for i in 0..count {
        let Some(file) = files.get(i) else { continue };
        let mime = file.type_();
        if !mime.starts_with("image/") {
            continue;
        }
        let filename = file.name();
        let file_for_task = file.clone();
        let file_path_for_task = file_path.clone();
        let target_for_task = active_target.clone();
        status.set(format!("Uploading {filename}…"));
        leptos::task::spawn_local(async move {
            let buf_promise = file_for_task.array_buffer();
            let buf = match JsFuture::from(buf_promise).await {
                Ok(v) => v,
                Err(_) => {
                    status.set(format!("Read failed: {filename}"));
                    return;
                }
            };
            let Ok(array) = buf.dyn_into::<js_sys::ArrayBuffer>() else {
                status.set(format!("Read failed: {filename}"));
                return;
            };
            let bytes = js_sys::Uint8Array::new(&array).to_vec();
            let alt = strip_ext(&filename);
            match upload_asset(target_for_task, filename.clone(), bytes).await {
                Ok(path) => {
                    let link = crate::markdown::repo_relative_link_target(
                        file_path_for_task.as_deref(),
                        &path,
                    );
                    let snippet = format!("\n\n![{alt}]({link})\n");
                    body.update(|b| b.push_str(&snippet));
                    status.set(format!("Uploaded {path}"));
                }
                Err(e) => {
                    status.set(format!("Upload failed ({filename}): {e}"));
                }
            }
        });
    }
}

#[cfg(feature = "hydrate")]
fn strip_ext(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => filename.to_string(),
    }
}
