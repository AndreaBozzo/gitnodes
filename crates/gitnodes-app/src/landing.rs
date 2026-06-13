use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::api::AppConfig;

#[component]
pub fn Landing() -> impl IntoView {
    let query = use_query_map();

    let app_config = use_context::<Resource<Result<AppConfig, crate::api::ApiError>>>();

    let resolved_config = Memo::new(move |_| {
        app_config
            .and_then(|resource| resource.get())
            .and_then(Result::ok)
    });

    let brand_name = move || {
        resolved_config.with(|config| {
            config
                .as_ref()
                .map(|c| c.brand.name.clone())
                .unwrap_or_else(|| "Brain".to_string())
        })
    };

    let brand_org = move || {
        resolved_config.with(|config| {
            config
                .as_ref()
                .map(|c| c.brand.org_label.clone())
                .filter(|label| !label.trim().is_empty())
                .or_else(|| config.as_ref().map(|c| c.target.org.clone()))
                .unwrap_or_else(|| "configured".to_string())
        })
    };

    let target_label = move || {
        resolved_config.with(|config| {
            config
                .as_ref()
                .map(|c| format!("{}/{}/{}", c.target.org, c.target.repo, c.target.branch))
                .unwrap_or_else(|| "configured GitHub repository".to_string())
        })
    };

    let login_org =
        move || resolved_config.with(|config| config.as_ref().and_then(|c| c.login_org.clone()));

    let error_msg = move || {
        let params = query.get();
        match params.get_str("error") {
            Some("not_login_org_member") | Some("not_org_member") => Some(format!(
                "Access denied — you must be a member of the {} GitHub organisation.",
                login_org().unwrap_or_else(brand_org)
            )),
            Some("state_mismatch") => {
                Some("Login failed (state mismatch). Please try again.".to_string())
            }
            Some(_) => Some("Login failed. Please try again.".to_string()),
            None => None,
        }
    };

    view! {
        <div class="min-h-screen flex flex-col bg-slate-950 text-slate-100">
            <Suspense fallback=move || view! { <div class="p-6">"Loading..."</div> }>
                {move || view! {
                <>
                <header class="px-6 py-4 border-b border-slate-800 flex items-center justify-between gap-4">
                    <div class="flex items-center gap-3 min-w-0">
                        <div class="w-2 h-2 rounded-full bg-teal-400"></div>
                        <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300 truncate">
                            {brand_name()}
                        </h1>
                    </div>
                    <span class="hidden sm:inline text-xs text-slate-500 truncate">
                        {target_label()}
                    </span>
                </header>
                <main class="flex-1 px-6 py-12 md:py-16">
                    <div class="mx-auto grid w-full max-w-5xl gap-10 md:grid-cols-[minmax(0,1.1fr)_minmax(320px,0.9fr)] md:items-center">
                        <section class="space-y-7">
                            <div class="inline-flex items-center gap-2 rounded-full border border-teal-400/20 bg-teal-400/10 px-3 py-1 text-xs font-medium text-teal-100">
                                <span class="h-1.5 w-1.5 rounded-full bg-teal-300"></span>
                                "GitHub-backed knowledge workspace"
                            </div>
                            <div class="space-y-4">
                                <h2 class="max-w-3xl text-4xl font-semibold tracking-tight text-slate-50 md:text-5xl">
                                    "Operate the knowledge base from the graph."
                                </h2>
                                <p class="max-w-2xl text-base leading-relaxed text-slate-400 md:text-lg">
                                    {move || format!("{} turns the {} repository into a browsable wiki, graph, and editing surface. ", brand_name(), target_label())}
                                    "Content stays in GitHub while the app adds structure for concepts, decisions, notes, and work items."
                                </p>
                            </div>
                            <div class="grid gap-3 text-sm text-slate-300 sm:grid-cols-3">
                                <div class="border-l border-slate-700 pl-3">
                                    <p class="font-medium text-slate-100">"Browse"</p>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-500">"Read the projected graph and repo-backed files."</p>
                                </div>
                                <div class="border-l border-slate-700 pl-3">
                                    <p class="font-medium text-slate-100">"Edit"</p>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-500">"Write changes through the current GitHub permissions."</p>
                                </div>
                                <div class="border-l border-slate-700 pl-3">
                                    <p class="font-medium text-slate-100">"Admin"</p>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-500">"Tune views and config as the workspace evolves."</p>
                                </div>
                            </div>
                        </section>

                        <section class="rounded-md border border-slate-800 bg-slate-900/70 p-6 shadow-2xl shadow-black/20">
                            <div class="space-y-5">
                                <div class="space-y-2">
                                    <p class="text-xs font-semibold uppercase tracking-[0.2em] text-slate-500">
                                        "Access"
                                    </p>
                                    <h2 class="text-2xl font-semibold text-slate-50">
                                        {brand_name()}
                                    </h2>
                                    <p class="text-sm leading-relaxed text-slate-400">
                                        {move || format!("Sign in with GitHub to open the {} workspace.", brand_org())}
                                    </p>
                                </div>

                                <Show when=move || error_msg().is_some()>
                                    <div class="rounded-md bg-red-500/10 border border-red-400/30 px-4 py-3 text-red-200 text-sm">
                                        {move || error_msg().unwrap_or_default()}
                                    </div>
                                </Show>

                                <a
                                    href="/auth/login"
                                    rel="external"
                                    class="inline-flex w-full items-center justify-center gap-2 rounded-md border border-teal-400/40 bg-teal-500/20 px-5 py-3 text-sm font-medium text-teal-100 transition-colors hover:bg-teal-500/30"
                                >
                                    <svg class="w-4 h-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
                                        <path d="M8 0C3.58 0 0 3.58 0 8a8 8 0 005.47 7.59c.4.07.55-.17.55-.38v-1.33c-2.23.48-2.7-1.07-2.7-1.07-.36-.92-.89-1.17-.89-1.17-.72-.49.06-.48.06-.48.8.06 1.23.83 1.23.83.71 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.65-.89-3.65-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 014 0c1.52-1.03 2.19-.82 2.19-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.74.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0016 8c0-4.42-3.58-8-8-8z"/>
                                    </svg>
                                    "Login with GitHub"
                                </a>

                                <div class="border-t border-slate-800 pt-5">
                                    <div class="rounded-md border border-amber-400/30 bg-amber-500/10 px-4 py-3">
                                        <p class="text-xs font-semibold uppercase tracking-[0.2em] text-amber-200">
                                            "OAuth status"
                                        </p>
                                        <p class="mt-2 text-sm leading-relaxed text-amber-100/80">
                                            {move || match login_org() {
                                                Some(org) => format!(
                                                    "Login is limited to members of {org}. Repository access is checked separately against your live GitHub permissions."
                                                ),
                                                None => "Any GitHub user may sign in. Each workspace is still protected by live repository permissions.".to_string(),
                                            }}
                                        </p>
                                    </div>
                                    <p class="mt-3 text-xs text-slate-600">
                                        {move || match login_org() {
                                            Some(org) => format!("Login is restricted to {org} organisation members."),
                                            None => "This deployment uses org-less login.".to_string(),
                                        }}
                                    </p>
                                </div>
                            </div>
                        </section>
                    </div>
                </main>
                <footer class="px-6 py-4 border-t border-slate-800 text-xs text-slate-600 text-center space-y-1">
                    <p>"Brain · Edge Administration"</p>
                    <p>
                        "© 2026 Andrea Bozzo · AGPLv3 · "
                        <a
                            href="https://github.com/AndreaBozzo/Brain_UI"
                            rel="external noopener"
                            target="_blank"
                            class="underline hover:text-slate-400"
                        >
                            "Source"
                        </a>
                    </p>
                </footer>
                </>
                }}
            </Suspense>
        </div>
    }
}
