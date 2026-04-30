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
      - L'upload asset resta direct-write oriented perché un asset proposto via PR non è immediatamente referenziabile dal markdown live; va ripensato insieme al fallback PR UX per immagini/draft.
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

- [ ] **3.7 Repo Structure Transparency**

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

- [ ] **3.8 Embedded Analytics Views**

    Razionale: Brain UI è un control plane su una knowledge base editoriale, ma una parte crescente del valore aziendale vive in dati operativi pesanti (Postgres/Supabase, ads Instagram via Airbyte, metriche prodotto) che non ha senso ingerire dentro la projection SQLite né renderizzare con charting library lato WASM. La via naturale è far convivere dashboard esterni (Metabase, Superset, PowerBI, Grafana) come *view* di prima classe accanto ai grafi/nodi, embeddati via iframe, configurati dallo stesso `.brain-config.yml` e gated dalla stessa RBAC. Brain UI resta agnostico rispetto al data source: non parla mai col DB, non sa cosa c'è dentro il dashboard, sa solo che esiste, chi può vederlo, e dove punta.

    Obiettivo dichiarativo: usare il pattern `ViewSpec` introdotto da 3.4-α come superficie unificata per tutte le viste Brain (saved filter views *e* embedded analytics), evitando un secondo schema parallelo. Zero charting library aggiunte al bundle WASM. L'iframe è opaco per design.

    **Realignment ex-ante:** la slice viene spezzata in α (whitelist statico, no JWT) → β (JWT signing per provider che lo supportano), coerente col precedente di 3.2/3.4. α deve essere end-to-end shippable da solo — niente "dummy server fn che ritorna URL hardcoded": un dashboard pubblico/whitelistato è già un caso reale.

    Vincoli condivisi (validi per α/β):
    - Lo schema `views` di 3.4-α viene esteso con un campo `type: filter | iframe` (default `filter` per backward compat); le `iframe` view portano `url`, opzionalmente `provider` (per la β), e `requires_role` riusa la capability matrix di 3.3.
    - Allowlist domini embeddabili dichiarata in `.brain-config.yml` (`embed_allowed_origins: [metabase.example.com, ...]`). Validata server-side in fase di config parse e *ri-validata* dalla server fn che restituisce l'URL — il frontend non può aggirarla.
    - CSP `frame-src` calcolato server-side dall'allowlist (non hardcoded), così aggiungere un dominio è un edit di config, non un deploy. Header `sandbox="allow-scripts allow-same-origin"` di default sull'iframe; override esplicito nel `ViewSpec` se il provider lo richiede.
    - Zero charting/visualization library aggiunte al bundle WASM. Se uno use case sembra richiederlo, è il segnale che dovrebbe essere un dashboard esterno embeddato, non nativo.
    - Editor visuale: il pattern `iframe` view va supportato esplicitamente in 3.4-β (o γ se si preferisce raggrupparlo coi long-tail), non lasciato a raw YAML, perché l'allowlist e il `requires_role` sono campi che gli admin devono poter modificare in UI.

    - **3.8-α Whitelist-based iframe views** _(scope minimo per validare il pattern)_
        - Estensione schema: `ViewSpec` accetta `type: iframe` con `url`, `requires_role`. Allowlist `embed_allowed_origins` a livello root del config. Validazione `BrainConfig::parse`: ogni `iframe` view deve avere `url` il cui host è in allowlist; slug unici come per 3.4-α.
        - Server fn `GetEmbedUrl(target, slug) → Result<EmbedUrlResponse, ServerFnError>`: legge config cached, verifica esistenza view + match slug, verifica `requires_role` contro la capability matrix dell'utente, ri-valida il dominio contro allowlist (defense in depth), ritorna URL + eventuali sandbox flags. Audit log `embed_url_issued { slug, target }`.
        - Componente `<EmbeddedAnalytics/>` in `brain-app/frontend`: `create_resource` su `GetEmbedUrl`, `<Suspense>` con skeleton, render `<iframe>` full-content-area al success. Errore esplicito (non blank) su `Blocked by permissions` / `Domain not allowlisted` / `View not found`.
        - Router/sidebar: la sidebar dinamica di 3.4-α già renderizza le view; estendere il render per distinguere chip filter (set query params) da chip iframe (navigate a `/{org}/{repo}/views/{slug}`). Route nuova che monta `<EmbeddedAnalytics/>` con lo slug come param.
        - Esplicitamente fuori da α: JWT signing, secret management, provider-specific (Metabase signed embeds, Superset guest tokens, ecc.), audit del *contenuto* visto dall'utente, multi-tenancy a livello dashboard (un dashboard Metabase per org).
        - Success criterion: un admin aggiunge un dashboard Metabase pubblico (o un Grafana share link) all'allowlist e come `iframe` view, lo vede comparire nella sidebar con il giusto `requires_role`, l'utente target apre la view e il dashboard è renderizzato dentro Brain UI senza che il backend abbia mai parlato col data source.

    - **3.8-β Signed embeds (provider-aware)** _(post-feedback expansion)_
        - Estensione `ViewSpec` con `provider: metabase | superset | powerbi | grafana | none` e blocco `signing_config` per-view (resource id, embed type, scope params).
        - Secret per-target in env var (`BRAIN_EMBED_SECRET_<TARGET_ID>` o equivalente — schema esatto da disegnare quando arriviamo qui). Mai nel config file, mai nel bundle WASM.
        - `GetEmbedUrl` esteso: per provider che lo supportano, firma payload JWT/HMAC con TTL corto (≤ 10min), include user id e role per provider che fanno row-level security via embed params. URL firmato non cachato lato server (TTL applicato lato provider).
        - Audit esteso: `embed_url_issued` include `provider`, `expires_at`, `signed: true`. Sufficiente per accountability senza loggare il contenuto visto.
        - Forma esatta dei `signing_config` per-provider va disegnata alla luce di α in produzione e dei provider effettivamente richiesti dagli utenti, non pre-pianificata.

    Esplicitamente fuori da 3.8 (entrambi gli slice):
    - Charting nativo / visualizzazioni custom dentro Brain UI — anti-goal esplicito.
    - Embed bidirezionale (iframe che pubblica eventi al parent Brain UI via `postMessage`) — superficie di sicurezza e UX troppo grande per questo scope.
    - Auto-discovery dei dashboard disponibili sul provider (es. listing di tutti i dashboard Metabase visibili all'utente) — manuale via config.
    - Auth federation Brain → provider (SSO, token exchange, OAuth delegation) — vive in fase 5 insieme all'AI Assistant Proxy se mai necessario.
    - Pre-rendering server-side dei dashboard come immagini cachate — fuori scope, è un dominio diverso (snapshot/reporting, non control plane).

    Vincolo trasversale: se l'iframe pattern non basta per uno use case (es. l'utente vuole filtri Brain UI che si propagano nel dashboard), la risposta di default è "documenta il limite" non "estendi Brain UI". L'integrazione cross-frame è una superficie di rischio sproporzionata rispetto al valore di un control plane.

### Post-Fase 3: Dogfooding collaborativo

- [ ] **Pokemon Brain mock con contributor limitato** _(assegnato a `@JacoTube` nel repo Brain, 2026-04-28)_
    - Creare una mini knowledge base Pokemon come target/sandbox config-driven, senza riusare la tassonomia del Brain principale: `.brain-config.yml` custom, tipi come `pokemon`/`trainer`/`gym`/`route`/`battle-report`/`quest`, template dedicati e saved view proprie.
    - Esercitare le feature shippate in Fase 3: routing target-aware, config loader YAML, node types custom, work item mutations, binding a GitHub Issue, saved view/config flow quando permesso, direct-write vs branch+PR fallback, webhook/SSE reconciliation e graph canvas polish.
    - Usare i commenti della GitHub Issue bindata come thread QA/collaborazione e validare la nuova sezione `Comments` del detail panel; eventuali gap rimasti vanno estratti come follow-up (`issue_comment` SSE/cache timeline).
    - Vincolo operativo: `@JacoTube` lavora da contributor non-admin; `main` resta protetto e ogni modifica finale passa da PR review di Andrea/Matteo.
    - Success criterion: PR aperta dal contributor con note QA su cosa ha funzionato, cosa e' stato confuso e quali follow-up vanno estratti prima della Fase 4.
- [ ] **Task Inbox e segnali operativi sui nodi** _(follow-up UX emerso dal dogfooding, 2026-04-29)_
    - Aggiungere una sezione o pannello “My Tasks” nella Knowledge UI che usa il read model già esistente (`list_work_items`) per mostrare le task assegnate all'utente corrente GitHub, escludendo di default `done` e `cancelled`.
    - MVP: lista compatta con titolo, stato, assignee, path/binding GitHub e click-to-focus sul nodo/file. Deve riusare `WorkItem.assignees`, `WorkItem.state`, `content_path` ed eventuale `external_binding`, senza introdurre un secondo modello task.
    - Evoluzione: tab/filtri `Mine`, `Open`, `Blocked`, `Unassigned`, `Done`, quick action per transizioni di stato e assignee update riusando `TransitionWorkItem` / `AssignWorkItem`.
    - Aggiungere segnali visivi direttamente sul graph canvas: piccoli badge/ring/banner sui nodi work item per stato interno (`blocked`, `in-progress`, `done`), assignee presenti e binding esterno. Il segnale deve essere leggibile ma non trasformare il grafo in una board kanban.
    - Estendere la legenda/toolbar del grafo per spiegare questi segnali e permettere toggle rapidi (“show blocked”, “show mine”, “dim done”), mantenendo URL/localStorage coerenti con i filtri esistenti.
    - Success criterion MVP: un utente vede immediatamente quali task gli sono assegnate e quali nodi sono bloccati/in progress senza aprire uno per uno il detail panel.
- [ ] **Repo structure: evoluzioni post-MVP** _(follow-up di 3.7, da pianificare dopo dogfooding)_
    - **Heatmap strutturale** — overlay nel tree view che mostra densità di link entranti/uscenti per cartella o età media dei file. Distingue cartelle "vive" da cartelle stagnanti senza richiedere drill-down manuale. Da disegnare solo dopo aver visto come gli utenti usano il tree base di 3.7.
    - **Vista "directory ↔ node type" come matrice** — diagnostica admin che mostra distribuzione: quali tipi di nodo finiscono in quali cartelle, quali cartelle hanno mix sospetti (es. `runbook` in `concepts/`). Utile per accorgersi di drift editoriale prima che diventi rumore.
    - **Sezione `Recently created/modified` separata** — derivata da git log via projection già esistente (commit metadata in `files`), mostra gli ultimi N nodi toccati come lista temporale. Complementare al grafo che è atemporale per definizione.
    - **Ristrutturazione bulk admin-only** — UI per move-by-pattern (es. "tutti i `runbook` di Q1 in `runbooks/2026/q1/`"), riusa `BranchTransaction` di 4.0 per atomicità + preview via `transaction.plan()`. Solo dopo che 4.0 è chiusa.
    - **Path collision detection in edit mode** — durante rename/move, evidenziare in tempo reale se il path target collide con un file esistente, è soggetto a case-insensitive collision (rilevante su filesystem case-folding), o supera limiti di profondità ragionevoli. Reuse parziale della logica `expect_absent` di `GitTransaction`.
    - Vincolo trasversale: ognuna di queste evoluzioni deve restare opt-in e non deve trasformare Brain UI in un file manager. Se l'utente vuole un file manager, ha GitHub.
- [ ] **Admin Node Control / manutenzione nodi** _(riduzione frizione editoriale, 2026-04-29)_
    - Aggiungere una superficie admin-only per controllo completo dei nodi, distinta dall'editor standard: l'utente normale continua a modificare contenuto e campi sicuri, mentre admin/maintainer possono correggere struttura, metadati e path senza aprire GitHub o un editor locale.
    - MVP: pannello “Node maintenance” nel detail/editor con raw frontmatter editor validato, preview/diff prima del salvataggio, edit dei campi non esposti dal form standard e recovery dei frontmatter malformati che oggi bloccano il save.
    - Operazioni strutturali: cambio tipo nodo/directory con move sicuro del file, rename path-aware, normalizzazione slug/titolo, aggiornamento backlink quando possibile e avviso esplicito quando l'impatto non e' risolvibile automaticamente.
    - Guardrail per azioni distruttive: delete/rename/move devono mostrare impatto su backlink, binding esterni e work item collegati; salvataggio via orchestratore Git già esistente con commit diretto o PR fallback, mai bypassando la source-of-truth del repo.
    - Estensione post-MVP: manutenzione bulk per retag, merge duplicati, correggere nodi orphan/unknown type, aggiungere campi richiesti mancanti e audit trail delle modifiche admin.
    - Success criterion MVP: un admin corregge da UI un nodo con tipo sbagliato o frontmatter rotto, vede il diff, salva con commit/PR esplicito e il grafo/detail si aggiornano senza interventi manuali sul repository.

---

## 🔴 Fase 4: Forge Independence & Deep History

Sfruttare la maturazione del backend GitHub-first per standardizzare il boundary verso altri forge e usare la projection SQLite come motore per la vista temporale e il local/offline mode.

**Principio guida:** il trait non va estratto da `GithubClient` in modo cosmetico. Va ricavato dalle capability realmente saturate in Fase 3: repository discovery, snapshot tree/blob, mutazioni branch/PR, work item sync, webhook normalization e policy di rate limit. Il pezzo più maturo è il transaction layer (`GitTransaction` in `brain-storage::git_transaction`) — la Fase 4 parte da lì (vedi 4.0), non dal client HTTP.

- [ ] **4.0 Git Transaction Layer Maturation** _(prerequisito di 4.1)_

    Razionale: `crates/brain-storage/src/git_transaction.rs` è oggi il pezzo più maturo del workspace — builder fluente, preconditions duali (`expect_absent` / `expect_sha`), retry mirato solo su 422 fast-forward, riuso blob content-addressed, outcome osservabile, invariante No Dual-Write rispetto alla projection. È stato validato però contro un solo call site applicativo (`api/file_ops.rs::rename_brain_file`) e contro un fallback PR composto inline nel `write_orchestrator`. Prima di estrarre un `ForgeAdapter` trait servono altre due/tre forme d'uso reali per evitare di cementare la firma sbagliata.

    Obiettivo della 4.0: chiudere quelle forme nel runtime esistente, **senza ancora** estrarre un crate esterno o un trait multi-forge. Le primitive di sotto vengono disegnate in modo che il loro nome e contratto siano riusabili tali e quali da un futuro `ForgeTransactionAdapter`.

    - **`BranchTransaction` esplicita** — oggi il fallback PR (write_orchestrator: `prepare_pr_write` → `create_branch_from_sha` / `ensure_fork` + `create_branch_with_retry` → `commit_transaction` su branch effimero → `open_pull_request`) è una sequenza ad-hoc spalmata fra orchestrator e storage. Promuoverla a tipo: `BranchTransaction::new(base_sha, branch_name, …).add(GitTransaction).commit_all(http, gh, token) → BranchTransactionOutcome { branch, head_sha, commits, pr: Option<…> }`. Rollback esplicito = delete del branch se uno step fallisce dopo la creazione. Vincolo: niente magia sul `head` del branch utente — la PR resta un'azione separata.

    - **`expect_tree_sha(prefix)`** — generalizzazione di `expect_sha` a livello directory. Oggi puoi dichiarare "il file X è a sha Y", non "la sottocartella Z non è cambiata sotto di me". Serve a Admin Node Control (move/rename di intere directory, retag bulk, fix orphan) e al futuro auto-binding 4.5 quando la mutazione tocca più file derivati dal work item. Implementazione: leggere `base_tree` recursive (già fatto) e calcolare un hash deterministico delle entry sotto `prefix`, oppure verificare che il `tree.sha` della subdir non sia cambiato.

    - **`transaction.plan(http, gh, token) → TransactionPlan`** — dry-run che ritorna l'elenco di blob/tree da creare e gli eventuali precondition failure rilevati, senza eseguire `PATCH /git/refs`. Use case principali: il Visual Configuration Editor di 3.4 quando estende a editorial config (preview diff prima del commit), Admin Node Control (impatto su backlink/binding prima di un rename strutturale), Auto-binding 4.5 (preview "create issue + write file" prima di committare). Questo è anche il pezzo che rende la futura conflict resolution (4.4) implementabile senza shortcut.

    - **`TransactionObserver` trait** — hook su `attempt_started / precondition_failed / fast_forward_retry / committed`. Oggi i log/audit sono inline. Estrarre l'observer chiarisce cosa appartiene al transaction layer (eventi tecnici) e cosa appartiene all'audit applicativo (intent: `propose_write`, `propose_rename`, ecc.).

    - **Idempotency keys (opzionale, da valutare)** — hash deterministico di `(path-set, expected-shas, message-prefix)` per de-dup di replay webhook in 3.2-β bidirectional. Da introdurre solo se in produzione vediamo replay reali; non designare in astratto.

    - **Crate split — non ancora.** L'estrazione in crate esterno `git-tx-github` (o nome migliore) viene **rinviata a 4.1**, perché solo a quel punto avremo: (a) un secondo adapter forge che valida la forma del trait, (b) un error type proprio (oggi dipendenza concreta su `BrainError`), (c) un client trait minimo (oggi `GithubClient` è una struct concreta). Estrarre prima cementerebbe scelte locali. Pubblicazione su crates.io ulteriormente rinviata: solo dopo ≥3 mesi di superficie API stabile in produzione su due adapter reali.

    - **Crate Rust di terze parti — valutati e scartati per questa slice.** Per archiviare la decisione: `octocrab` ha solo wrapper sottili sulla Git Data API e zero transazionalità — sostituirebbe `GithubHttp` (50 righe) con ~5 KLOC nel binario senza vantaggi. `gix`/`git2` parlano protocollo Git locale, non REST forge: rilevanti per 4.3 (Local mode), non qui. `backon`/`backoff` sostituirebbero le ~30 righe di `BackoffPolicy` con una dipendenza che capisco peggio. `reqwest-retry` non distingue retry semantici (422 fast-forward = retry, 422 precondition = abort) e quindi non è usabile.

    - Success criterion: ogni write path applicativo (save, delete, rename, work item mutation, fallback PR) passa per `GitTransaction` o `BranchTransaction`; `plan()` è chiamato almeno da una superficie reale (config editor o admin); il file `git_transaction.rs` è promosso a sottomodulo dichiarato pubblico di `brain-storage` con test estratti da inline a `tests/git_transaction/`.

- [ ] **4.1 `ForgeAdapter` Trait (capability-driven)**
    - Estrarre un boundary che copra i bisogni reali del runtime, non un semplice wrapper HTTP. Le capability minime previste sono: target discovery/listing, lettura tree/blob, write commit/branch/ref update (sopra le primitive `GitTransaction` / `BranchTransaction` chiuse in 4.0), PR/MR creation, work item issue mutation, webhook verification + event normalization.
    - Valutare esplicitamente se implementarlo come un singolo trait o come famiglia di subtrait/capabilities (`ForgeRepoAdapter`, `ForgeTransactionAdapter`, `ForgeCollaborationAdapter`, `ForgeWebhookAdapter`) per evitare un "lowest common denominator" troppo povero o un god-trait ingestibile. La separazione `ForgeTransactionAdapter` è naturale: GitHub Git Data API, GitLab Repository Files API e Gitea hanno semantica simile ma endpoint diversi, e la transazione con preconditions resta utile a tutti.
    - Estrazione del crate `git-tx-<forge>` (o nome migliore): avviene **qui**, non in 4.0. A questo punto esistono due adapter reali che validano la forma del trait, un error type dedicato (`GitTxError` / `ForgeError`) e un client trait minimo che sostituisce la dipendenza concreta su `GithubClient`. La crate resta interna al workspace finché la superficie API non è stabile in produzione per ≥3 mesi su due adapter; solo allora valutare pubblicazione su crates.io.
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
    - Riuso esplicito del `transaction.plan()` di 4.0: il preview hunk-based non è una funzione separata, è il dry-run della transazione che evidenzia le precondition fallite e mostra cosa cambia per ciascun path coinvolto.
    - Questo asse è particolarmente importante se la 3.3 porta davvero contributor multipli e branch temporanei: il conflitto non sarà più eccezione, ma percorso operativo ordinario.
- [ ] **4.5 Auto-binding work item ↔ issue/tracker esterni**
    - Oggi il binding di un `WorkItem` verso un'issue GitHub (o futuro tracker) è interamente manuale: l'utente compila `system / project / item_key / url` nel form di `BindWorkItem`. Non c'è creazione automatica di issue dal documento Brain, né auto-discovery di issue già esistenti che referenziano il `brain_id` o il path del nodo.
    - Conseguenza pratica: `system_of_record = split | external` richiede oggi che l'issue esista già, sia stata creata fuori da Brain UI e che l'utente conosca il numero. Per workflow `Brain-first` (creo il task in UI → voglio che l'issue venga creata su GitHub e bindata automaticamente) non c'è ancora un percorso supportato.
    - Direzione: introdurre un'azione `Create & bind issue` lato UI che, sopra l'orchestratore di scrittura permission-aware (3.3) e l'adapter forge (4.1), apra l'issue con titolo/body derivati dal documento, applichi le label `brain:*` da `label_taxonomy` e popoli `external_binding` in un singolo flusso atomico. In parallelo, esplorare auto-match su issue esistenti (es. ricerca per `brain_id` nel body, per path file, o tag convenzionale) come hint nel binding form invece che come legame implicito.
    - Vincolo: l'auto-binding non deve diventare implicito o silenzioso — `system_of_record` resta esplicito, e la creazione issue richiede comunque conferma utente e capability di scrittura sul forge target.
- [ ] **4.6 Multi-Tab Detail/Editor Workspace**
    - Sostituire `selected_path: RwSignal<Option<String>>` con uno stato a tabs (`Vec<TabState>` + active index), così l'utente può aprire più nodi simultaneamente senza perdere contesto durante cross-reference.
    - Le tab vivono nel pannello destro esistente (no two-pane split): cambia il modello di stato, non il layout. Esc cascade (caveat #13) si estende al close della tab attiva prima del clear della selezione.
    - URL contract: `?tabs=path1,path2&active=1` sopra il routing multi-tenant già introdotto in 3.1.
    - Prerequisito reale: 4.2 Temporal Graph View, che già introduce un secondo "modo" del detail panel (live vs historical SHA). Disegnare tabs dopo 4.2 evita di astrarre nel buio.

---

## 🟣 Fase 5: AI & Automations Ecosystem

Trasformare la Brain UI in un assistente attivo tramite IA e trigger di automazione esterni.

- [ ] **AI Assistant Proxy (Copilot Integration)**
    - Assistente AI nell'editor Leptos: generazione markdown, autocompletamento, summarization, tagging.
    - Proxy sicuro in Axum che usa l'OAuth token dell'utente per le API AI di GitHub/Copilot (RBAC + accounting corretto).
    - L'esposizione read-side della projection (graph, work_items, nodi per tag/tipo) avviene via endpoint MCP-compliant (SSE/HTTP) sopra l'infra SSE già in [crates/brain-app/src/server/sse.rs](../crates/brain-app/src/server/sse.rs) e l'OAuth in [crates/brain-app/src/server/auth.rs](../crates/brain-app/src/server/auth.rs), riutilizzando le query parametriche introdotte in 3.5. Auth via PAT/scoped token (separato dalle session OAuth), così tool MCP-compatibili (Cursor, Claude Desktop, ecc.) possono interrogare la knowledge base puntando a Brain UI senza riusare la sessione browser. Design rinviato a quando RBAC di 3.3 è stabile.
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

4. ~~`prose-sm` typography sizing is a guess~~ — **DONE 2026-04-26 (no-op)**. Both render sites (`detail_panel.rs`, `editor.rs`) already use `prose prose-invert max-w-prose` (default size, equivalent to `prose-base`), not `prose-sm`. The original caveat was stale. Future tuning of `tailwind.config.js` `typography.invert` palette remains available if real content warrants it.

5. ~~Update path regenerates frontmatter from templates~~ — **DONE 2026-04-22**. `merge_frontmatter` fa overlay dei campi del form sulla mappa preservata invece di rigenerare da template. Tests in `brain-app::api::merge_frontmatter_tests`.

6. ~~No auto-refresh after out-of-band commits~~ — **DONE 2026-04-24**. `RefreshBrainGraph` server fn + `RefreshButton` component in the knowledge header now trigger a full rebuild of the local SQLite projection after invalidating graph, template, and config caches. The button closes the "commit esterno → reload manuale" gap and doubles as manual reindex/drift recovery until webhook/SSE sync lands.

7. ~~Rename issues N+2 Contents API commits~~ — **DONE 2026-04-25**. `rename_brain_file` now produces a single commit via `GithubStorage::atomic_rename` (Git Data API: blobs → tree → commit → ref update with `force=false`). Fast-forward conflicts (422) are retried with capped exponential backoff; uploaded blobs are reused across retries since they're content-addressed.

8. ~~Graph cache is process-global, not user- or target-scoped~~ — **DONE 2026-04-24**. All caches in `brain-storage` (graph, template) and `config_loader` are now keyed by `TargetKey({org}/{repo}/{branch})` via `OnceLock<Mutex<HashMap<TargetKey, _>>>`. `GithubHttp` is target-agnostic (plain `Arc<reqwest::Client>`); each `GithubStorage` / call-site supplies its own `GithubClient` built from an explicit `TargetConfig`. Safe for Phase 3 multi-target without re-architecting.

9. ~~`register_explicit` boilerplate is LTO-coupled~~ — **DONE 2026-04-24**. `SERVER_FNS: &[&str]` const + `include_str!`-based test in `api.rs` catches any `#[server]` fn not listed in the const before it reaches CI.

10. ~~Sync visibility is still page-local~~ — **DONE 2026-04-26**. `LiveSync` (EventSource subscription) e `SyncStatusBanner` ora vivono in `App`, sopra `<Routes>`, leggendo `graph_version`/`sync_status` esposti via `provide_context(GraphVersion)` / `provide_context(SyncStatusSignal)`. Admin su `/admin` o future route work-item vedono lo stesso banner `Stale Data` di `/knowledge`. `RefreshButton` resta page-local in Knowledge ma muta gli stessi signal globali.

11. **UI limitations (canvas)** — No animated transitions between viewBox states (snap is instant). Nodes near graph edges show empty area outside the data space. Hover does not recenter, only selection does. No zoom: scale stays 100×100. **Mitigation 2026-04-26**: hover/selection states on individual nodes and edges now crossfade via CSS `transition` on `r`, `stroke`, `stroke-width`, `stroke-opacity`, and `filter` — pure CSS, no RAF/JS. The full viewBox tween + zoom controls remain Phase 3.6.

12. ~~Filters are not URL-persisted~~ — **DONE 2026-04-26**. `active_tags` and `active_types` round-trip through `?tags=` and `?types=` query params alongside the existing `?path=`. Refresh and link-share both restore the filtered view. Navigation uses `replace: true` so toggling filters doesn't pollute history. Tags are normalized lowercase in both directions; types preserve case (they map to `BrainConfig.node_types[].name`).

13. ~~No keyboard dismissal~~ — **DONE 2026-04-26**. Esc cascade in `KnowledgeView` (hydrate-only `keydown` listener on `window`): closes the editor first if open, otherwise clears the selected node. Skipped while focus is in `input`/`textarea`/`select`/`contenteditable` so Esc doesn't fight IME or form-local handlers. Listener is cleaned up via `on_cleanup` on route change.

14. **`Stale Data` banner can flash before login** — `SyncStatusBanner` lives above `<Routes>` (caveat #10) and its signal is hydrated on app boot, so unauthenticated users hitting the public landing or the OAuth redirect path can briefly see the banner before the session resolves. The banner should be gated on auth state (or at least on having an active target), not just on `sync_status`. Likely fix: thread an `is_authenticated` / `has_active_target` derived signal into `SyncStatusBanner` and short-circuit render when false. Low priority but unpolished — first impression for new users.

15. **No auto-binding to external issues/trackers** — Il binding di un `WorkItem` verso un'issue del forge è oggi interamente manuale: l'utente apre il binding form e compila `system / project / item_key / url` a mano (vedi `BindWorkItem` server fn e `DetailPanel::WorkItemCard`). Non esiste un'azione `Create issue from this work item` che apra l'issue su GitHub e popoli `external_binding` in un singolo passaggio, né auto-discovery di issue esistenti che già referenzino il `brain_id` o il `content_path`. Conseguenza: `system_of_record = split | external` richiede che l'issue esista già fuori da Brain UI. La soluzione naturale vive sopra il `ForgeAdapter` di 4.1 ed è tracciata come follow-up in **4.5 Auto-binding work item ↔ issue/tracker esterni**. Workaround attuale: creare l'issue su GitHub, copiare numero/URL, bindare manualmente.

16. ~~Asset proxy 404 silenzioso su repo privati~~ — **DONE 2026-04-29**. `serve_asset` in [crates/brain-app/src/server/assets.rs](../crates/brain-app/src/server/assets.rs) interrogava `raw.githubusercontent.com` con `Authorization: Bearer <token>`, ma quell'host **non accetta** bearer auth su repo privati (richiede `?token=` query param legacy o Contents API). Ritornava 404 silenzioso. Sintomo concreto: il primo asset prodotto dal Brain Clipper (`assets/2026/04/schermata-del-2026-04-29-…png`) era committato correttamente su `main` (Contents API: 200, sha matching, size 515850) ma non si vedeva in Brain UI. **Fix**: il proxy ora chiama la Contents API con `Accept: application/vnd.github.raw`, che restituisce i bytes raw direttamente con la stessa autenticazione bearer di tutte le altre chiamate. Verificato end-to-end con probe curl: 200, content-type corretto, file scaricato byte-identical al locale. Aggiungere test wiremock-based che fallisca se qualcuno reintroduce `gh.raw_base()` nel proxy.

17. **Node hover flicker on graph canvas** — Hovering some nodes produces a visible flicker / re-trigger of the hover state instead of the smooth crossfade introduced in caveat #11. **Diagnosi 2026-04-30**: tre ipotesi originali (a/b/c) ridotte a una probabile causa dopo lettura di [graph_canvas.rs:535-603](../crates/brain-app/src/knowledge/graph_canvas.rs#L535-L603).
    - **(b) overlap di elementi che intercettano il pointer — ESCLUSA.** Il `<text>` label ha già `pointer-events:none` ([graph_canvas.rs:598](../crates/brain-app/src/knowledge/graph_canvas.rs#L598)). Non c'è halo separato né bounding box trasparente nello struct nodo: solo `<g>` con handlers + un singolo `<circle>` visibile + `<text>` non-interattivo. Il fix tipico ("apply `pointer-events:none` to the labels") qui è già in essere.
    - **(c) `r` attribute oscillante — CAUSA PROBABILE.** Il `<circle>` ha `r = base_r + bump` con `bump = 0.5` su hover ([graph_canvas.rs:561-566](../crates/brain-app/src/knowledge/graph_canvas.rs#L561-L566)). Pointer lento sul bordo del cerchio: hover ON → `r` aumenta → bordo si sposta *sotto* il cursore → frame dopo `mouseleave` (cursore ora *fuori* dal cerchio appena ingrandito mid-transition) → `r` decresce → `mouseenter` di nuovo. Feedback positivo geometrico: l'hit-area visiva e l'hit-area effettiva sono lo stesso cerchio che cambia dimensione in funzione del proprio stato di hover.
    - **(a) re-render del parent — DA ESCLUDERE PRIMA DI FIXARE.** `bright`/`is_selected`/`is_hovered` sono `Memo` chiusi su `selected`/`hovered`. Se Leptos sta ricostruendo il `<g>` invece di mutare attributi quando `hovered` cambia, la transizione CSS si resetterebbe a ogni hover state change. Verificare via DevTools (lo stesso `<circle>` DOM node deve persistere across hover) prima di committare il fix di (c) — se (a) è anche presente, il fix di (c) da solo non basta.
    - **Direzione fix (per quando ci si torna sopra)**: disaccoppiare hit-target da visivo. Un secondo `<circle>` trasparente con `r = base_r + max_bump + buffer` come pointer target (handler `mouseenter`/`mouseleave` lì), il `<circle>` visibile mantiene `r` libero di transire senza influenzare l'hit detection. Pattern standard SVG, niente JS animation loop.
