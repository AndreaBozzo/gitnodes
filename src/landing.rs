use leptos::prelude::*;

#[component]
pub fn Landing() -> impl IntoView {
    view! {
        <div class="min-h-screen flex flex-col bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-teal-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "Dritara Brain"
                </h1>
            </header>
            <main class="flex-1 flex items-center justify-center px-6">
                <div class="max-w-xl w-full text-center space-y-8">
                    <div class="space-y-3">
                        <h2 class="text-4xl font-semibold tracking-tight">
                            "Internal Knowledge & Edge-Administration"
                        </h2>
                        <p class="text-slate-400 text-base leading-relaxed">
                            "A wiki, graph, and CMS for the Dritara Brain repository. "
                            "Read concepts, decisions, and meeting notes — and write them back to GitHub."
                        </p>
                    </div>
                    <div class="flex justify-center">
                        <a
                            href="/auth/login"
                            class="inline-flex items-center gap-2 px-5 py-2.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-100 text-sm font-medium hover:bg-teal-500/30 transition-colors"
                        >
                            <svg class="w-4 h-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
                                <path d="M8 0C3.58 0 0 3.58 0 8a8 8 0 005.47 7.59c.4.07.55-.17.55-.38v-1.33c-2.23.48-2.7-1.07-2.7-1.07-.36-.92-.89-1.17-.89-1.17-.72-.49.06-.48.06-.48.8.06 1.23.83 1.23.83.71 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.65-.89-3.65-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 014 0c1.52-1.03 2.19-.82 2.19-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.74.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0016 8c0-4.42-3.58-8-8-8z"/>
                            </svg>
                            "Login with GitHub"
                        </a>
                    </div>
                    <p class="text-xs text-slate-600">
                        "Access is restricted to repository collaborators."
                    </p>
                </div>
            </main>
            <footer class="px-6 py-4 border-t border-slate-800 text-xs text-slate-600 text-center">
                "Brain · Edge-Administration"
            </footer>
        </div>
    }
}
