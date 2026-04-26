# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

**Stato architetturale corrente:** Git e il repo target restano la *Single Source of Truth*. SQLite copre sessioni, audit log e una projection locale target-scoped per `nodes`, `edges`, `files`, `backlinks`, `work_items` e `work_item_bindings`, con rebuild esplicito e watermark/error state. La sincronizzazione inbound baseline (webhook/SSE) e la materializzazione operativa dei work item sono chiuse; il gap residuo sta ora nelle mutazioni bidirezionali verso il forge, nel routing multi-target end-to-end e nella gestione permission-aware delle scritture.

**Realignment 2026-04-26:** Fasi 3 e 4 non vanno più trattate come estensioni speculative. La base costruita in Fase 2B (projection SQLite multi-tenant, target-scoped cache, `brain_id` stabile, rename atomici via Git Data API) consente di pianificare il lavoro successivo come evoluzione concreta del runtime esistente. In pratica: Fase 3 diventa la fase del **workspace multi-tenant collaborativo**, Fase 4 quella della **standardizzazione forge/time-travel/local mode**.

---

## 🟢 Fase 1: Astrazione e Configurabilità

Rimozione dell'hardcoding per rendere la Brain UI un tool generico e platform-agnostic, basato interamente su API REST.

**Principio guida:** Fase 1 deve essere non-breaking per installazioni esistenti e libera da speculazione architetturale. Abstraction layers (trait) e admin UI vengono rimandati alle fasi dove esiste un secondo consumer/use-case che ne valida la forma.

**Stato al 2026-04-24:** i deliverable tecnici e di contratto-prodotto sono chiusi. Il repo `Brain` ha `.brain-config.yml` esplicito e il deploy rimane backward-compatible sugli env legacy; la chiusura operativa residua viene trattata come smoke checklist di deploy, non come deliverable che blocca l'avvio della fase successiva.

- [x] **Frontmatter round-trip** — _2026-04-22_
    - `EditPrefill::from_raw` parsa l'intero frontmatter YAML in `BTreeMap<String, serde_yaml::Value>`.
    - `BrainFilePayload.preserved_frontmatter` propaga il dict all'update; `merge_frontmatter` fa overlay dei campi del form invece di rigenerare da template.
    - Campi custom (status, severity, cliente, ecc.) sopravvivono ai save.
- [x] **Config Loader + Schema + Default Migration** — _2026-04-22_
    - Parser YAML per `.brain-config.yml` con validazione al load (no duplicate directory, colori validi, nomi non riservati `tags`/`templates`).
    - Default config incluso nel binario (`BrainConfig::default()`), equivalente esatto all'attuale `NodeType` hardcodato. Repo senza config file funzionano identici a oggi: zero migrazione richiesta.
    - Validazione fail-soft (non fail-fast): il server parte prima che esista un token autenticato; parsing failure logga warning e cade sul default. Errore visibile via orphan banner.
    - TTL-cache in memoria 30s + `invalidate()` pronto per i webhook push di Fase 2.
- [x] **NodeType → Config Lookup**
    - Sostituzione dell'enum `NodeType` con `String` + lookup `Arc<BrainConfig>`.
    - Migrazione di `EditPrefill::from_raw`, `draft.rs` (localStorage schema-version bump), template loader, ricavo directory/label/accent.
- [x] **Graceful Fallback per Tipi Sconosciuti** — _2026-04-22_
    - `config.lookup(type_str).unwrap_or(&config.default_spec())` applicato in tutti i render site.
    - Banner amber su `/knowledge` (`orphan_banner.rs`): lista tipi sconosciuti con conteggio + CTA a `.brain-config.yml` via `GithubClient::config_blob_url()`. Dismissable per sessione.
    - Banner rosé nell'editor + Save disabilitato per frontmatter malformato, coerente col refuse server-side già esistente.
- [x] **Creazione Guidata (Zero Typo)** — _2026-04-22_
    - Menu "Nuovo Nodo" generato da `config.creatable()`.
    - Autocompilazione frontmatter YAML (`NodeTypeSpec.frontmatter_seed`) e path di salvataggio.
- [x] **`GithubClient` Centralizzato (URL-only, no trait)** — _2026-04-22_
    - Struct `GithubClient` in `brain-domain/src/config.rs`: `contents_url`, `tree_url`, `raw_base`, `blob_base`, `config_blob_url`. `TargetConfig` ridotto a puro data carrier.
    - Migrati tutti i 16 callsite. Nessun `ForgeAdapter` trait: design rimandato a Fase 4 contro un secondo adapter reale.
    - Deferred a Fase 2: pooling `reqwest::Client`, centralizzazione header `Authorization`/`User-Agent`.
- [x] **`NodeTypeSpec`: campi per-tipo** — _2026-04-23_
    - `title_key`, `date_create_field`, `date_update_field`, `body_label` come `Option<String>` in `NodeTypeSpec`.
    - Eliminati tutti gli switch hardcodati su nome tipo in `api.rs` e nell'editor.
- [x] **WorkItem model + label taxonomy** — _2026-04-24_
    - `WorkItem` in `brain-domain/src/work_items.rs`: `brain_id`, `kind` (Task/Discussion/Decision/Incident/Change), `state`, `labels`, `assignees`, `content_path`, `external_binding` opzionale, `system_of_record`.
    - `ExternalWorkItemBinding`: `system`, `project`, `item_key`, `provider_id?`, `url?`. Le issue del forge sono un binding, non l'ontologia.
    - `WorkItemLabelSpec` + `label_taxonomy` in `BrainConfig`: mapping machine-readable `kind → forge_label` e `state → forge_label`. Cinque kind built-in con label `brain:*`. Helper `labels_for_kind()`, `all_kind_labels()`.
    - Source-of-truth dichiarata: frontmatter = verità editoriale; `WorkItem` = dominio operativo; SQLite = read model (Fase 2); forge issue = backend di collaborazione opzionale.
- [x] **Dogfooding su repo Brain** — _2026-04-24_
    - `.brain-config.yml` creato nel repo Brain: tutti e sette i tipi + `label_taxonomy`. Equivalente 1:1 a `BrainConfig::default()`.
    - La verifica residua viene ridotta a smoke test operativo su deploy/configurazione, senza riaprire la fase sul piano architetturale.

### Spostati fuori da Fase 1

- **Pannello Impostazioni (Visual YAML Editor)** → Fase 3. Admin-only, dipende da RBAC.
- **`ForgeAdapter` trait** → Fase 4. Progettare un trait su una sola implementazione produce quasi sempre la forma sbagliata.

---

## 🟡 Fase 2: Operatività, Sync e Dati

Introduzione di flussi operativi, sincronizzazione in tempo reale e salvaguardia dell'integrità dei dati.

**Realignment 2026-04-24:** la fase viene esplicitamente spezzata in due sottostadi. Prima si chiudono i prerequisiti operativi che oggi rendono fragile il runtime single-target; solo dopo si introduce la projection locale e l'operatività work item visibile in UI.

### 2A. Hardening operativo (prerequisiti)

- [x] **Scoping delle cache runtime** — _2026-04-24_
    - Cache process-global in `brain-storage` (graph/template) e `config_loader` (config) hanno chiavi troppo deboli.
    - Rekeying per `target_id` come minimo; dove la visibilità diverge per ruolo, aggiungere `user_id_or_role`.
    - L'invalidazione da webhook o write-path deve colpire solo le entry del target corretto.
- [x] **`GithubClient`: pooling + header centralization** — _2026-04-24_
    - `reqwest::Client` pooled condiviso tra storage, config loader, asset proxy e sync jobs.
    - Helper `get/put/delete/post` che centralizzano `Authorization`, `User-Agent`, retry policy e logging.
- [x] **Baseline refresh prima di SSE/Webhook** — _2026-04-24_
    - Aggiungere un percorso di refresh esplicito e/o polling leggero per ridurre la staleness percepita prima di investire nella pipeline eventi completa.
    - L'obiettivo è chiudere il gap operativo "commit esterno → reload manuale" con la soluzione più economica disponibile.
- [x] **Release safety per server functions** — _2026-04-24_
    - Ridurre il rischio introdotto da `register_server_functions` manuale in release (`lto = true`).
    - Introdurre guardrail di test/build o automazione che intercetti server fn non registrate prima del deploy.
- [x] **Hardening link semantics / rename safety** — _2026-04-24_
    - Consolidare il comportamento dei link relativi, dei backlink e delle rewrite su path nested.
    - Estendere i test per i casi `Related / See also`, rename e link markdown con destinazioni relative.

### 2B. Projection, sync e operatività

- [x] **Projection Layer SQLite** — _2026-04-24_
    - SQLite esteso oltre sessioni/audit con tabelle `targets`, `projection_sync_state`, `nodes`, `edges`, `files`, `backlinks`, `work_items`, `work_item_bindings`.
    - Schema multi-tenant fin dal giorno zero: ogni tabella di projection usa `target_id` come FK verso `targets`, così il backend resta multi-repo capable anche con UI single-target per sessione.
    - Bootstrap iniziale dal tree Git via `GithubStorage::fetch_raw_files()` + `server::projection::rebuild()`, con pipeline idempotente di full upsert per target.
    - `LoadBrainGraph` legge la projection SQLite locale; Git resta source of truth, SQLite diventa read model locale con riallineamento post-write.
- [x] **Rebuild, reconciliation e drift recovery** — _2026-04-24_
    - `RefreshBrainGraph` esegue un full rebuild/manual reindex della projection invece di limitarsi al cache busting.
    - `projection_sync_state` conserva `last_attempt_at`, `last_success_at`, `last_error_at`, `last_error`, `last_reason` e i count principali, fornendo watermark e stato osservabile.
    - In caso di reconcile fallito con snapshot già valida, il backend serve l'ultima projection buona e registra l'errore, riducendo il rischio di UI vuota per drift o sync parziali.
- [x] **Sincronizzazione inbound: Webhooks + SSE (baseline)** — _2026-04-25_
    - `POST /webhook/github` accetta payload GitHub, valida `X-Hub-Signature-256` con HMAC-SHA256 a tempo costante (`WEBHOOK_SECRET` env). Eventi non-`push` ricevono `202 Accepted` silenzioso per non sporcare il delivery log.
    - Su `push`: rebuild della projection per il target corrente via `GITHUB_TOKEN` server-side; pubblica `BrainEvent::GraphUpdated` su sync riuscita o `BrainEvent::SyncFailed { message }` su rebuild fallito sul `tokio::sync::broadcast` bus.
    - `GET /sse/events` espone lo stream typed (`graph_updated`, `sync_failed`) con keep-alive di default; la route è auth-gated e ogni client Leptos connesso bumpa `graph_version` via `EventSource` (componente `LiveSync`, hydrate-only).
    - Refinement 2026-04-25: `LiveSync` implementa reconnect/backoff esplicito lato client e la Knowledge UI mantiene `SyncStatus` esplicito con banner operativo `Stale Data`. Il refetch preserva meglio lo stato locale usando `selected_path` come ancora della selezione invece del solo node id volatile.
    - Da questo punto in poi il lavoro residuo su questo asse non blocca più la Fase 2: rimane soprattutto il desiderio di una vista admin/status condivisa oltre al banner page-local.
    - Rimane fuori da questa baseline: eventi più granulari (`FileUpdated { path }`) e sync incrementale di `work_item`/`work_item_bindings` da eventi operativi del forge.
- [x] **Rename atomici via Git Data API** — _2026-04-25_
    - `GithubStorage::atomic_rename` orchestra `POST /git/blobs` → `/git/trees` (delete via `sha: null` su `base_tree`) → `/git/commits` → `PATCH /git/refs/heads/{branch}` con `force=false`. Helper isolato in `brain-storage::atomic_rename`.
    - Retry su `422 Update is not a fast forward` con backoff esponenziale (default 100/400/1600 ms, max 3 tentativi); altri 4xx propagano subito. Le blob già caricate vengono riusate fra tentativi (sono content-addressed).
    - `rename_brain_file` ora produce **un solo commit** con tutti i referrer + creazione + delete; il messaggio commit dell'utente vale per l'unico commit. La projection locale resta riallineata via `rebuild_projection_after_write` (post-write, sequenziale): nessun dual-write nel write-path.
    - Test wiremock-based su happy path, header auth/UA, payload tree con `sha: null`, retry su fast-forward (verifica blob non ricaricate), 422 non-fast-forward non ritentato, cap massimo tentativi.
- [x] **Fondazione operativa Work Item** — _2026-04-25_
    - `task` introdotto come primo `work_item_kind` via config/template reali nel repo Brain (`.brain-config.yml` + `templates/Task.md`), senza reintrodurre hardcode nel create flow.
    - `merge_frontmatter` assegna `brain_id` al primo save per i tipi work item e lo preserva negli update successivi, così rename e rebuild non cambiano identità quando il documento nasce dalla UI.
    - La rebuild della projection materializza i documenti work item in SQLite (`work_items`, `work_item_bindings`) leggendo `work_item_kind`, `state`, `system_of_record`, `assignees` ed eventuale `external_binding` dal frontmatter; i label provider vengono derivati dalla `label_taxonomy` del config.
    - `LoadWorkItemByPath` espone il read model SQLite come API read-only dedicata; il detail panel mostra il primo scope UI minimale e read-only (`state`, `system_of_record`, `assignees`, binding esterno) senza introdurre scritture sul forge.
    - Policy iniziale: `task ↔ issue` è opt-in, non rigido. `system_of_record: brain|split|external` e `external_binding` opzionale coprono `draft task`, item local-only e item provider-bound senza introdurre dual-write nel write path.
    - Il primo scope UI resta volutamente minimo sul lato forge: l'editor già espone i campi operativi (`state`, `system_of_record`, `assignees`) nel normale save flow, ma commenti, mutazioni issue/PR, sync provider-bound e board UX rimangono rinviati alla Fase 3.

---

## 🟠 Fase 3: Workspace Collaborativo, Work Item & RBAC

Abilitare un workspace realmente multi-tenant e collaborativo sopra le fondamenta già chiuse in Fase 2B. Il focus non è più "aggiungere OAuth" in astratto, ma separare chiaramente tre piani: **routing del target**, **mutazioni operative dei work item**, **orchestrazione permission-aware delle scritture**.

**Principio guida:** il backend ha già le primitive giuste (`TargetKey`, projection SQLite per target, `brain_id` stabile, write path GitHub centralizzato). La Fase 3 deve evitare nuove scorciatoie single-target e introdurre ogni nuova capability come target-aware fin dal primo commit.

- [ ] **3.1 Multi-Tenant Workspace Routing**
    - Evolvere il router Leptos da path statici (`/knowledge`, `/admin`) a path dinamici `/{org}/{repo}/knowledge` e `/{org}/{repo}/admin`, mantenendo eventuali redirect dal target di default solo come compat layer temporaneo.
    - Spostare la risoluzione del `TargetConfig` dal boot statico del server al contesto della request/pagina. `GithubHttp` pooled è già target-agnostic; ciò che manca è togliere l'assunzione che asset proxy, SSR context e webhook state puntino a un solo repo definito da env.
    - Introdurre il **Brain Switcher** nella sidebar: discovery via token OAuth dei repo accessibili all'utente che contengono `.brain-config.yml`, con stato esplicito per `accessible / missing-config / forbidden`.
    - Success criterion: la stessa istanza Brain UI può navigare più repo/target senza restart né collisioni di cache/projection, e l'URL diventa l'identificatore canonico del target attivo.
- [ ] **3.2 Work Items Interattivi e Bidirectional Sync**
    - Promuovere il modello `WorkItem` da read model/editorial overlay a workflow operativo completo. Aggiornare `state`, `assignees`, label taxonomiche e binding esterno deve poter mutare il provider quando `system_of_record` lo richiede.
    - Aggiungere server fn mutate-only dedicate (`transition_work_item`, `assign_work_item`, `bind_work_item`, ecc.) invece di sovraccaricare il save markdown generico: il salvataggio editoriale del file e la mutazione del provider hanno failure mode, audit trail e policy diverse.
    - Usare l'access token OAuth dell'utente per le mutazioni GitHub Issue/PR; in ingresso, estendere la pipeline webhook oltre `push` per intercettare eventi `issues`, `issue_comment`, `pull_request`, `labeled`, `assigned` quando afferenti a item bindati, e propagare SSE mirate (`work_item_updated`, `binding_updated`) oltre al generico `graph_updated`.
    - Formalizzare una policy di coerenza per `system_of_record`:
      - `brain`: mutate solo frontmatter/projection locale.
      - `split`: mutare sia Brain che provider con riconciliazione esplicita.
      - `external`: UI Brain come control plane/read-through cache di un item nato altrove.
    - Success criterion: un cambio di stato fatto in UI aggiorna l'Issue GitHub corrispondente con il token dell'utente; un update GitHub out-of-band rientra in projection/UI senza refresh manuale.
- [ ] **3.3 RBAC e Save Orchestration Permission-Aware**
    - Smettere di modellare RBAC come semplice flag admin/non-admin. Il controllo reale è una capability matrix per target: `can_read`, `can_write_default_branch`, `can_review_via_pr`, `can_admin_config`, derivata da sessione OAuth + permessi repo/branch.
    - Riutilizzare la logica di tree commit atomico già introdotta per i rename, generalizzandola in un write path capace di scegliere tra:
      - commit diretto sul branch target se l'utente ha write access;
      - branch temporaneo `patch-{user}-{timestamp}` + apertura PR se non può scrivere direttamente ma può proporre review.
    - Tutte le azioni di scrittura ad alto livello (`save`, `rename`, `config update`, future work item mutations che toccano file) devono passare per questo orchestratore, così il fallback PR non diventa un caso speciale fragile.
    - La UI deve rendere esplicito l'esito: `Saved to main`, `Proposed via PR`, `Blocked by permissions`, con link a branch/PR e audit coerente.
    - Success criterion: un contributor senza write access può usare Brain UI normalmente; il sistema devia automaticamente su branch+PR senza perdere atomicità o metadata di autore.
- [ ] **3.4 Visual Configuration Editor** _(spostato da Fase 1)_
    - Portare `.brain-config.yml` fuori dall'editor raw con una GUI admin-only che copra i casi reali emersi: node types, directory mapping, accent colors, `work_item_kind`, `label_taxonomy`, binding provider e impostazioni visuali del grafo.
    - Il backend continua a serializzare YAML con `serde_yaml`, ma il save deve transitare dallo stesso orchestratore permission-aware della 3.3: commit diretto per admin con write access, PR proposta negli altri casi approvati.
    - La validazione deve riutilizzare il parser runtime del config, non uno schema parallelo nel frontend, per evitare drift tra editor visuale e loader server-side.
- [ ] **3.5 Rate-Limit Shielding e Background Reconciliation**
    - Una volta introdotte mutazioni work item e discovery multi-repo, spostare le chiamate GitHub più costose dietro job/reconcile espliciti e usare SQLite come cache operativa interrogabile dal frontend.
    - Questo asse non è l'entry point della fase, ma diventa necessario appena la UI smette di leggere solo il repo attivo e comincia a scansionare repo, issue e PR per utente.
- [ ] **3.6 Graph Canvas Polish (zoom, transitions, edge framing)**
    - Promuovere il viewBox SVG da `"0 0 100 100"` hardcodato a un signal `(cx, cy, scale)` guidato da wheel/pinch/`+`/`-`/reset, per uscire dal "scale fisso 100×100" attuale.
    - Sostituire lo snap istantaneo del viewBox sulla selezione con un tween RAF-driven (~300ms) — `viewBox` non è transitionable via CSS, va animato esplicitamente.
    - Reframing dei nodi vicini al bordo del data space (oggi mostrano area vuota) e opzionale recentering anche su hover, non solo su selezione.
    - Vincolo: niente D3 — ~30KB di JS aggiuntivi non si sposano con il bundle Leptos+WASM. La logica resta in Rust/SVG nativo.
    - Success criterion: zoom continuo 0.25×–4×, transizioni morbide tra stati di selezione, nessuna area vuota visibile sui bordi del grafo per repo realistici.

---

## 🔴 Fase 4: Forge Independence & Deep History

Sfruttare la maturazione del backend GitHub-first per standardizzare il boundary verso altri forge e usare la projection SQLite come motore per la vista temporale e il local/offline mode.

**Principio guida:** il trait non va estratto da `GithubClient` in modo cosmetico. Va ricavato dalle capability realmente saturate in Fase 3: repository discovery, snapshot tree/blob, mutazioni branch/PR, work item sync, webhook normalization e policy di rate limit.

- [ ] **4.1 `ForgeAdapter` Trait (capability-driven)**
    - Estrarre un boundary che copra i bisogni reali del runtime, non un semplice wrapper HTTP. Le capability minime previste sono: target discovery/listing, lettura tree/blob, write commit/branch/ref update, PR/MR creation, work item issue mutation, webhook verification + event normalization.
    - Valutare esplicitamente se implementarlo come un singolo trait o come famiglia di subtrait/capabilities (`ForgeRepoAdapter`, `ForgeCollaborationAdapter`, `ForgeWebhookAdapter`) per evitare un "lowest common denominator" troppo povero o un god-trait ingestibile.
    - Adapter secondari: **GitLab** e **Gitea/Forgejo**. GitHub resta reference implementation finché i casi reali non stabilizzano la forma finale.
- [ ] **4.2 Temporal Graph View (Git Time Jump)**
    - La feature non deve limitarsi a mostrare vecchi file. L'obiettivo è una modalità storica completa con slider/timeline che ricostruisce una projection temporanea del repository a una data/SHA e la rende navigabile nella stessa UI a grafo.
    - Reuse intenzionale della pipeline esistente: fetch tree storico → build in-memory/ephemeral SQLite projection → render di graph canvas, detail panel e knowledge base in modalità read-only storica.
    - Estensione naturale: confronto `then vs now` per nodi, backlink e work item bindati, senza introdurre un parser o store storico separato.
- [ ] **4.3 Local / Offline Execution Context**
    - Permettere l'esecuzione di Brain UI contro un `.git` locale o una working tree locale senza dipendere da Axum come proxy di un forge remoto.
    - Implementare un `LocalFileSystemAdapter`/`LocalGitAdapter` che offra la stessa superficie minima usata dal runtime: lettura snapshot, commit locali, branch locali, eventuale sync successivo verso remoto opzionale.
    - Evitare fork architetturali: stessa UI, stesso projection pipeline, diverso adapter.
- [ ] **4.4 Advanced Conflict Resolution**
    - La Fase 2B ha introdotto il banner `Stale Data`; con collaborazione reale e fallback via PR non basta più.
    - Quando webhook o ref update rivelano divergenze rispetto al draft locale o al branch corrente, servono diff e merge espliciti: vista side-by-side, scelta hunk-based o almeno `local / remote / apply anyway`.
    - Questo asse è particolarmente importante se la 3.3 porta davvero contributor multipli e branch temporanei: il conflitto non sarà più eccezione, ma percorso operativo ordinario.

---

## 🟣 Fase 5: AI & Automations Ecosystem

Trasformare la Brain UI in un assistente attivo tramite IA e trigger di automazione esterni.

- [ ] **AI Assistant Proxy (Copilot Integration)**
    - Assistente AI nell'editor Leptos: generazione markdown, autocompletamento, summarization, tagging.
    - Proxy sicuro in Axum che usa l'OAuth token dell'utente per le API AI di GitHub/Copilot (RBAC + accounting corretto).
- [ ] **Outbound Webhooks Engine**
    - Motore di eventi in background per inviare webhook a sistemi esterni (GitHub Actions, Zapier, CI/CD).
    - Trigger configurabili in `.brain-config.yml` (es. `on_work_item_done: https://...`).

---

> **ROADMAP IS ALWAYS SUBJECT TO CHANGES AND REALIGNMENTS** — this sketch is indicative of direction, not a commitment.

---

## Known caveats

1. **CSRF `state_mismatch` diagnostics** — **PARTIAL 2026-04-26**. `SameSite=Lax` is correct for the top-level GitHub callback redirect; `SESSION_COOKIE_SECURE=1` is now confirmed set on Railway, eliminating the most likely cause of dropped state cookies in prod. `oauth_callback` now logs `login_fail/state_missing` (cookie absent → SameSite/Secure/session-store problem) separately from `login_fail/state_mismatch` (cookie present but value differs → replay or stale link), so the two failure modes can be distinguished from the audit log without guessing. Residual risk: a horizontal scale-out on Railway without a shared session store would still drop state; revisit if `state_missing` shows up in audit despite the Secure cookie.

2. **DONE 2026-04-26** **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. ~~Update path regenerates frontmatter from templates~~ — **DONE 2026-04-22**. `merge_frontmatter` fa overlay dei campi del form sulla mappa preservata invece di rigenerare da template. Tests in `brain-app::api::merge_frontmatter_tests`.

6. ~~No auto-refresh after out-of-band commits~~ — **DONE 2026-04-24**. `RefreshBrainGraph` server fn + `RefreshButton` component in the knowledge header now trigger a full rebuild of the local SQLite projection after invalidating graph, template, and config caches. The button closes the "commit esterno → reload manuale" gap and doubles as manual reindex/drift recovery until webhook/SSE sync lands.

7. ~~Rename issues N+2 Contents API commits~~ — **DONE 2026-04-25**. `rename_brain_file` now produces a single commit via `GithubStorage::atomic_rename` (Git Data API: blobs → tree → commit → ref update with `force=false`). Fast-forward conflicts (422) are retried with capped exponential backoff; uploaded blobs are reused across retries since they're content-addressed.

8. ~~Graph cache is process-global, not user- or target-scoped~~ — **DONE 2026-04-24**. All caches in `brain-storage` (graph, template) and `config_loader` are now keyed by `TargetKey({org}/{repo}/{branch})` via `OnceLock<Mutex<HashMap<TargetKey, _>>>`. `GithubHttp` is target-agnostic (plain `Arc<reqwest::Client>`); each `GithubStorage` / call-site supplies its own `GithubClient` built from an explicit `TargetConfig`. Safe for Phase 3 multi-target without re-architecting.

9. ~~`register_explicit` boilerplate is LTO-coupled~~ — **DONE 2026-04-24**. `SERVER_FNS: &[&str]` const + `include_str!`-based test in `api.rs` catches any `#[server]` fn not listed in the const before it reaches CI.

10. ~~Sync visibility is still page-local~~ — **DONE 2026-04-26**. `LiveSync` (EventSource subscription) e `SyncStatusBanner` ora vivono in `App`, sopra `<Routes>`, leggendo `graph_version`/`sync_status` esposti via `provide_context(GraphVersion)` / `provide_context(SyncStatusSignal)`. Admin su `/admin` o future route work-item vedono lo stesso banner `Stale Data` di `/knowledge`. `RefreshButton` resta page-local in Knowledge ma muta gli stessi signal globali.

11. **UI limitations** — No animated transitions between viewBox states (snap is instant). Nodes near graph edges show empty area outside the data space. Hover does not recenter, only selection does. No zoom: scale stays 100×100.
