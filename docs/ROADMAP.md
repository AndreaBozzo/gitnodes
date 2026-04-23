# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

L'architettura si basa su Git come *Single Source of Truth* e su un database SQLite locale come *Materialized View* per garantire performance e aggirare i limiti delle API dei provider.

---

## 🟢 Fase 1: Astrazione e Configurabilità
Rimozione dell'hardcoding per rendere la Brain UI un tool generico e platform-agnostic, basato interamente su API REST.

**Principio guida:** Fase 1 deve essere non-breaking per installazioni esistenti e libera da speculazione architetturale. Abstraction layers (trait) e admin UI vengono rimandati alle fasi dove esiste un secondo consumer/use-case che ne valida la forma.

**Stato al 2026-04-23:** l'obiettivo tecnico della fase è sostanzialmente raggiunto. Tutti i deliverable infrastrutturali di de-hardcoding/configurabilità sono atterrati; restano aperti solo i workstream di convergenza prodotto e cutover sul caso reale `Brain`, necessari per dichiarare la fase chiusa anche operativamente.

- [x] **Frontmatter round-trip (prerequisito bloccante, ex-caveat #5)** — _2026-04-22_
    - `EditPrefill::from_raw` parsa l'intero frontmatter YAML in `BTreeMap<String, serde_yaml::Value>`.
    - `BrainFilePayload.preserved_frontmatter` propaga il dict all'update; `merge_frontmatter` fa overlay dei campi del form invece di rigenerare da template.
    - Campi custom (status, severity, cliente, ecc.) sopravvivono ai save.
- [x] **Config Loader + Schema + Default Migration** — _2026-04-22_
    - Parser YAML per `.brain-config.yml` con validazione al load (no duplicate directory, colori validi, nomi non riservati `tags`/`templates`).
    - Default config incluso nel binario (`BrainConfig::default()`), equivalente esatto all'attuale `NodeType` hardcodato (concept/adr/meeting/post-mortem/preventivo/runbook/tag). Repo senza config file funzionano identici a oggi: zero migrazione richiesta.
    - **Deviazione deliberata dalla prescrizione originale:** validazione fail-soft, non fail-fast all'avvio. Il server parte prima che esista un token autenticato per leggere il repo; parsing/validation failure ora loggano warning e cadono sul default. La fail-fast originale sposterà al banner UI del prossimo step (admin vede l'errore invece che a runtime crash).
    - TTL-cache in memoria 30s (allineata al pattern di `brain-storage`) + `invalidate()` pronto per i webhook push di Fase 2.
    - Server fn `load_brain_config` esposta, registrata in `register_explicit`.
- [x] **NodeType → Config Lookup**:
    - Sostituzione dell'enum `NodeType` in `brain-domain/src/types.rs` con `String` + lookup `Arc<BrainConfig>`.
    - Migrazione di `EditPrefill::from_raw`, `draft.rs` (localStorage schema-version bump per invalidare draft vecchi post-deploy), generazione template (consumare `NodeTypeSpec.frontmatter_seed`), ricavo directory/label/accent.
    - Prerequisito bloccante (frontmatter round-trip): ✅ risolto sopra.
- [x] **Graceful Fallback per Tipi Sconosciuti** — _2026-04-22_:
    - `config.lookup(type_str).unwrap_or(&config.default_spec())` applicato in tutti i render site (graph canvas, detail bar/panel, stats header, editor).
    - Banner UI amber su `/knowledge` (`orphan_banner.rs`) lista i tipi sconosciuti con conteggio + CTA esterno a `.brain-config.yml` via `GithubClient::config_blob_url()`. Dismissable per sessione.
    - Bonus (gap latente scoperto durante il lavoro): `frontmatter_malformed` ora rende un banner rosé nell'editor e disabilita il pulsante Save, matchando il refuse server-side che esisteva già.
- [x] **Creazione Guidata (Zero Typo)** — _2026-04-22_:
    - Menu "Nuovo Nodo" generato da `config.creatable()` (sostituito il filtro manuale `!directory.is_empty() || name != "tag"` che divergeva per tipi con `creatable: false` + directory non vuota).
    - Autocompilazione del frontmatter YAML (`NodeTypeSpec.frontmatter_seed`) e del path di salvataggio già attive da _2026-04-22_.
- [x] **`GithubClient` Centralizzato (URL-only, no trait)** — _2026-04-22_:
    - Nuova struct `GithubClient` in `brain-domain/src/config.rs` che possiede i quattro builder (`contents_url`, `tree_url`, `raw_base`, `blob_base`) più il nuovo `config_blob_url()` per il CTA del banner orfani.
    - `TargetConfig` tenuto come puro data carrier (`{org, repo, branch}`); le URL builders rimosse dall'`impl TargetConfig`.
    - Migrati tutti i 16 callsite (brain-storage, api.rs, config_loader, assets, markdown, detail_panel).
    - **NO `ForgeAdapter` trait in questa fase**: il design dell'abstraction viene fatto in Fase 4 contro un secondo adapter reale (GitLab/Gitea), non in astratto contro uno solo.
    - **Deferred esplicitamente** (vedi Fase 2 → "GithubClient: pooling + header centralization"):
        - `reqwest::Client` ricostruito ad ogni request; dovrebbe essere pooled dentro `GithubClient`.
        - `Authorization: Bearer {token}` + `User-Agent` settati a mano ad ogni chiamata; centralizzare in helper `GithubClient::get/put/delete`.
        - URL diretti in `brain-auth` (`/user`, `/orgs/.../members/...`, OAuth token endpoint) NON migrati — sono auth-domain, non repo-domain, e Fase 4 non li toccherà.
- [x] **`NodeTypeSpec`: campi per-tipo per eliminare hardcoded switches** _(chiuso 2026-04-23)_:
    - Promossi in `NodeTypeSpec` come `Option<String>` (tutti `#[serde(default, skip_serializing_if = "Option::is_none")]`): `title_key`, `date_create_field`, `date_update_field`, `body_label`.
    - `api.rs::merge_frontmatter` consulta lo spec per titolo e date (niente più match per nome).
    - `EditPrefill::from_raw(path, sha, raw, config)` risolve `title_key` via config, fallback a `"topic"` per tipi custom che lo omettono. Cross-fallback `progetto↔topic` rimossa.
    - `editor.rs::markdown_body_label` eliminata; label letto da `spec.body_label` con fallback `"Description"`.
    - `BrainConfig::default()` aggiornata per riprodurre il comportamento pre-change dei sette tipi built-in.
    - Test di round-trip custom-type aggiunti in `brain-domain/src/types.rs` (load) e `brain-app/src/api.rs` (save, create+update).
- [ ] **Convergenza del modello operativo su Work Items + binding provider**:
    - Definizione di un'entità interna `WorkItem` come modello operativo minimo e stabile (`brain_id`, `kind`, `state`, `labels`, `assignees`, `content_path`, binding esterno opzionale).
    - Le Issues dei forge diventano un **backend/binding** del modello, non la sua ontologia: supportano il caso principale ma non definiscono l'identità interna del sistema.
    - Definizione di una tassonomia minima di label (`brain:discussion`, `brain:task`, `brain:decision`, ecc.) e del mapping machine-readable in config, così la UI può derivare il comportamento senza introdurre nuovi switch GitHub-centrici.
    - Chiarimento del confine di responsabilità fra Markdown/frontmatter, projection locale e work item fields: stato, assignee, label, milestone e commenti devono avere una source of truth esplicita prima di atterrare in runtime.
    - Deliverable di chiusura della Fase 1: contratto prodotto/dati approvato sul modello `WorkItem` e sul binding provider. Projection runtime, sync e UI operativa atterrano in Fase 2; impersonation, review workflow e PR automation restano in Fase 3.
- [ ] **Dogfooding su repo Brain + cutover di produzione**:
    - Adozione di `.brain-config.yml` dentro il repo `Brain`, smettendo di dipendere dal default compilato nel binario come configurazione primaria.
    - Regressione end-to-end sul caso reale: create/edit/delete/rename/render per tipi built-in e almeno un tipo custom, inclusi `Related / See also`, template loading e fallback per tipi sconosciuti.
    - Verifica operativa in prod di sessioni/cookie, asset proxy, env vars e rollback path.
    - La Fase 1 si considera chiusa quando il deployment di produzione gira sul repo `Brain` in modalità config-driven senza regressioni funzionali.
### Spostati fuori da Fase 1

- **Pannello Impostazioni (Visual YAML Editor)** → **Fase 3**. È effettivamente un sotto-flusso admin-only e dipende da RBAC; fino ad allora gli admin editano YAML via commit come il resto del contenuto git-nativo.
- **`ForgeAdapter` trait** → **Fase 4**. Progettare un trait attorno a una singola implementazione produce quasi sempre la forma sbagliata; la riusiamo solo quando esiste un secondo adapter che la valida.


## 🟡 Fase 2: Operatività, Sync e Dati
Introduzione di flussi operativi, sincronizzazione in tempo reale e salvaguardia dell'integrità dei dati.

- [ ] **Projection Layer SQLite (dal session store alla materialized view del contenuto)**:
    - Espandere l'uso di SQLite oltre sessioni/audit: tabelle per `nodes`, `edges`, `files`, `backlinks`, `work_items`, `work_item_bindings` e stato dell'ultimo sync.
    - Costruire un bootstrap iniziale dal tree Git e una pipeline idempotente di upsert, così il frontend legge la proiezione locale invece di dipendere dal tree walk live ad ogni refetch.
    - Formalizzare Git come source of truth e SQLite come read model/write-through cache, evitando che Fase 3 introduca background jobs su una base ancora in-memory.
- [ ] **Fondazione del modello Work Item**:
    - Introdurre `task` come primo `work_item_kind` via config/template reali, non via hardcode, lasciando aperta l'estensione a `discussion`, `decision`, `incident`, ecc.
    - Rendere l'identità interna (`brain_id`) separata dal binding esterno (`GitHub #123`, `GitLab #456`, ...), così offline mode e multi-forge non dipendono dall'oggetto provider.
    - Decidere se il binding 1:1 `task ↔ issue` è una policy iniziale o una regola rigida, e coprire esplicitamente le eccezioni (`draft task`, `local-only item`, `provider item senza doc`).
    - Rendere esplicita la source of truth: Work Item come dominio operativo, provider issue come backend di collaborazione, Markdown/frontmatter come supporto editoriale e linking semantico.
    - Primo scope di UI: badge/stato/filtri/detail panel coerenti e visibilità del binding esterno. Commenti, review branch e PR restano fuori fino alla Fase 3.
- [ ] **Sincronizzazione inbound: Webhooks, projection refresh e SSE**:
    - Endpoint Axum per `push` e per gli eventi del backend operativo esterno (Issues/Work Items del forge) con validazione firma, idempotenza e fan-out verso invalidazione/selective refresh della proiezione locale.
    - Su `push`: invalidare solo il target corretto, aggiornare config/template/graph projection e pubblicare evento SSE al frontend.
    - Sugli eventi operativi esterni: aggiornare `work_items` e `work_item_bindings` senza aspettare un commit Git.
    - Lato Leptos: `EventSource` con reconnect/backoff e instradamento su segnali dedicati (`graph_version` o equivalenti) senza full route reset.
- [ ] **Completamento della resource invalidation lato write-path**:
    - L'infrastruttura base esiste già: save, delete e rename bumpano `graph_version`, che a sua volta refetcha `Resource::new_blocking`.
    - Resta da unificare il comportamento: stesso pathway di invalidazione per save locale, rename, delete e update inbound da SSE/webhook.
    - Preservare stato UI durante il refetch (selezione corrente, editor aperto, path rinominato) invece di trattare ogni refresh come hard reset implicito.
- [ ] **Gestione "Link Rot" con rename atomici**:
    - Lo use-case esiste già, ma oggi `rename_brain_file` esegue N+2 commit via Contents API (uno per referrer, poi create+delete).
    - Migrare a un'unica operazione logica via Git Data API (`blobs`/`trees`/`commit`/`refs`) per ottenere rename + rewrite backlink atomici e ridurre churn nella history.
    - Aggiornare nella stessa transazione la proiezione locale (`path`, backlink, eventuali riferimenti work-item/binding) per evitare drift temporanei fra Git e SQLite.
- [ ] **Scoping delle cache runtime** _(mandatory before Fase 3/4)_:
    - Oggi il problema non riguarda solo il grafo: cache process-global esistono in `brain-storage` (graph/template) e `config_loader` (config), con chiavi troppo deboli o assenti.
    - Prima di RBAC e multi-target bisogna rikeyare almeno per `target`; dove la visibilità può divergere serve anche lo scope per `user_id_or_role`.
    - L'invalidazione da webhook o write-path deve colpire solo le entry del target corretto, non azzerare stato globale del processo.
- [ ] **`GithubClient`: pooling, header centralization e seam per sync jobs** _(deferred da Fase 1)_:
    - Sostituire i `reqwest::Client::builder()...build()` per-request con un client pooled condiviso, riusato da storage, config loader, asset proxy e futuri sync jobs.
    - Esporre helper per `get/put/delete/post` che centralizzano `Authorization`, `User-Agent`, retry policy, eventuali conditional requests e logging.
    - Separare chiaramente il dominio repo/content dal dominio auth: gli endpoint OAuth possono restare fuori dal client condiviso finché non esiste un beneficio operativo netto.
- [ ] **Rebuild, reconciliation e drift recovery**:
    - I webhook non saranno perfetti: serve un full rebuild/manual reindex per ricostruire la projection layer se un evento viene perso o fallisce a metà.
    - Registrare watermark/lag/errori di sync in SQLite o audit log, così la Fase 3 può appoggiarsi a job di background osservabili invece che opachi.
    - Questo diventa il paracadute operativo per rate-limit shielding, multi-target e history navigation delle fasi successive.

---

## 🟠 Fase 3: Collaborazione e Sicurezza (RBAC)
Abilitare l'uso della piattaforma a team estesi, sfruttando l'App OAuth esistente.

- [ ] **Impersonation tramite OAuth**:
    - Utilizzo dell'access token univoco dell'utente loggato per eseguire azioni sul Forge a suo nome (es. commenti su work item bindati o apertura PR).
- [ ] **Workflow di Review (Branching & PR)**:
    - Flusso "Proponi Modifica" per utenti non-admin.
    - Creazione automatica di branch temporanei e apertura di Pull Request (tramite REST) direttamente dalla UI.
- [ ] **Rate-Limit Shielding**:
    - Centralizzazione delle chiamate al Forge tramite job in background di Axum. Il frontend interroga esclusivamente la cache SQLite.
- [ ] **Pannello Impostazioni (Visual YAML Editor)** _(spostato da Fase 1)_:
    - Interfaccia UI dedicata (solo `Admin`, dipende dall'RBAC di questa fase) per creare e modificare i "Tipi di Nodo" tramite form visuali (Color Picker, gestione cartelle, mapping `work_item_kind`/binding provider).
    - Il backend Axum serializza (`serde_yaml`) e pusha le modifiche a `.brain-config.yml` sul branch principale.

---

## 🔴 Fase 4: History & Multi-Forge
Esplorazione del passato e indipendenza totale dal provider.

- [ ] **Git Time Jump**:
    - Navigazione della history dalla UI (visualizzazione di un nodo a uno specifico SHA), mantenendo a schermo lo stato conversazionale corrente del `WorkItem` bindato quando presente.
- [ ] **`ForgeAdapter` Trait + Multi-Forge Support** _(trait spostato da Fase 1)_:
    - Estrazione del `trait ForgeAdapter` da `GithubClient` (Fase 1), progettato contro i requisiti reali dei nuovi adapter — non in astratto.
    - Sviluppo degli adapter ufficiali per **GitLab** e **Gitea/Codeberg**.
- [ ] **Offline Mode / Local Git**:
    - Supporto per repository locali senza un forge remoto.

---

## 🟣 Fase 5: AI & Automations Ecosystem
Trasformare la Brain UI in un assistente attivo tramite IA e trigger di automazione esterni.

- [ ] **AI Assistant Proxy (Copilot Integration)**:
    - Integrazione nell'editor UI (Leptos) di un assistente AI per generazione markdown, autocompletamento, summarization automatica e tagging.
    - Creazione di un proxy sicuro in Axum che sfrutti l'OAuth token dell'utente per interrogare le API AI di GitHub/Copilot (garantendo RBAC e accounting corretto).
- [ ] **Outbound Webhooks Engine**:
    - Motore di eventi in background (Axum) per inviare webhook a sistemi esterni (GitHub Actions, Zapier, CI/CD).
    - Configurazione dei trigger tramite il file `.brain-config.yml` (es. `on_work_item_done: https://...`).

### ROADMAP IS ALWAYS SUBJECT TO CHANGES AND REALIGNMENTS, this sketch is meant to be indicative of the direction.

---

## Known caveats

1. **CSRF `state_mismatch` on dropped session cookie** — `/auth/login` stores state in session, `/auth/callback` compares. If the browser drops the cookie between redirects (cross-site cookie policy, incognito) the callback returns `/?error=state_mismatch`. Likely culprit: `SameSite=Lax` vs. GitHub redirect chain. Fix only when it bites.

2. **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. ~~Update path regenerates frontmatter from templates~~ — **DONE 2026-04-22**. `EditPrefill::from_raw` ora parsa l'intero frontmatter in un `BTreeMap` YAML; `merge_frontmatter` (ex `generate_frontmatter`) fa overlay dei campi del form sulla mappa preservata invece di rigenerare da template. Campi custom (status, severity, cliente, ecc.) sopravvivono. Tests in `brain-app::api::merge_frontmatter_tests`.

6. **No auto-refresh after out-of-band commits** — the 30s TTL cache in `brain-storage/src/lib.rs` bounds staleness for edits made via `git push` directly. Acceptable; documented here so symptom isn't mistaken for a bug.

7. **Rename issues N+2 Contents API commits** — `rename_brain_file` in `crates/brain-app/src/api.rs` commits once per backlinked file plus a create and a delete, instead of one batched commit. Chosen for simplicity; repo is small and rename is rare. If commit churn becomes a complaint, migrate to the Git Data API (`POST /git/blobs` → `/git/trees` → `/git/commits` → `PATCH /git/refs/heads/{branch}`). The existing `rewrite_links` + backlink discovery are reusable.

8. **Graph cache is process-global, not user- or target-scoped** — `static Mutex<Option<CacheEntry>>` in `brain-storage/src/lib.rs:87-93`. Safe today (all authed org members see the same repo). **Becomes a bug in Phase 3** (RBAC: permission-differentiated views would leak across users) and **Phase 4** (multi-forge/multi-target: one process serving multiple repos). Key the cache by `(user_id_or_role, target)` before either phase lands.

9. **`register_explicit` boilerplate is LTO-coupled** — `api.rs::register_server_functions` manually lists every `#[server]` fn because `lto = true` in `[profile.release]` strips the `inventory::submit!` entries that `#[server]` relies on for auto-registration. Every new server fn must be added here or it silently 404s in release builds (dev builds still work, which makes the failure mode worse). Don't forget the line when adding a handler.

10. **UI limitations** — Nessuna transizione animata tra i due viewBox: lo snap è istantaneo.
Se il nodo è vicino ai bordi del grafo (x o y vicini a 0 o 100), il viewBox mostra area "vuota" fuori dallo spazio dei dati — il nodo resta centrato ma c'è bordo nero attorno.
Hover non ricentra, solo selected — coerente col fatto che hover è transitorio.
Non c'è zoom: la scala resta 100×100. Se i nodi sono molto vicini, selezionarne uno non "zooma dentro".