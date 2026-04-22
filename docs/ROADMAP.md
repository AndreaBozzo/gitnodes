# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

L'architettura si basa su Git come *Single Source of Truth* e su un database SQLite locale come *Materialized View* per garantire performance e aggirare i limiti delle API dei provider.

---

## 🟢 Fase 1: Astrazione e Configurabilità
Rimozione dell'hardcoding per rendere la Brain UI un tool generico e platform-agnostic, basato interamente su API REST.

**Principio guida:** Fase 1 deve essere non-breaking per installazioni esistenti e libera da speculazione architetturale. Abstraction layers (trait) e admin UI vengono rimandati alle fasi dove esiste un secondo consumer/use-case che ne valida la forma.

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
- [ ] **NodeType → Config Lookup**:
    - Sostituzione dell'enum `NodeType` in `brain-domain/src/types.rs` con `String` + lookup `Arc<BrainConfig>`.
    - Migrazione di `EditPrefill::from_raw`, `draft.rs` (localStorage schema-version bump per invalidare draft vecchi post-deploy), generazione template (consumare `NodeTypeSpec.frontmatter_seed`), ricavo directory/label/accent.
    - Prerequisito bloccante (frontmatter round-trip): ✅ risolto sopra.
- [ ] **Graceful Fallback per Tipi Sconosciuti**:
    - `config.lookup(type_str).unwrap_or(&config.default_type)` — niente `ParseError`, niente crash WASM su draft vecchi o nodi con `type:` non più censito.
    - Banner UI sui nodi orfani con CTA "aggiungi questo tipo a `.brain-config.yml`".
    - Nota: `#[serde(other)]` NON si applica qui perché `NodeType` non è più un enum; il fallback è sul lookup, non sulla deserializzazione.
- [ ] **Creazione Guidata (Zero Typo)**:
    - Menu "Nuovo Nodo" generato dinamicamente dalla config attiva.
    - Autocompilazione del frontmatter YAML e del path di salvataggio.
- [ ] **`GithubClient` Centralizzato (no trait yet)**:
    - Tutte le costruzioni di URL verso `api.github.com` centralizzate in una singola struct `GithubClient` in `brain-domain/src/config.rs` o crate dedicato.
    - **NO `ForgeAdapter` trait in questa fase**: il design dell'abstraction viene fatto in Fase 4 contro un secondo adapter reale (GitLab/Gitea), non in astratto contro uno solo.
- [ ] **Unified Issue System ("Tutto è un'Issue")**:
    - Abbandono delle dipendenze da feature proprietarie (es. GitHub Discussions).
    - Mapping di ogni entità comunicativa sulle **Issues**, usando **Labels** (es. `brain:discussion`, `brain:task`) per differenziare il comportamento UI.

### Spostati fuori da Fase 1

- **Pannello Impostazioni (Visual YAML Editor)** → **Fase 3**. È effettivamente un sotto-flusso admin-only e dipende da RBAC; fino ad allora gli admin editano YAML via commit come il resto del contenuto git-nativo.
- **`ForgeAdapter` trait** → **Fase 4**. Progettare un trait attorno a una singola implementazione produce quasi sempre la forma sbagliata; la riusiamo solo quando esiste un secondo adapter che la valida.


## 🟡 Fase 2: Operatività, Sync e Dati
Introduzione di flussi operativi, sincronizzazione in tempo reale e salvaguardia dell'integrità dei dati.

- [ ] **Type: Task**:
    - Introduzione del nodo di tipo operativo per il tracciamento di attività, linkato 1:1 a un'Issue (Stato Open/Closed, Assignees).
- [ ] **Sincronizzazione Esterna (Inbound Webhooks & SSE)**:
    - Endpoint in Axum dedicato alla ricezione di Webhooks (`push`, `issues`) dal Forge.
    - Invalidazione della cache SQLite e invio di eventi **SSE (Server-Sent Events)** al frontend Leptos per l'Hot Reload del grafo.
- [ ] **Gestione "Link Rot" (Rename Transazionali)**:
    - Implementazione sicura del cambio nome file/cartella tramite transazioni `git mv`.
    - Aggiornamento contestuale del campo `path` in SQLite per mantenere intatto il legame storico.

---

## 🟠 Fase 3: Collaborazione e Sicurezza (RBAC)
Abilitare l'uso della piattaforma a team estesi, sfruttando l'App OAuth esistente.

- [ ] **Impersonation tramite OAuth**:
    - Utilizzo dell'access token univoco dell'utente loggato per eseguire azioni sul Forge a suo nome (es. commenti o apertura PR).
- [ ] **Workflow di Review (Branching & PR)**:
    - Flusso "Proponi Modifica" per utenti non-admin.
    - Creazione automatica di branch temporanei e apertura di Pull Request (tramite REST) direttamente dalla UI.
- [ ] **Rate-Limit Shielding**:
    - Centralizzazione delle chiamate al Forge tramite job in background di Axum. Il frontend interroga esclusivamente la cache SQLite.
- [ ] **Pannello Impostazioni (Visual YAML Editor)** _(spostato da Fase 1)_:
    - Interfaccia UI dedicata (solo `Admin`, dipende dall'RBAC di questa fase) per creare e modificare i "Tipi di Nodo" tramite form visuali (Color Picker, gestione cartelle, mapping Issue/Task).
    - Il backend Axum serializza (`serde_yaml`) e pusha le modifiche a `.brain-config.yml` sul branch principale.

---

## 🔴 Fase 4: History & Multi-Forge
Esplorazione del passato e indipendenza totale dal provider.

- [ ] **Git Time Jump**:
    - Navigazione della history dalla UI (visualizzazione di un nodo a uno specifico SHA), mantenendo a schermo i commenti attuali dell'Issue collegata.
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
    - Configurazione dei trigger tramite il file `.brain-config.yml` (es. `on_task_close: https://...`).

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