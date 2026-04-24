# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

L'architettura si basa su Git come *Single Source of Truth* e su un database SQLite locale come *Materialized View* per garantire performance e aggirare i limiti delle API dei provider.

---

## 🟢 Fase 1: Astrazione e Configurabilità

Rimozione dell'hardcoding per rendere la Brain UI un tool generico e platform-agnostic, basato interamente su API REST.

**Principio guida:** Fase 1 deve essere non-breaking per installazioni esistenti e libera da speculazione architetturale. Abstraction layers (trait) e admin UI vengono rimandati alle fasi dove esiste un secondo consumer/use-case che ne valida la forma.

**Stato al 2026-04-24:** tutti i deliverable tecnici e di contratto-prodotto sono chiusi. Il merge `staging → master` è avvenuto; la verifica operativa in produzione sul repo `Brain` con `.brain-config.yml` esplicito è l'ultimo passo per dichiarare la fase chiusa operativamente.

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
    - Regressione e-2-e e verifica operativa in prod da completare post-deploy.

### Spostati fuori da Fase 1

- **Pannello Impostazioni (Visual YAML Editor)** → Fase 3. Admin-only, dipende da RBAC.
- **`ForgeAdapter` trait** → Fase 4. Progettare un trait su una sola implementazione produce quasi sempre la forma sbagliata.

---

## 🟡 Fase 2: Operatività, Sync e Dati

Introduzione di flussi operativi, sincronizzazione in tempo reale e salvaguardia dell'integrità dei dati.

- [ ] **Projection Layer SQLite**
    - Espandere SQLite oltre sessioni/audit: tabelle `nodes`, `edges`, `files`, `backlinks`, `work_items`, `work_item_bindings`, stato ultimo sync.
    - Schema multi-tenant fin dal giorno zero: ogni tabella ha `target_id` come FK, così il backend è multi-repo capable anche se la UI rimane ancorata a un singolo target per sessione. Risolve il Caveat #8 senza una migrazione successiva.
    - Bootstrap iniziale dal tree Git + pipeline idempotente di upsert; il frontend legge la proiezione locale invece del tree walk live.
    - Formalizzare Git come source of truth e SQLite come read model/write-through cache.
- [ ] **Fondazione operativa Work Item**
    - Introdurre `task` come primo `work_item_kind` via config/template reali, non via hardcode.
    - Decidere se il binding 1:1 `task ↔ issue` è policy iniziale o regola rigida; coprire le eccezioni (`draft task`, `local-only item`, `provider item senza doc`).
    - Primo scope UI: badge/stato/filtri/detail panel e visibilità del binding esterno. Commenti, review branch e PR restano fuori fino alla Fase 3.
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
- [ ] **Scoping delle cache runtime** _(prerequisito per Fase 3/4)_
    - Cache process-global in `brain-storage` (graph/template) e `config_loader` (config) hanno chiavi troppo deboli.
    - Rekeying per `target_id` come minimo; dove la visibilità diverge per ruolo, aggiungere `user_id_or_role`.
    - L'invalidazione da webhook o write-path deve colpire solo le entry del target corretto.
- [ ] **`GithubClient`: pooling + header centralization** _(deferred da Fase 1)_
    - `reqwest::Client` pooled condiviso tra storage, config loader, asset proxy e sync jobs.
    - Helper `get/put/delete/post` che centralizzano `Authorization`, `User-Agent`, retry policy e logging.
- [ ] **Rebuild, reconciliation e drift recovery**
    - Full rebuild/manual reindex per ricostruire la projection se un webhook viene perso o fallisce a metà.
    - Watermark/lag/errori di sync in SQLite o audit log — base osservabile per i background job di Fase 3.

---

## 🟠 Fase 3: Collaborazione e Sicurezza (RBAC)

Abilitare l'uso della piattaforma a team estesi, sfruttando l'App OAuth esistente.

- [ ] **Impersonation tramite OAuth**
    - Uso dell'access token univoco dell'utente loggato per azioni sul forge a suo nome (commenti su work item bindati, apertura PR).
- [ ] **Multi-Repo Workspace UX**
    - "Brain Switcher" nella UI (menu laterale) per navigare tra repository dell'organizzazione senza cambiare URL.
    - Discovery via token OAuth: scansione dei repo accessibili all'utente e listing automatico di quelli che espongono `.brain-config.yml`. Dipende dall'impersonation dello stesso step.
    - Gestione graceful degli errori di autorizzazione: se l'utente non ha permessi Git su un Brain, messaggio esplicito senza crash.
    - Il backend diventa effettivamente multi-repo (richiede Projection Layer SQLite multi-tenant di Fase 2 come prerequisito).
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
    - Estrazione del `trait ForgeAdapter` da `GithubClient`, progettato contro i requisiti reali dei nuovi adapter.
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

6. **No auto-refresh after out-of-band commits** — the 30s TTL cache bounds staleness for edits made via `git push` directly. Acceptable; documented here so the symptom isn't mistaken for a bug.

7. **Rename issues N+2 Contents API commits** — `rename_brain_file` commits once per backlinked file plus a create and a delete. Chosen for simplicity; migrate to Git Data API if commit churn becomes a complaint.

8. **Graph cache is process-global, not user- or target-scoped** — `static Mutex<Option<CacheEntry>>` in `brain-storage/src/lib.rs`. Safe today. **Becomes a bug in Phase 3** (RBAC) and **Phase 4** (multi-target). Must be rekeyed before either phase lands — partially addressed by the Projection Layer SQLite multi-tenant design in Fase 2.

9. **`register_explicit` boilerplate is LTO-coupled** — `api.rs::register_server_functions` manually lists every `#[server]` fn because `lto = true` strips the `inventory::submit!` entries. Every new server fn must be added here or it silently 404s in release builds (dev builds still work, making the failure mode worse).

10. **UI limitations** — No animated transitions between viewBox states (snap is instant). Nodes near graph edges show empty area outside the data space. Hover does not recenter, only selection does. No zoom: scale stays 100×100.
