# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

**Stato architetturale corrente:** Git e il repo target restano la *Single Source of Truth*. SQLite copre sessioni, audit log e una projection locale target-scoped per `nodes`, `edges`, `files` e `backlinks`, con rebuild esplicito e watermark/error state. La sincronizzazione inbound (webhook/SSE) e la materializzazione operativa dei work item restano capability di Fase 2B ancora aperte.

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
- [ ] **Sincronizzazione inbound: Webhooks + SSE**
    - Endpoint Axum per `push` e per eventi operativi del forge: validazione firma, idempotenza, fan-out verso invalidazione/selective refresh della proiezione.
    - Su `push`: invalidare solo il target corretto, aggiornare config/template/graph projection, pubblicare evento SSE al frontend.
    - Su eventi operativi esterni: aggiornare `work_items` e `work_item_bindings` senza aspettare un commit Git.
    - Lato Leptos: `EventSource` con reconnect/backoff su segnali dedicati, senza full route reset.
- [ ] **Resource invalidation unificata sul write-path**
    - L'infrastruttura base esiste: save/delete/rename bumpano `graph_version`. Resta da unificare il pathway per update inbound da SSE/webhook.
    - Preservare stato UI durante il refetch (selezione corrente, editor aperto, path rinominato).
- [ ] **Rename atomici via Git Data API**
    - Oggi `rename_brain_file` esegue N+2 commit via Contents API.
    - Migrare a un'unica operazione: `POST /git/blobs` → `/git/trees` → `/git/commits` → `PATCH /git/refs/heads/{branch}`.
    - Aggiornare la proiezione locale nella stessa transazione per evitare drift Git/SQLite.
- [ ] **Fondazione operativa Work Item**
    - Introdurre `task` come primo `work_item_kind` via config/template reali, non via hardcode.
    - Decidere se il binding 1:1 `task ↔ issue` è policy iniziale o regola rigida; coprire le eccezioni (`draft task`, `local-only item`, `provider item senza doc`).
    - Il primo scope UI parte solo dopo che projection e sync inbound esistono almeno in forma minima; commenti, review branch e PR restano fuori fino alla Fase 3.

---

## 🟠 Fase 3: Collaborazione e Sicurezza (RBAC)

Abilitare l'uso della piattaforma a team estesi, sfruttando l'App OAuth esistente.

- [ ] **Impersonation tramite OAuth**
    - Uso dell'access token univoco dell'utente loggato per azioni sul forge a suo nome (commenti su work item bindati, apertura PR).
- [ ] **Multi-Repo Workspace UX**
    - "Brain Switcher" nella UI (menu laterale) per navigare tra repository dell'organizzazione senza cambiare URL.
    - Discovery via token OAuth: scansione dei repo accessibili all'utente e listing automatico di quelli che espongono `.brain-config.yml`. Dipende dall'impersonation dello stesso step.
    - Gestione graceful degli errori di autorizzazione: se l'utente non ha permessi Git su un Brain, messaggio esplicito senza crash.
    - Il backend diventa effettivamente multi-repo solo dopo target-scoped caches + Projection Layer SQLite multi-tenant; fino ad allora il runtime resta concettualmente single-target per sessione.
- [ ] **Workflow di Review (Branching & PR)**
    - Flusso "Proponi Modifica" per utenti non-admin.
    - Creazione automatica di branch temporanei e apertura PR direttamente dalla UI.
- [ ] **Rate-Limit Shielding**
    - Centralizzazione delle chiamate al forge tramite job in background. Il frontend interroga esclusivamente la cache SQLite.
- [ ] **Pannello Impostazioni (Visual YAML Editor)** _(spostato da Fase 1)_
    - UI dedicata (solo Admin) per creare e modificare tipi di nodo: Color Picker, gestione cartelle, mapping `work_item_kind`/binding provider.
    - Il backend Axum serializza (`serde_yaml`) e pusha le modifiche a `.brain-config.yml`.

---

## 🔴 Fase 4: History & Multi-Forge

Esplorazione del passato e indipendenza totale dal provider.

- [ ] **Git Time Jump**
    - Navigazione della history dalla UI (visualizzazione di un nodo a uno specifico SHA), con stato conversazionale del `WorkItem` bindato quando presente.
- [ ] **`ForgeAdapter` Trait + Multi-Forge Support** _(trait spostato da Fase 1)_
    - Estrazione del `trait ForgeAdapter` da `GithubClient`, progettato contro i requisiti reali dei nuovi adapter e solo dopo aver saturato il backend GitHub-first.
    - Adapter ufficiali per **GitLab** e **Gitea/Codeberg**.
- [ ] **Offline Mode / Local Git**
    - Supporto per repository locali senza un forge remoto.

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

1. **CSRF `state_mismatch` on dropped session cookie** — `/auth/login` stores state in session, `/auth/callback` compares. If the browser drops the cookie between redirects (cross-site cookie policy, incognito) the callback returns `/?error=state_mismatch`. Likely culprit: `SameSite=Lax` vs. GitHub redirect chain. Fix only when it bites.

2. **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. ~~Update path regenerates frontmatter from templates~~ — **DONE 2026-04-22**. `merge_frontmatter` fa overlay dei campi del form sulla mappa preservata invece di rigenerare da template. Tests in `brain-app::api::merge_frontmatter_tests`.

6. ~~No auto-refresh after out-of-band commits~~ — **DONE 2026-04-24**. `RefreshBrainGraph` server fn + `RefreshButton` component in the knowledge header now trigger a full rebuild of the local SQLite projection after invalidating graph, template, and config caches. The button closes the "commit esterno → reload manuale" gap and doubles as manual reindex/drift recovery until webhook/SSE sync lands.

7. **Rename issues N+2 Contents API commits** — `rename_brain_file` commits once per backlinked file plus a create and a delete. Chosen for simplicity; migrate to Git Data API if commit churn becomes a complaint.

8. ~~Graph cache is process-global, not user- or target-scoped~~ — **DONE 2026-04-24**. All caches in `brain-storage` (graph, template) and `config_loader` are now keyed by `TargetKey({org}/{repo}/{branch})` via `OnceLock<Mutex<HashMap<TargetKey, _>>>`. `GithubHttp` is target-agnostic (plain `Arc<reqwest::Client>`); each `GithubStorage` / call-site supplies its own `GithubClient` built from an explicit `TargetConfig`. Safe for Phase 3 multi-target without re-architecting.

9. ~~`register_explicit` boilerplate is LTO-coupled~~ — **DONE 2026-04-24**. `SERVER_FNS: &[&str]` const + `include_str!`-based test in `api.rs` catches any `#[server]` fn not listed in the const before it reaches CI.

10. **UI limitations** — No animated transitions between viewBox states (snap is instant). Nodes near graph edges show empty area outside the data space. Hover does not recenter, only selection does. No zoom: scale stays 100×100.
