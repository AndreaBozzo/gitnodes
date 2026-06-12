# Brain UI Roadmap — Archive

This file holds the original prose of phases that have closed. The forward-looking spec lives in [ROADMAP.md](ROADMAP.md). When a phase or hardening item closes, its detailed rationale moves here in the same PR that flips the last checkbox, leaving only a one-line summary + link in the active roadmap.

---

## 🟢 Fase 1: Astrazione e Configurabilità _(closed 2026-04-24)_

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
    - `WorkItem` in `brain-domain/src/work_items.rs`: `brain_id`, `kind` (Task/Discussion/Decision/Incident/Change/Quote), `state`, `labels`, `assignees`, `content_path`, `external_binding` opzionale, `system_of_record`.
    - `ExternalWorkItemBinding`: `system`, `project`, `item_key`, `provider_id?`, `url?`. Le issue del forge sono un binding, non l'ontologia.
    - `WorkItemLabelSpec` + `label_taxonomy` in `BrainConfig`: mapping machine-readable `kind → forge_label` e `state → forge_label`. Kind built-in con label `brain:*`. Helper `labels_for_kind()`, `all_kind_labels()`.
    - Source-of-truth dichiarata: frontmatter = verità editoriale; `WorkItem` = dominio operativo; SQLite = read model (Fase 2); forge issue = backend di collaborazione opzionale.
- [x] **Dogfooding su repo Brain** — _2026-04-24_
    - `.brain-config.yml` creato nel repo Brain: tutti e sette i tipi + `label_taxonomy`. Equivalente 1:1 a `BrainConfig::default()`.
    - La verifica residua viene ridotta a smoke test operativo su deploy/configurazione, senza riaprire la fase sul piano architetturale.

### Spostati fuori da Fase 1

- **Pannello Impostazioni (Visual YAML Editor)** → Fase 3. Admin-only, dipende da RBAC.
- **`ForgeAdapter` trait** → Fase 4. Progettare un trait su una sola implementazione produce quasi sempre la forma sbagliata.

---

## 🟡 Fase 2: Operatività, Sync e Dati _(closed 2026-04-25)_

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

## 🟠 Fase 3: Workspace Collaborativo, Work Item & RBAC _(closed 2026-05-02 scope freeze; dogfooding gate)_

Abilitare un workspace realmente multi-tenant e collaborativo sopra le fondamenta già chiuse in Fase 2B. Il focus non è più "aggiungere OAuth" in astratto, ma separare chiaramente tre piani: **routing del target**, **mutazioni operative dei work item**, **orchestrazione permission-aware delle scritture**.

**Principio guida:** il backend ha già le primitive giuste (`TargetKey`, projection SQLite per target, `brain_id` stabile, write path GitHub centralizzato). La Fase 3 deve evitare nuove scorciatoie single-target e introdurre ogni nuova capability come target-aware fin dal primo commit.

**Scope freeze 2026-05-02:** Fase 3 non accetta più nuove capability platform. Da qui alla chiusura entrano solo: bugfix, polish piccolo, documentazione operativa, e follow-up estratti dal dogfooding se bloccano davvero l'uso con contributor limitato. Tutto il resto viene spostato in "Next hardening" o "Future product expansion".

- [x] **3.1 Multi-Tenant Workspace Routing**
    - Evolvere il router Leptos da path statici (`/knowledge`, `/admin`) a path dinamici `/{org}/{repo}/knowledge` e `/{org}/{repo}/admin`, mantenendo eventuali redirect dal target di default solo come compat layer temporaneo.
    - Spostare la risoluzione del `TargetConfig` dal boot statico del server al contesto della request/pagina. `GithubHttp` pooled è già target-agnostic; ciò che manca è togliere l'assunzione che asset proxy, SSR context e webhook state puntino a un solo repo definito da env.
    - Introdurre il **Brain Switcher** nella sidebar: discovery via token OAuth dei repo accessibili all'utente che contengono `.brain-config.yml`, con stato esplicito per `accessible / missing-config / forbidden`.
    - Success criterion: la stessa istanza Brain UI può navigare più repo/target senza restart né collisioni di cache/projection, e l'URL diventa l'identificatore canonico del target attivo.

- [x] **3.2 Work Items Interattivi e Bidirectional Sync** _(α+β landed 2026-04-27)_
    - **3.2-α (landed)** — UI → file → projection → SSE loop chiuso, sempre con `system_of_record = brain` lato write:
      - `BrainEvent::WorkItemUpdated { brain_id, content_path }` e `BindingUpdated { ... }` aggiunti al bus tipato; `LiveSync` ora sottoscrive entrambi gli event name.
      - `EventBus::init()` / `global()` espone il bus come singleton process-wide così le server fn possono pubblicare senza prendere il bus in firma.
      - Mutazioni projection single-row: `update_work_item_state`, `update_work_item_assignees`, `upsert_work_item_binding`, `find_work_item_by_external` in `server::projection`.
      - Tre server fn mutate-only: `TransitionWorkItem`, `AssignWorkItem`, `BindWorkItem`. Pattern condiviso in `apply_work_item_mutation`: load file → patch frontmatter mirato (overlay del singolo campo, custom keys preservate) → singolo commit via `GithubStorage::save_file` → patch projection → publish SSE → return record refreshed. Audit log per ogni kind (`work_item_transition`/`assign`/`bind`).
      - UI controls inline nel `DetailPanel::WorkItemCard`: `<details>` collapsible con state dropdown, assignees CSV input, binding form (system/project/item_key/url) + Unbind. Action separate per kind di mutazione, errori scoped, `graph_version` bumpato su success per refetch.
      - Webhook pipeline esteso a `issues`/`pull_request`: parsing minimale (repo + number), gate sul target repo, lookup binding via `find_work_item_by_external`. Item non bindati → silently 202; bindati → rebuild projection + emit `WorkItemUpdated { brain_id }` con il `brain_id` reale del documento.
    - **3.2-β (landed)** — bidirectional sync sopra la base α:
      - Provider-side push delle mutazioni quando `system_of_record` è `split` o `external`: `GithubHttp::issue_labels` + `patch_issue` leggono le label correnti, preservano le label non-Brain, applicano lo stato GitHub (`done/cancelled → closed`, altri stati → `open`), sincronizzano gli assignee e auditano i failure provider-side senza rollback dell'editorial save.
      - Coerenza `system_of_record` esplicita: `brain` = no-op provider; `split`/`external` = commit editoriale Brain + push provider quando il binding è GitHub. Le mutazioni provider-originated rientrano senza echo di ritorno.
      - Webhook `issues`/`pull_request`: per item bindati legge state/labels/assignees dal payload, mappa le label `label_taxonomy` allo stato Brain, aggiorna il file Markdown/projection con `GITHUB_TOKEN` e pubblica `WorkItemUpdated`.
    - Success criterion chiuso: un cambio di stato fatto in UI aggiorna l'Issue GitHub corrispondente con il token dell'utente; un update GitHub out-of-band su item già bindato rientra in projection/UI senza refresh manuale.
    - Follow-up residui: `issue_comment` come evento SSE incrementale puro resta da valutare insieme alla query layer parametrica/rate-limit shielding. Il read path dei commenti issue bindati è stato introdotto il 2026-04-28 con `LoadWorkItemComments(brain_id)` + sezione `Comments` nel work item detail panel; per ora legge live da GitHub con il token OAuth dell'utente e non persiste la timeline in SQLite.

- [x] **3.3 RBAC e Save Orchestration Permission-Aware** _(landed 2026-04-27)_
    - Smettere di modellare RBAC come semplice flag admin/non-admin. Il controllo reale è una capability matrix per target: `can_read`, `can_write_default_branch`, `can_review_via_pr`, `can_admin_config`, derivata da sessione OAuth + permessi repo/branch.
    - Riutilizzare la logica di tree commit atomico già introdotta per i rename, generalizzandola in un write path capace di scegliere tra:
      - commit diretto sul branch target se l'utente ha write access;
      - branch temporaneo `patch-{user}-{timestamp}` + apertura PR se non può scrivere direttamente ma può proporre review.
    - Tutte le azioni di scrittura ad alto livello (`save`, `rename`, `config update`, future work item mutations che toccano file) devono passare per questo orchestratore, così il fallback PR non diventa un caso speciale fragile.
    - La UI deve rendere esplicito l'esito: `Saved to main`, `Proposed via PR`, `Blocked by permissions`, con link a branch/PR e audit coerente.
    - Success criterion: un contributor senza write access può usare Brain UI normalmente; il sistema devia automaticamente su branch+PR senza perdere atomicità o metadata di autore.
    - Implementazione corrente:
      - `GetWriteCapabilities` espone la capability matrix per target (`read`, `write default branch`, `review via PR`, `admin config`) dai permessi repo GitHub.
      - Save, delete, rename atomico e mutazioni work item passano dal write orchestrator: direct commit quando possibile; fallback a branch temporaneo + PR quando il branch target rifiuta la scrittura o quando serve un fork utente.
      - I fallback PR non mutano la projection locale del branch target finché la PR non viene mergiata; la UI mostra `Proposed via PR #...` invece di simulare un save live.
      - Audit dedicato per `propose_write`, `propose_delete`, `propose_rename`, `propose_work_item_mutation`.
    - Follow-up non bloccanti:
      - Il futuro Visual Configuration Editor di 3.4 deve riusare lo stesso orchestratore.
      - L'upload asset resta direct-write oriented perché un asset proposto via PR non è immediatamente referenziabile dal markdown live; chiusura prevista nella slice BYOB α (BlobAdapter + R2 → la PR contiene solo diff Markdown, preview live funzionante).

- [x] **3.4 Visual Configuration Editor** _(spostato da Fase 1)_

    **Realignment 2026-04-27:** la fase viene spezzata in slice verticali (α → β → γ) invece di un singolo drop visual editor completo. Razionale: la roundtrip GUI ↔ YAML ↔ orchestratore di scrittura è il vero rischio architetturale; isolarlo sulla dimensione di config più piccola consente di iterare sui feedback prima di estendere la GUI all'intera superficie di `BrainConfig`. Ogni slice è indipendentemente shippabile e dogfoodabile.

    Vincoli condivisi (validi per α/β/γ):
    - Il backend continua a serializzare YAML con `serde_yaml` (no schema parallelo frontend); ogni save passa dall'orchestratore permission-aware della 3.3 (`save_file_permission_aware`) → commit diretto per admin con write access, PR proposta altrimenti, blocked se nessuna delle due opzioni è permessa.
    - Validazione = parser runtime esistente (`BrainConfig::parse/validate`), esteso quando serve. Niente regex client-side che possano divergere.
    - Save = re-serialize del file intero a partire da `BrainConfig` in memoria; le sezioni non toccate dalla slice corrente devono sopravvivere round-trip senza riformattazione lossy.
    - GUI sotto `/{org}/{repo}/admin/...`, gated su `can_admin_config` dalla capability matrix.

    - **3.4-α Saved Views (vertical slice)** _(scope minimo per validare il pattern)_
        - Nuovo blocco `views` in `BrainConfig`: `Vec<ViewSpec { name, slug, tags, types }>`. Ogni view è un named tuple di filtri già URL-persistenti (`?tags=`, `?types=`); niente nuove dimensioni di filtro. Slug auto-generato da `name` con fallback a override esplicito quando l'utente vuole controllarlo o risolvere collisioni.
        - Validazione estesa: slug univoci nel target, tags lowercase, ogni `type` referenziato deve esistere in `node_types`. Round-trip YAML deve preservare gli altri campi del file invariati.
        - Server fn nuove: `ListViews(target)` (read da config cached), `SaveViews(target, Vec<ViewSpec>) → SaveOutcome` (riusa l'orchestratore 3.3, audit `update_views` / `propose_views_update`, invalidate config cache su direct commit).
        - Route admin dedicata `/{org}/{repo}/admin/views` (separata dall'admin esistente, coerente col routing multi-tenant 3.1): form add/edit/delete con tag chips + type chips, banner outcome (`Saved` / `Proposed via PR #...` / `Blocked by permissions`).
        - Sidebar Knowledge: render delle view come chip cliccabili che impostano `?tags=&types=`. Implementazione triviale perché i filtri sono già URL-persistenti — niente nuovo state management.
        - Esplicitamente fuori da α: "create view from current filters" (rivalutato dopo dogfooding), per-view ordering UI oltre l'ordine array, view-scoped permissions.
        - Success criterion: un admin crea/modifica una view dalla GUI, il file `.brain-config.yml` viene aggiornato con un singolo commit (o PR), il banner della Knowledge sidebar mostra la nuova view senza refresh manuale, gli altri campi del file restano byte-identici alla sezione esterna a `views`.

    - **3.4-β Editorial config (post-feedback expansion)** _(da pianificare dopo che α è in produzione e raccoglie feedback)_
        - Estendere la GUI a `node_types` (name, directory, accent, frontmatter_seed minimo) e `label_taxonomy` (`work_item_kind` mapping + state labels), che sono le dimensioni più toccate dopo le views.
        - Forma esatta dei form e granularità dei campi vanno disegnate alla luce di cosa emerge dall'uso reale di α; pianificarle ora produrrebbe quasi certamente la forma sbagliata.

    - **3.4-γ Brand & long-tail config** _(deferred — raw YAML resta accettabile finché α/β non maturano)_
        - `brand`, asset settings, eventuali futuri `graph_visual_settings`. Bassa frequenza di edit, l'escape hatch raw YAML copre il caso fino a che β non dimostra che il pattern scala su più dimensioni.

- [x] **3.5 Rate-Limit Shielding e Background Reconciliation**
    - Una volta introdotte mutazioni work item e discovery multi-repo, spostare le chiamate GitHub più costose dietro job/reconcile espliciti e usare SQLite come cache operativa interrogabile dal frontend.
    - La query layer SQLite esposta al frontend deve essere parametrica (`list_nodes(target, filters)`, `list_work_items(target, filters)`, `read_node(target, path)`) invece di server fn bespoke per schermata: stessa shape che servirà a un futuro endpoint MCP in Fase 5, senza commitarsi al protocollo ora.
    - Questo asse non è l'entry point della fase, ma diventa necessario appena la UI smette di leggere solo il repo attivo e comincia a scansionare repo, issue e PR per utente.

- [x] **3.6 Graph Canvas Polish (zoom, transitions, edge framing)**
    - Promuovere il viewBox SVG da `"0 0 100 100"` hardcodato a un signal `(cx, cy, scale)` guidato da wheel/pinch/`+`/`-`/reset, per uscire dal "scale fisso 100×100" attuale.
    - Sostituire lo snap istantaneo del viewBox sulla selezione con un tween RAF-driven (~300ms) — `viewBox` non è transitionable via CSS, va animato esplicitamente.
    - Reframing dei nodi vicini al bordo del data space (oggi mostrano area vuota) e opzionale recentering anche su hover, non solo su selezione.
    - Vincolo: niente D3 — ~30KB di JS aggiuntivi non si sposano con il bundle Leptos+WASM. La logica resta in Rust/SVG nativo.
    - Success criterion: zoom continuo 0.25×–4×, transizioni morbide tra stati di selezione, nessuna area vuota visibile sui bordi del grafo per repo realistici.

- [x] **3.7 Repo Structure Transparency** — **DONE 2026-04-30**. MVP shippato sopra la projection SQLite: `list_files` (server fn parametrica introdotta come estensione della query layer di 3.5, vedi `crates/brain-app/src/api/files.rs`) espone `files` + metadata nodo/work-item/orphan senza leggere il forge; Knowledge UI ha tree read-only opt-in nella sidebar con cartelle espandibili, conteggi, badge per cartelle che contengono solo work item / solo nodi di un certo tipo, badge `isolated` per-cartella e toggle globale `N isolated markdown files` che attiva il filtro `?orphan=true`; stato espanso/collapsed persistito in localStorage per target; `?path_prefix=` e `?orphan=true` filtrano il grafo insieme a tag/type; l'editor mostra preview live del path finale e segnala cartelle nuove implicite; il detail panel mostra breadcrumb cliccabile che filtra per directory e filename linkato al blob GitHub.

    Success criterion verificabile: il filtro `?path_prefix=` e `?orphan=true` sopravvivono a refresh; lo stato espanso del tree sopravvive a navigation tra target; breadcrumb del detail panel è clickable end-to-end (segmento → filtro grafo, filename → blob GitHub).

    Razionale: oggi Brain UI mostra il grafo logico (nodi/edge derivati dal frontmatter e dai wiki link) ma rende quasi opaca la struttura fisica del repo. Una save/rename può atterrare in `runbooks/`, `concepts/sub_folder/`, `concepts/bozza-manifesto/` o creare implicitamente una nuova cartella, e l'utente non ha un modo immediato di vedere cosa esiste già, dove sta cosa, quanti file ci sono per directory, quale path verrà davvero generato. Il `LocationPicker` attuale (`editor.rs:1195`) è un input testuale con datalist piatto delle cartelle esistenti — utile per autocomplete, inutile come mappa.

    Obiettivo: rendere la struttura fisica di prima classe accanto a quella logica, **senza** trasformare Brain UI in un file explorer generico (non è un IDE) e **senza** introdurre un secondo modello — la projection SQLite ha già `files` e `nodes` con `content_path`, basta esporli con la forma giusta.

    - **Tree view sidebar (read-only, opt-in)** — sotto il filter panel della Knowledge view, una sezione collapsible "Repository structure" con albero gerarchico costruito da `list_files(target)` (server fn parametrica già introdotta in 3.5, da estendere se necessario): cartelle espandibili, conteggio file per cartella, badge per cartelle che contengono solo work item o solo nodi di un certo tipo. Click su un file = stessa azione di click sul nodo del grafo (focus + detail panel). Stato espanso/collapsed persistito in localStorage per target.
    - **Path preview live nell'editor** — sopra/sotto il `LocationPicker`, una riga "Will be saved as: `runbooks/2026/04/foo-bar.md`" che si ricalcola in tempo reale dal `folder` + `node_type.directory` + slug derivato dal title. Evidenziare visivamente quando il path implica la creazione di una cartella nuova (`new folder: drafts/q3/`) — oggi succede silenziosamente.
    - **Detail panel: breadcrumb cliccabile** — il path stringa attuale diventa un breadcrumb `org/repo › concepts › sub_folder › TestbrainUI.md` dove ogni segmento, se cliccato, filtra il grafo sui nodi sotto quella cartella (riusando `?path_prefix=` da aggiungere alla query layer parametrica di 3.5). Il filename finale apre il blob su GitHub in nuova tab, coerente con il pattern già usato per il config file.
    - **Filtro "by directory" parallelo a tag/type** — un terzo asse di filtro nella sidebar che usa la struttura cartelle reale come dimensione ortogonale a `tags`/`types`. URL-persistito come `?path_prefix=concepts/`. Niente nuovo state management — segue il pattern di 3.4-α.
    - **Banner orphan strutturale** — oggi esiste il banner amber per tipi di nodo sconosciuti (Fase 1). Equivalente strutturale: file markdown nel repo che la projection ha indicizzato ma che non sono raggiungibili da nessun link wiki né da nessuna view (silos editoriali). Banner discreto con conteggio + link al filtro `is_orphan_in_graph: true`. Aiuta a vedere cosa c'è di "perso" nel repo.

    Esplicitamente fuori da 3.7: drag-and-drop tra cartelle (è una move/rename — vive sotto Admin Node Control con `BranchTransaction` di 4.0), creazione cartella esplicita ex-nihilo (le cartelle restano implicite — caveat di design), upload di file binari arbitrari (non-markdown), preview anteprima file non-markdown.

    Vincolo: niente nuova server fn per ogni nuova superficie — la query layer parametrica di 3.5 (`list_files`, `list_nodes` con filtri) deve coprire tutti i casi sopra. Se uno di questi richiede una nuova fn, è un segnale che 3.5 va completata prima.

    Success criterion: un utente che apre per la prima volta un repo Brain capisce in <30 secondi quali cartelle esistono, quanti file contengono, dove finirà un nuovo documento e quali file sono "isolati" rispetto al grafo, senza dover aprire GitHub.

- [x] **3.7B Canonical TargetRef & Trust Boundary Preflight** — **DONE 2026-05-01** _(emerged from architecture review, 2026-05-01)_

    Razionale: la multi-tenancy funziona già nel percorso principale, ma il runtime ha ancora alcuni punti dove l'identità del target è implicita: server function che derivano `TargetConfig` dalla route corrente o dal `Referer`, URL multi-tenant senza branch, discovery che conosce `default_branch` ma non lo conserva nel link canonico, webhook ancora modellato soprattutto intorno al target env. Prima di aggiungere iframe, blob esterni e MCP/AI conviene chiudere questo strato: il target attivo deve essere un dato esplicito, verificabile e tracciabile in ogni read/write.

    - [x] **3.7B-α — TargetRef canonico in URL e API** — **DONE 2026-05-01**. Introdotto `TargetRef { org, repo, branch }` in `brain-domain` come identità serializzabile/validabile, mantenendo `TargetConfig` come wrapper runtime per storage/config loader. Aggiunte route canoniche `/{org}/{repo}/{branch}/knowledge` e `/{org}/{repo}/{branch}/admin[/views]`; le route legacy `/{org}/{repo}/...` restano compat e risolvono sticky branch via `target_registry`. Le server fn target-scoped usate da Knowledge/Admin ora ricevono `TargetRef` esplicito per graph/config target, refresh, file read/write/delete/rename/upload, folders, template, work item mutation/comment e saved views; le pagine SSR derivano il target dai route params o dal resolver legacy, non dal `Referer`. `target_from_path` resta confinato all'asset proxy legacy. Verificato con `cargo check -p brain-app --features ssr`, `cargo check -p brain-app --features hydrate`, `cargo test -p brain-app --features ssr`.
    - [x] **3.7B-β — Brain Switcher target states + branch persistito** — **DONE 2026-05-01**. `targets.default_branch` viene migrato/popolato e aggiornato dalla discovery dello switcher senza sovrascrivere il branch operativo sticky. `AccessibleTarget` ora espone `active_branch`, `default_branch` e `AccessibleTargetState` (`Accessible`, `MissingConfig`, `Forbidden`, `BranchMissing`, `ConfigInvalid`); il probe valida `.brain-config.yml` invece di limitarsi al booleano di presenza. Lo switcher linka al branch attivo, mostra lo stato per target non navigabili e aggiunge badge quando `active_branch != default_branch`.
    - [x] **3.7B-γ — Webhook routing per TargetRef reale + SSE target-scoped** — **DONE 2026-05-01**. I webhook push estraggono `TargetRef` da `repository.full_name` + `refs/heads/*` e lo confrontano con il target registrato; item event (`issues`/`pull_request`) risolvono il target via registry repo, non più da `WebhookState.target`. Target non registrati o branch mismatch rispondono `202 Accepted` con audit/debug e non rebuildano il target env. `BrainEvent` ora include `target: TargetRef` in ogni payload; `LiveSync` filtra lato client contro il target canonico corrente, così tab su target diversi non si invalidano a vicenda.
    - Success criterion: aprendo due target diversi in tab diverse, ogni server fn, asset proxy, SSE/refetch e webhook opera sul target corretto senza dipendere dal `Referer`; il branch è parte dell'identità canonica, non un dettaglio nascosto.

- [x] **3.7F Frontend Component Library Posture** — _baseline DONE 2026-05-02_

    Razionale: la UI Leptos è cresciuta a forza di utility Tailwind hand-rolled (136 occorrenze color-utility censite in `crates/brain-app/src/`, banner/button/pill idiom ripetuti tra `editor.rs` 1469 LoC e `page.rs` 554 LoC). Il rischio non è estetico ma di drift: ogni nuova superficie reinventa il proprio bottone primario/ghost, e la coerenza visiva regredisce a ogni feature. La direzione scelta è installare DaisyUI come *capability* sopra Tailwind 3.4 — zero JS runtime, zero impatto su Leptos/SSR/hydration — senza retrofit di massa.

    **Baseline shippata 2026-05-02:**
    - DaisyUI 4 come devDep (`package.json`), plugin in `tailwind.config.js` con tema custom `brain` che mappa il palette esistente (primary teal `#2dd4bf`, accent violet, neutral slate). `base: false` per evitare override di global styles esistenti; `styled: true`, `utils: true`.
    - `data-theme="brain"` attaccato a `<html>` in `crates/brain-app/src/app.rs` — l'unico touchpoint runtime.
    - Polish chirurgico (3 swap, solo veri "button idiom"): submit primario in `editor.rs`, `+ New` / `Close Editor` in `page.rs`, `Refresh` in `page.rs`. Banner/pill/toggle/disclosure idiom **non toccati** — sono pattern visivi distinti che DaisyUI default appiattirebbe.

    **Esplicitamente fuori da 3.7F:** retrofit massivo del palette (i 136 utility color usage restano), porting di banner/alert idiom a `alert alert-*` (perdono il look ghost-fill translucido attuale), introduzione di Headless UI Rust ports.

    **Cosa fare in futuro (gated, non pianificato):**
    - **Adozione incrementale, opt-in per nuova feature.** Ogni nuova superficie costruita in Fase 3.x/4.x dovrebbe usare componenti DaisyUI (`btn`, `modal`, `dropdown`, `toggle`, `tabs`) come default. Niente sweep dedicati di refactor: il vecchio rimane finché non viene toccato per altri motivi.
    - **Modal/Dropdown sweep.** Quando arriva la prima feature che richiede un dialog non-banale (es. confirm di rename strutturale in 4.0 Admin Node Control, o conflict resolution UI in 4.4), usare `<dialog class="modal">` di DaisyUI invece di reinventare il pattern. Trigger: prima feature che genuinamente serve un dialog modale.
    - **Web Components bridge → 4.x, gated.** Discussione conclusa 2026-05-02: porting di un bridge JS/TS via Custom Elements ha senso **solo** se l'editor Markdown hits a real Leptos ceiling (rich-text/WYSIWYG, embedding di CodeMirror/ProseMirror/TipTap, o un visualizzatore graph WebGL per >1k nodi). Decision gate: deve emergere un need utente concreto in Fase 3 (3.4 visual editor / 3.7 dogfooding feedback) o 4.x. Niente spike speculativi. Costo della preparazione: piccolo — `js-sys` e `wasm-bindgen` sono già nel `Cargo.toml` di `brain-app`, quindi quando arriva il momento il runtime è pronto, manca solo il build step JS.
    - **D3/Cytoscape interop per `graph_canvas.rs` → backlog senza data.** Oggi `graph_canvas.rs` è 878 LoC di SVG hand-rolled con wheel/touch/pan-zoom: funziona, ma se serve force-directed layout o WebGL rendering sopra ~1k nodi, hand-off a libreria JS è preferibile a port Rust. Trigger: complaint utente concreto su perf/feature, non "perché si può".
    - **Dark/light theme switch.** Non in roadmap: oggi `color-scheme: dark` è hardcoded in `tailwind.css` e in `<meta>`. Se in futuro serve light mode (es. embed in dashboard chiari), il theme `brain` può affiancarne uno light — DaisyUI gestisce via `data-theme` switch. Trigger: bisogno reale, non simmetria estetica.

    **Caveat tecnico noto:** `base: false` significa che gli `<html>`/`<body>`/`<button>` di default Tailwind/browser restano. Se in futuro si abilita `base: true`, fare prima un audit visivo perché DaisyUI reset diversi default (focus rings, button padding, link colors) e i 136 utility color usage attuali potrebbero subire regressioni puntuali. Decisione: lasciare `base: false` finché non c'è una ragione esplicita per cambiare.

    **Success criterion:** ogni nuova superficie UI futura riusa `btn`/`modal`/`tabs`/`alert` di DaisyUI quando il pattern è applicabile, riducendo divergenza visiva senza richiedere sweep dedicati. La review verifica che PR di feature non reintroducano button idiom hand-rolled quando esiste l'equivalente DaisyUI.

- [x] **3.7G Sidebar Real-Estate & No-Clutter Posture** — _baseline DONE 2026-05-02_

    Razionale: la sidebar di `KnowledgePage` aveva accumulato cinque sezioni in un singolo scroll context (`BrainSwitcher` → `Views` → `Type` → `Tags` → `RepoStructureTree` → footer hint). Tre problemi concreti emersi guardando avanti a 4.x: (1) `all_tags` è unbounded — un repo con 30+ tag attivi creava un wall di pill che dominava la sidebar; (2) `RepoStructureTree` (la superficie di navigazione più usata insieme al graph) era sotto i tag, scrollata via dal default viewport; (3) la slot per il futuro **Temporal Graph View di 4.2 (git history visualization, stile VS Code source-control)** non aveva spazio riservato — appendere come sesta sezione avrebbe replicato il problema.

    **Baseline shippata 2026-05-02 (`crates/brain-app/src/knowledge/filter_panel.rs`):**
    - **Split-pane primitive.** L'`<aside>` si è spaccato in tre regioni con `flex-col h-full min-h-0`: header fisso (BrainSwitcher), filter pane scrollabile autonomamente (`flex-1 min-h-0 overflow-y-auto`), bottom slot riservato per la history view di 4.2 (oggi gated da `HISTORY_SLOT_ENABLED: bool = false`, occupa `h-1/3 min-h-[160px]` quando attivato). Quando flippato, il filter pane cede 1/3 dell'altezza al bottom slot senza ulteriori interventi sulla page layout.
    - **RepoStructure promosso sopra Tags.** "Where things live" (concreto, persistente) è ora più alto di "how things are labeled" (semantico, rumoroso). Ordine finale del filter pane: Views → Type → RepoStructureTree → Tags → footer hint.
    - **Tags taming.** Search input `<input type="search">` filtra incrementalmente; container scrollabile `max-h-48 overflow-y-auto`; tag attivi pinnati in cima alla lista renderizzata; conteggio totale mostrato nell'header della sezione; empty-state quando nessun tag esiste o nessuno matcha la query. Niente "top-N collapsed" extra: search + scroll già coprono il caso a basso costo cognitivo.
    - **Header del Knowledge layout (cambio collegato di 3.7F):** chip per-type a accent-dot rimosso (legenda vive solo nella sidebar), zero-count nascosto, top-5 + DaisyUI `dropdown dropdown-end` con `+N more` per i tipi rimanenti; sidebar pill mostra `[●] Label N` con count tabulare e `opacity-50` per type a count zero.

    **Esplicitamente fuori da 3.7G:** sidebar collapsibility/icon-rail in stile VS Code (è engineering reale: icone, tooltip, animazione, tutto da costruire — rivedere solo se utenti reclamano larghezza su small screen o se nasce una seconda tool sidebar), draggable splitter tra filter pane e history slot (rinviato a 4.2, quando saremo davvero a riempire lo slot e si potrà calibrare il rapporto), tag-frequency ranking (oggi ordine alfabetico — predicibile; ranking by count è cosa che si può aggiungere se l'ordine alfabetico cessa di essere utile in pratica).

    **Posture vincolante per le slice future:**
    - **No nuove sezioni nella sidebar senza un budget esplicito di altezza.** Ogni proposta che aggiunge una sezione a `FilterPanel` deve dichiarare: (a) sta sostituendo qualcosa di esistente, (b) sta vivendo nel filter pane scrollabile sopra, o (c) ha bisogno di un proprio split-pane. Niente "appendi in fondo al filter pane".
    - **Bottom slot è prenotato per 4.2.** Il `HISTORY_SLOT_ENABLED` flag non viene flippato finché non c'è un componente reale dietro. Nel mentre lo slot resta dimensionalmente reclamato dal filter pane.
    - **Real-estate review per ogni feature UI.** Prima di disegnare una nuova superficie nel layout knowledge (sidebar, header, detail panel), il PR deve spiegare dove vive nel layout esistente e cosa cede spazio. Il default è "no": se non c'è uno spot ovvio, la feature aspetta o trova un altro layout (es. modal, dedicated route, tab).
    - **Tags sotto stress.** Se in produzione si vede una repo con >100 tag visibili, il search box potrebbe non bastare — valutare una category/group taxonomy in `.brain-config.yml` o un secondary level di filtraggio (recently used, by node type). Trigger reale: feedback utente o screenshot di sidebar troppo densa.

    **Success criterion:** la sidebar resta navigabile a colpo d'occhio anche con repo da centinaia di nodi e decine di tag; il bottom slot di 4.2 si attiva flippando una const senza ridisegnare il layout; le PR di nuove superfici UI dichiarano esplicitamente l'impatto sul real-estate prima di mergiare.

### Fase 3 Closeout: dogfooding collaborativo

- [x] **Audit follow-up 2026-05-09 — server-fn auth gating regression guard** _(DONE 2026-05-09)_
    - Tutte le 24+ server fn ripetono manualmente `session::require_session_and_token().await.map_err(sfe)?` (alcune `require_authenticated`, alcune `require_target_admin_session`). L'audit del 2026-05-09 ha trovato `GetCurrentUser` senza gate (corretto in-line); `GetAppConfig` rimane intenzionalmente public (la landing page lo legge anonima per mostrare brand/target). Ogni nuova server fn re-inventa il gate: precisamente come la regressione si è infilata.
    - Direzioni alternative: (a) un attribute proc-macro `#[require_session]` che inietta il gate prima del corpo, (b) un extractor Axum tipato `AuthenticatedSession` che sostituisce la coppia `(session, token)`, (c) un test `cargo expand`-based che fallisce se una `#[server]` fn non chiama una funzione marker `__assert_gated`. Scegliere la più economica al momento del lavoro.
    - Success criterion: aggiungere una nuova server fn senza il gate fallisce in CI; l'audit del 2026-05-09 (lista `#[server]` ↔ guard) si automatizza invece di restare grep-driven.
- [x] **Audit follow-up 2026-05-09 — config-parse banner UX** _(DONE 2026-05-09)_
    - `knowledge/config_loader::load` ricade silenziosamente su `BrainConfig::default()` quando `.brain-config.yml` non parsea: l'admin che rompe l'YAML non vede feedback diretto, scopre il problema solo quando i tipi "scompaiono" dal grafo.
    - Estendere il banner orphan esistente in `orphan_banner.rs` con una variante `Config invalid` — testo dell'errore di parse + link al file su GitHub. Coerente col pattern dei banner attuali, niente stato globale nuovo.
    - Success criterion: rompere `.brain-config.yml` su un branch di test produce un banner con il diagnostic preciso entro il TTL di 30s del config cache.

---

## 🟤 Next Hardening Lane — closed items _(through 2026-05-23)_

- [x] **Security & Content Trust Baseline** _(pre-embed / pre-blob / pre-AI)_ — **chiusa 2026-05-20** (resta solo l'allowlist iframe, accoppiata all'embed work futuro)

    Razionale: Brain UI renderizza contenuto controllato dal repository e da provider esterni (Markdown, commenti issue, asset, in futuro iframe e AI-generated text). Finché l'app era un editor interno GitHub-first il rischio era gestibile; con dashboard embed, BYOB e assistant diventa un vero trust boundary. Questo lavoro va fatto prima di embedded analytics, BYOB/blob e AI, ma non è un prerequisito per dogfoodare il core già chiuso.

    - ~~Markdown rendering policy esplicita~~ **Done 2026-05-20:** policy = escape del raw HTML a monte nel parser (`Event::Html`/`InlineHtml` → testo, WASM-safe, coerente tra render SSR e preview editor) **+** sanitizzazione dell'HTML generato con `ammonia` (allowlist di schemi URL, `#[cfg(feature = "ssr")]` perché html5ever non compila in WASM). Copre documenti Brain, preview editor e commenti GitHub nel detail panel. Test XSS in `markdown.rs` (script/iframe/onerror/`javascript:`).
    - ~~CSP baseline globale~~ **Done 2026-05-20:** `Content-Security-Policy` statico emesso da `security_headers`: `default-src 'self'`, `script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'` (richiesto da Leptos hydration; nonce = hardening futuro), `style-src 'self' 'unsafe-inline'`, `img-src 'self' data: blob: https:`, `connect-src 'self' ws: wss:`, `frame-src 'none'` (diventa dinamico con l'embed allowlist), `object-src/base-uri/frame-ancestors 'none'`. Backstop `DefaultBodyLimit` 8 MiB sul router.
    - Upload safety: validare content-type e magic bytes lato server, non solo estensione; limitare SVG upload finché non esiste un sanitizer/policy dedicata. **Micro-fix 2026-05-01:** `UploadAsset` non accetta più nuovi `.svg`; gli SVG storici restano serviti dal proxy come asset legacy. **Done 2026-05-20:** `UploadAsset` ora valida i magic bytes (`infer`) contro l'estensione dichiarata — un payload HTML/script travestito da `.png` viene rifiutato.
    - GitHub Contents API URL hardening. **Micro-fix 2026-05-20:** `GithubClient::contents_url` percent-encoda i segmenti del path repo preservando `/`, tutti i read Contents API passano `ref` via `reqwest::RequestBuilder::query`, e l'asset proxy decodifica il path browser prima di ricodificarlo verso GitHub. Test wiremock copre path con spazi / `#` / `?` e branch con slash.
    - Protected route contract. **Micro-fix 2026-05-20:** il matcher auth e' stato estratto in `is_protected_path` con test table-driven per rotte legacy, multi-target, canonical branch, asset proxy e SSE. Il test ha chiuso anche un gap reale: `/{org}/{repo}/assets/...` profondo non era classificato come protetto dal vecchio pattern a lunghezza esatta.
    - ~~CSRF per mutazioni server fn~~ **Done 2026-05-20:** middleware `csrf_protect` rifiuta le mutazioni `POST/PUT/PATCH/DELETE` su `/api/*` il cui host `Origin` non combacia con `Host` (`Sec-Fetch-Site` onorato quando presente). Webhook (HMAC) e callback OAuth (GET) esenti. `is_same_origin` con test table-driven.
    - Session/token posture: decidere esplicitamente tra OAuth token utente, GitHub App installation token e token scoped per MCP. Tracciare TTL, revoca, cifratura at-rest della session store e rotazione dei secret. **Decisione 2026-05-20:** token utente OAuth = sistema-di-record per le mutazioni dell'utente; installation token (App-first, PAT fallback) per la sync inbound dei webhook; MCP scoped rimandato. Cifratura at-rest → vedi sotto. TTL/revoca/rotazione: il token OAuth vive nella session cifrata; rotazione = re-login; revoca = `RevokeSession` + scadenza cookie.
    - **Audit follow-up 2026-05-09:**
      - ~~Webhook task supervision~~ **Done 2026-05-20:** `handle_push` / `handle_item_event` ora passano per `spawn_supervised`, che fa `await` sul `JoinHandle` e logga esplicitamente panic/cancellazioni con il nome del task (`tracing::error!`). Resta fire-and-forget lato risposta HTTP (202 già inviato); l'obiettivo è la visibilità operativa. Si lega al "Background job/outbox".
      - ~~Schema-evolution helper tipato~~ **Done 2026-05-20:** `add_column_if_missing` non accetta più `&str`: prende un `enum MigratableTable` + `struct ColumnSpec` con campi `&'static str`, così nel `format!` del DDL possono finire solo identificatori da un'allowlist chiusa. I 4 call site usano le varianti tipate. Versionamento migrazioni resta in "Projection Schema v2".
      - ~~Input size caps server-side~~ **Done 2026-05-20:** modulo `api::limits` con `MAX_MARKDOWN_BYTES` (1 MiB) / `MAX_FRONTMATTER_BYTES` (64 KiB) / `MAX_PATH_LEN` (1024) / `MAX_VIEWS_BYTES` / `MAX_FIELD_LEN`, applicato a `SaveBrainFile`, `SaveViews`, le mutazioni work-item, e via `validate_markdown_path` (path len) a save/delete/rename. Coerente con `MAX_ASSET_BYTES`.
      - ~~Rate-limit baseline~~ **Done 2026-05-20:** `tower_governor` per-IP (`SmartIpKeyExtractor`, legge X-Forwarded-For dietro il proxy Railway) come layer outermost; default 2 req/s burst 60, override via `RATE_LIMIT_PER_SECOND` / `RATE_LIMIT_BURST`. Copre `/auth/callback`, `/webhook/github` e le server fn. `into_make_service_with_connect_info` per il fallback peer-IP in dev.
      - ~~Cifratura at-rest dei token OAuth~~ **Done 2026-05-20:** scelta (a) — `SessionManagerLayer::with_private` con chiave da `SESSION_ENCRYPTION_KEY` (base64, ≥64 byte). In prod (`SESSION_COOKIE_SECURE`) la chiave è obbligatoria (fail-fast come `WEBHOOK_SECRET`); in dev una chiave effimera è generata con warn. **Impatto deploy:** nuova env var su Railway, genera con `openssl rand -base64 64 | tr -d '\n'` (il `tr` evita il newline di wrapping che renderebbe il valore non-base64; il parser comunque ignora i whitespace interni); al primo deploy le sessioni esistenti si invalidano → un re-login.
    - Success criterion: esiste una test matrix di sicurezza che prova XSS da Markdown/commenti, upload asset ostili, server fn cross-site e iframe non allowlistati; nessuna nuova superficie esterna può bypassare questa policy. **Stato 2026-05-20:** coperti XSS Markdown/commenti, magic-bytes upload, same-origin CSRF (`is_same_origin`). Iframe non-allowlistati restano TODO finché non esiste l'embed allowlist (`frame-src 'none'` oggi blocca tutto).

- [x] **Failure-Mode Matrix & Operational Readiness** _(chiusa 2026-05-21 — α/β/γ/δ + doc table tutte shippate)_

    Razionale: il roadmap copre bene le capability, ma le feature collaborative diventano affidabili solo se i failure mode sono disegnati come percorsi di prodotto. Ogni nuova slice dovrebbe dichiarare cosa succede quando GitHub non risponde, il branch si muove, il permesso cambia, il provider sync fallisce, la projection è stale o il config è invalido.

    **Slicing (2026-05-21):** item diviso in commit indipendenti, ordinati α → β → γ → δ → doc.
    - **α — Health endpoints** _(DONE 2026-05-21)_: `/healthz` (liveness, 200 incondizionato) e `/readyz` (readiness, 200/503 con body per-check). Modulo `server::health`; check live = `SELECT 1` sul pool SQLite condiviso (copre anche session store + audit) + `projection_pool` initialized; `session_store_migrated` riportato come `true` boot-garantito (migrazione fail-fast). Rotte non autenticate (`is_protected_path` le esclude, CSRF gata solo `/api/*`). Test: unit + router-level via `oneshot`, e asserzioni public-route in `is_protected_path`.
    - **β — Errori tipati ai bordi** _(DONE 2026-05-21)_: `ApiError` enum (`api::error`) che impl `FromServerFnError` con `JsonEncoding`; le server fn ritornano `Result<T, ApiError>` **diretto** (non `ServerFnError<E>` — deprecato in 0.8, shape forward-compatible con 0.9). Bridge `From<BrainError>` classifica i messaggi GitHub (403→`PermissionDenied`, 404→`NotFound`, 409/not-fast-forward→`Conflict`, 429/rate→`RateLimited`); `sfe()` ridefinito come alias di `ApiError::from` così i ~120 call site non cambiano. `installation_token::mint` ritorna `TokenMintError` (no_app/mint_failed/no_creds). UI: `ApiError::actionable_message()` + editor save e work-item mutation matchano sul tipo (stale→reload, no-write→PR, rate→retry). Compila ssr+wasm; 126 test verdi.
    - **γ — Outbox/pending-sync** _(DONE 2026-05-21)_: tabella `pending_provider_sync(id, target_id, brain_id, kind, attempts, created_at, last_attempt_at, last_error)` con `UNIQUE(target_id, brain_id, kind)` (un fail ripetuto bumpa attempts, non duplica). Enqueue al punto best-effort di `apply_work_item_mutation_inner` (editorial save già committata, nessun rollback). Retry job supervisionato (`pending_sync_job::spawn`, `tokio::interval` 60s override `PENDING_SYNC_INTERVAL_SECS`, batch 25, stop a 20 attempts) che si autentica come **GitHub App** (no user session in background → commit App-attribuito) e riconcilia il provider allo stato Brain corrente (idempotente). Server fn admin-gated `ListPendingSync` + sezione read-only "Pending provider sync" in admin. Test outbox round-trip; ssr+wasm verdi, 127 test.
    - **δ — SSE per-target** _(DONE 2026-05-21)_: `EventBus` ora è `OnceLock<Mutex<HashMap<TargetKey, broadcast::Sender>>>` (idioma del codebase à la cache di `brain_storage`, non `dashmap` — send/subscribe sono a bassa frequenza). `send` instrada per `TargetKey` derivato dall'evento; canale per-target creato lazy a 64 slot, così un target rumoroso non evince gli eventi di un altro. L'handler `/sse/events?target=org/repo/branch` sottoscrive solo quel target (fallback al target env-default senza param); il client (`live_sync`) appende il target attivo. Test di isolamento per-target; ssr+wasm verdi, 130 test.
    - **doc** — tabella failure-mode (sotto), scritta per ultima così riflette il comportamento shippato.

    - **Failure-mode matrix** _(DONE 2026-05-21 — riflette il comportamento shippato dopo α–δ)._ Le superfici già costruite:

      | Feature | Failure mode | UX | Audit / log | Retry / recovery |
      |---|---|---|---|---|
      | **Save Brain file** (`SaveBrainFile`) | 403 / branch protetto | fallback automatico a PR (`should_fallback_to_pr`); `WriteResult::PullRequest` mostra link PR | `update` su success; `api_error` su errore | utente segue la PR; nessun retry server-side |
      | | 409 / stale sha | `ApiError::Conflict` → editor mostra "reload and retry" (`actionable_message`) | `api_error` | utente ricarica e riapplica |
      | | 429 / rate limit | `ApiError::RateLimited` → "wait and retry" | `api_error` | utente ritenta a breve |
      | **Rename** (`RenameBrainFile`) | 403 / protetto | fallback a PR come save | `rename` / `api_error` | via PR |
      | | path collision / non trovato | `ApiError::Conflict` / `NotFound` typed | `api_error` | reload |
      | **Work item sync** (transition/assign/bind) | push provider best-effort fallisce | save editoriale **resta** (no rollback); riga in `pending_provider_sync`; admin la mostra | `work_item_provider_sync_error` | retry job supervisionato (App-auth) riconcilia; stop a 20 tentativi |
      | **Webhook rebuild** (`/webhook/github`) | nessun token App/PAT | rebuild saltato; banner SSE `SyncFailed` ("showing last snapshot") | warn log | refresh manuale o prossimo push |
      | | rebuild projection fallisce | banner SSE `SyncFailed` con reason | warn log | refresh manuale (`RefreshBrainGraph`) |
      | | task panic | log esplicito via `spawn_supervised` (202 già inviato) | `tracing::error` | prossimo webhook |
      | **Config editor** (`SaveViews` / `.brain-config.yml`) | YAML invalido al load | banner "Config invalid" (`orphan_banner`) con diagnostic + link al file | — | fix YAML; cache TTL 30s |
      | | save views 403 / conflict | typed `ApiError` in admin | `update_views` / `api_error` | reload / PR |
      | **Asset proxy** (`/assets/...`) | upstream 404 | `404 Not Found` | — | — |
      | | upstream 401/403 | `403 Forbidden` (mai leak del token) | — | re-login se sessione scaduta |
      | | upstream irraggiungibile | `502 Bad Gateway` | — | retry |
      | **Health** (`/readyz`) | SQLite irraggiungibile | `503` + body per-check | — | orchestratore smette di routare |
      | **Live sync** (SSE) | connessione persa | banner "Stale Data" + reconnect con backoff | — | reconnect automatico per-target |

      Superfici **non ancora costruite** (failure mode da definire quando arrivano): **embed URL / iframe** (oggi `frame-src 'none'`), **blob URL / BYOB** (solo allowance CSP `blob:`), **FTS5 rebuild** (search non ancora implementata — vedi "Projection Schema v2").
    - ~~Health endpoint operativo (`/healthz`/`/readyz`)~~ **Done 2026-05-21 (α):** distingue SQLite raggiungibile (`SELECT 1`), projection pool pronto, session store migrato (boot-garantito). Il read del sync-state del target default è stato lasciato fuori da α (si appoggerà alla doc/admin slice) per tenere α minimale e senza risoluzione target_id.
    - Background job/outbox leggero per riconciliazioni costose o retryabili: provider sync, webhook replay, future blob cleanup. Non introdurre un job system pesante finché SQLite + tokio task supervisionato bastano.
    - **Audit follow-up 2026-05-09 (additions, not done):**
      - ~~Errori tipati ai bordi~~ **Done 2026-05-21 (β):** `TokenMintError` su `installation_token::mint` + `ApiError` enum (`FromServerFnError`/`JsonEncoding`) ritornato diretto dalle server fn; bridge `From<BrainError>` con classificazione GitHub status; UI matcha su variant via `actionable_message()`. Resta prerequisito soddisfatto per 4.4 conflict resolution.
      - ~~SSE broadcast per-target~~ **Done 2026-05-21 (δ):** `EventBus` instrada per `TargetKey` su canali per-target (`Mutex<HashMap<…, broadcast::Sender>>`); l'handler sottoscrive solo il target richiesto via `?target=`. Il filtro lato JS resta come backstop ma il server non consegna più gli eventi cross-target. (Implementato in anticipo rispetto alla soglia ~10 target/istanza.)
      - ~~Outbox/pending-sync surface per provider mutations~~ **Done 2026-05-21 (γ):** tabella `pending_provider_sync` + enqueue al fallimento del push best-effort + retry job supervisionato (App-auth) + sezione read-only in admin. Gli operatori vedono le mutazioni non ancora propagate invece di dedurle dall'audit log.
    - Success criterion ✅: quando una dipendenza esterna degrada, Brain UI mostra uno stato azionabile invece di un errore generico (typed `ApiError` + `actionable_message`, banner SyncFailed/Stale, fallback PR); gli operatori hanno log/audit + `/readyz` + admin pending-sync sufficienti per capire se serve refresh, retry, re-login o fix di config.

- [x] **Projection Schema v2 & SQLite Operations** _(landed 2026-05-23)_

    Razionale: le prossime feature usano SQLite non più solo come read model minimale. FTS5 richiede corpo/titolo/tag indicizzabili; Activity Stream e co-authorship richiedono commit metadata; Watch/Follow e advisory locks introducono dati user-scoped; Temporal Graph userà projection effimere. Prima di appoggiare tutto questo sulla tabella corrente, serve una piccola maturazione del layer dati.

    - **SQLite contention posture (reassessed 2026-05-21):** WAL è già abilitato a boot; `busy_timeout = 5s` viene ora impostato sulle connessioni del pool per assorbire burst brevi di webhook/rebuild/outbox writes invece di fallire subito con `database is locked`. Non introdurre subito database-per-target: oggi sessioni, audit, target registry e projection convivono nello stesso file per semplicità operativa. Rivalutare lo split `data/projections/{target}.db` solo dopo evidenza reale di lock contention multi-target o prima di un hosting pubblico multi-tenant.
    - ~~Versionare le migrazioni SQLite~~ **Done 2026-05-23:** sqlx `migrate!` macro + `crates/brain-app/migrations/{0001_baseline,0002_projection_v2}.sql`; baseline idempotente (claim-on-legacy verificato su copia di prod `sessions.db`); `audit_events` ripiegato dentro `0001`. CI test `migrate_claims_baseline_on_legacy_db` blocca regressioni.
    - ~~Estendere `files`/`nodes`~~ **Done 2026-05-23 (parziale):** aggiunti `files.body_text`, `files.frontmatter_json`, `nodes.body_text`, `nodes.frontmatter_json` (popolati via `brain_domain::split_frontmatter` in rebuild) + tabella `node_authors` popolata da `author:`/`authors:` frontmatter. **Deferred:** `commit_author`/`commit_message`/`last_commit_at` — `brain_storage::fetch_raw_files` non espone metadata per-file dei commit, servirebbe batching/incremental webhook prima di N+1 sulla rate limit.
    - ~~Definire retention/cleanup~~ **Done 2026-05-23:** `server/retention.rs` task supervisionato daily — audit log con `AUDIT_RETENTION_DAYS` (default 90), expired sessions (fallback per il sweeper non wired di `tower-sessions-sqlx-store`), warn-only su `pending_provider_sync` con `attempts >= MAX_ATTEMPTS` (threshold condiviso con il retry job). Editing locks/watch notifications restano out-of-scope finché le feature non esistono.
    - ~~Aggiungere una vista admin/status~~ **Done 2026-05-23:** server fn `get_projection_status()` + sezione admin con schema version (`MAX(version)` da `_sqlx_migrations`), per-target status/file/node/edge/work_item counts, `last_rebuild_duration_ms` (instrumentato in `rebuild` post-write). Webhook lag e rate-limit snapshot resi come placeholder `—` — slot pronto per la follow-up slice che plumba i due signal.
    - Success criterion ✅: FTS5, Activity Stream, Watch/Follow e Temporal Graph hanno una base dati coerente e migrabile; un deploy nuovo o esistente può aggiornarsi senza rebuild manuale cieco. Smoke test legacy (`LEGACY_DB_SMOKE=...`) e `migrate_claims_baseline_on_legacy_db` documentano la migrabilità.

- [x] **Presentation UI Polish Pass** _(landed 2026-05-23, post-Projection Schema v2, pre-open-source prep)_

    Razionale: nelle ultime slice il prodotto ha accumulato sostanza backend (TargetRef, provider outbox, health/readiness, API errors, SSE per-target). Prima della presentazione/investor talk di giugno 2026 conviene trasformare quella maturità in percezione immediata: una UI più calma, più leggibile e più dimostrabile sul Pokémon mock. Questa pass non apre nuove capability platform; rende presentabili le superfici già shippate e chiude i papercut che rovinerebbero una demo.

    - ~~**Knowledge first impression**~~ **Done 2026-05-23:** header ricalibrato su identità target/count/Status, Brain Switcher più leggibile, Refresh copy calmo, loading/error state non diagnostici.
    - ~~**Graph/sidebar/detail rhythm**~~ **Done 2026-05-23:** sidebar con scope summary, struttura più scansionabile, grafo con empty state filtrato, legenda/zoom più controllati, close detail pulisce anche la query selection.
    - ~~**Work item clarity**~~ **Done 2026-05-23:** card work item mostra titolo, stato, source-of-record, Brain ID, provider binding, sync posture e messaggi distinti per direct write vs PR fallback.
    - ~~**Admin/status polish**~~ **Done 2026-05-23:** projection status ora apre con readiness tiles, totals e operation chips; tabelle admin rese più operative e scrollabili.
    - ~~**Pokémon mock demo fixture**~~ **Done 2026-05-23:** percorso demo ripetibile documentato in un runbook dedicato (successivamente rimosso una volta esaurita la rehearsal); il sandbox resta il dataset di dogfooding/presentazione, con verifica fixture da fare nel repo target prima della rehearsal.
    - **Follow-up leggero annotato 2026-05-23:** `BrainSwitcher` oggi chiama `ListAccessibleTargets` ogni volta che viene riaperto. Accettabile per la demo, ma se la lista repo cresce o il provider rallenta conviene passare a cache client-side con refresh esplicito/revalidate.
    - **Success criterion ✅:** la demo 5-7 minuti ha un percorso esplicito: aprire il Pokémon Brain, navigare saved view/grafo/detail, mostrare un work item bindato, spiegare sync/projection status e proporre una modifica via flow collaborativo senza copy/layout temporanei.

---

## Closed caveats _(archived)_

2. **DONE 2026-04-26** **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

4. ~~`prose-sm` typography sizing is a guess~~ — **DONE 2026-04-26 (no-op)**. Both render sites (`detail_panel.rs`, `editor.rs`) already use `prose prose-invert max-w-prose` (default size, equivalent to `prose-base`), not `prose-sm`. The original caveat was stale. Future tuning of `tailwind.config.js` `typography.invert` palette remains available if real content warrants it.

5. ~~Update path regenerates frontmatter from templates~~ — **DONE 2026-04-22**. `merge_frontmatter` fa overlay dei campi del form sulla mappa preservata invece di rigenerare da template. Tests in `brain-app::api::merge_frontmatter_tests`.

6. ~~No auto-refresh after out-of-band commits~~ — **DONE 2026-04-24**. `RefreshBrainGraph` server fn + `RefreshButton` component in the knowledge header now trigger a full rebuild of the local SQLite projection after invalidating graph, template, and config caches. The button closes the "commit esterno → reload manuale" gap and doubles as manual reindex/drift recovery until webhook/SSE sync lands.

7. ~~Rename issues N+2 Contents API commits~~ — **DONE 2026-04-25**. `rename_brain_file` now produces a single commit via `GithubStorage::atomic_rename` (Git Data API: blobs → tree → commit → ref update with `force=false`). Fast-forward conflicts (422) are retried with capped exponential backoff; uploaded blobs are reused across retries since they're content-addressed.

8. ~~Graph cache is process-global, not user- or target-scoped~~ — **DONE 2026-04-24**. All caches in `brain-storage` (graph, template) and `config_loader` are now keyed by `TargetKey({org}/{repo}/{branch})` via `OnceLock<Mutex<HashMap<TargetKey, _>>>`. `GithubHttp` is target-agnostic (plain `Arc<reqwest::Client>`); each `GithubStorage` / call-site supplies its own `GithubClient` built from an explicit `TargetConfig`. Safe for Phase 3 multi-target without re-architecting.

9. ~~`register_explicit` boilerplate is LTO-coupled~~ — **DONE 2026-04-24**. `SERVER_FNS: &[&str]` const + `include_str!`-based test in `api.rs` catches any `#[server]` fn not listed in the const before it reaches CI.

10. ~~Sync visibility is still page-local~~ — **DONE 2026-04-26**. `LiveSync` (EventSource subscription) e `SyncStatusBanner` ora vivono in `App`, sopra `<Routes>`, leggendo `graph_version`/`sync_status` esposti via `provide_context(GraphVersion)` / `provide_context(SyncStatusSignal)`. Admin su `/admin` o future route work-item vedono lo stesso banner `Stale Data` di `/knowledge`. `RefreshButton` resta page-local in Knowledge ma muta gli stessi signal globali.

12. ~~Filters are not URL-persisted~~ — **DONE 2026-04-26**. `active_tags` and `active_types` round-trip through `?tags=` and `?types=` query params alongside the existing `?path=`. Refresh and link-share both restore the filtered view. Navigation uses `replace: true` so toggling filters doesn't pollute history. Tags are normalized lowercase in both directions; types preserve case (they map to `BrainConfig.node_types[].name`).

13. ~~No keyboard dismissal~~ — **DONE 2026-04-26**. Esc cascade in `KnowledgeView` (hydrate-only `keydown` listener on `window`): closes the editor first if open, otherwise clears the selected node. Skipped while focus is in `input`/`textarea`/`select`/`contenteditable` so Esc doesn't fight IME or form-local handlers. Listener is cleaned up via `on_cleanup` on route change.

14. ~~`Stale Data` banner can flash before login~~ — **DONE 2026-05-02**. `LiveSync` now opens `/sse/events` only on protected workspace routes (`/knowledge`, `/admin`, and multi-tenant target routes). Public landing visits no longer trigger the auth-gated SSE endpoint, so anonymous deploy visitors do not see a stale-data banner caused by the expected `401`. The landing page also now states that OAuth works but is still a raw access surface that needs product evaluation.

16. ~~Asset proxy 404 silenzioso su repo privati~~ — **DONE 2026-04-29**. `serve_asset` in [crates/brain-app/src/server/assets.rs](../crates/brain-app/src/server/assets.rs) interrogava `raw.githubusercontent.com` con `Authorization: Bearer <token>`, ma quell'host **non accetta** bearer auth su repo privati (richiede `?token=` query param legacy o Contents API). Ritornava 404 silenzioso. Sintomo concreto: il primo asset prodotto dal Brain Clipper (`assets/2026/04/schermata-del-2026-04-29-…png`) era committato correttamente su `main` (Contents API: 200, sha matching, size 515850) ma non si vedeva in Brain UI. **Fix**: il proxy ora chiama la Contents API con `Accept: application/vnd.github.raw`, che restituisce i bytes raw direttamente con la stessa autenticazione bearer di tutte le altre chiamate. Verificato end-to-end con probe curl: 200, content-type corretto, file scaricato byte-identical al locale. Aggiungere test wiremock-based che fallisca se qualcuno reintroduce `gh.raw_base()` nel proxy.

17. ~~Node hover flicker on graph canvas~~ — **DONE 2026-04-30**. Diagnosi confermata lato geometria SVG: l'hit-area del `<g>` era l'unione dei figli pitturati, quindi il `<circle>` visibile che transiva `r = base_r + bump` cambiava anche il target effettivo del puntatore. Fix in [graph_canvas.rs](../crates/brain-app/src/knowledge/graph_canvas.rs): il cerchio visibile ora ha `pointer-events="none"` e resta libero di animare; un secondo `<circle>` trasparente, stabile, con `r = base_r + selected_bump + buffer` riceve hover/click via bubbling sul gruppo. Aggiunto test `node_hit_radius_covers_all_visual_states`.

20. ~~Target identity still has ambient edges~~ — **DONE 2026-05-01**. Chiuso con **3.7B Canonical TargetRef & Trust Boundary Preflight**: branch nel path canonico, `TargetRef { org, repo, branch }` esplicito nelle server fn target-scoped principali, branch/default branch persistiti nel registry, webhook risolti dal payload reale e SSE filtrata per target.

22. ~~Baseline browser security headers missing~~ — **DONE 2026-05-09**. `cache_control` middleware impostava solo Cache-Control. Aggiunto un secondo `security_headers` middleware in `main.rs` che setta `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin` su ogni risposta, più `Strict-Transport-Security: max-age=31536000; includeSubDomains` quando `SESSION_COOKIE_SECURE=1`. CSP resta in roadmap come item separato perché richiede l'allowlist degli iframe per essere calcolato server-side senza spezzare la SSR.

23. ~~`GetCurrentUser` accessible without authentication~~ — **DONE 2026-05-09**. La server fn era gated solo da `session::session()` (cookie present, non necessariamente authenticated), permettendo a chi conosce un session id valido ma scaduto/non-org di leggere il login dell'ultimo utente del cookie. Ora chiama `require_authenticated()`. `GetAppConfig` resta intenzionalmente public: la landing page lo legge per anonimi per mostrare brand/target ("Sign in to open the {org} workspace") — disclosure intenzionale del contenuto già destinato al landing pubblico.

24. ~~`panic!` on missing env / failed bind in main.rs and auth.rs~~ — **DONE 2026-05-09**. `required_env_with_legacy` e i paniche per bind TCP / mkdir session DB / WEBHOOK_SECRET sono stati sostituiti con `tracing::error!(...)` + `std::process::exit(1)`. Stessa semantica fail-fast, ma container orchestrators e log aggregator vedono una linea strutturata con il nome dell'env mancante invece di "exit 101 from panic at main.rs:8".

25. ~~No supply-chain advisory check in CI~~ — **DONE 2026-05-09**. Aggiunto job `audit` in `.github/workflows/ci.yml` (`rustsec/audit-check@v2`) e `.cargo/audit.toml` con due ignore documentati: `RUSTSEC-2023-0071` (rsa Marvin Attack — transitive solo nel proc-macro graph di sqlx-macros, non nel runtime binary) e `RUSTSEC-2024-0436` (paste unmaintained — transitive via Leptos). Bumped `rustls-webpki` 0.103.12 → 0.103.13 per chiudere `RUSTSEC-2026-0104`. Re-evaluare l'audit.toml ogni volta che Leptos o sqlx ricevono un major update.

26. ~~Gemini review follow-up: URL/ref hardening + route auth contract~~ — **DONE 2026-05-20**. Decisione: non anticipare `moka`/`dashmap`, `anyhow` in boot, CSP completa o sub-router auth strutturali. Slice chiusa: Contents API senza concatenazione manuale di `?ref=`, path repo percent-encoded per segmenti, asset proxy robusto sui path percent-encoded, `protect_knowledge` appoggiato a un matcher testabile. Residual roadmap: CSRF/rate-limit/CSP restano nella Next Hardening Lane; cache concorrenti solo se dogfooding mostra contention o molti target/utenti.
