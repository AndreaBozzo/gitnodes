# Brain UI Roadmap

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

**Stato architetturale corrente:** Git e il repo target restano la *Single Source of Truth*. SQLite copre sessioni, audit log e una projection locale target-scoped per `nodes`, `edges`, `files`, `backlinks`, `work_items` e `work_item_bindings`, con rebuild esplicito e watermark/error state. La sincronizzazione inbound baseline (webhook/SSE) e la materializzazione operativa dei work item sono chiuse; il gap residuo sta ora nelle mutazioni bidirezionali verso il forge, nel routing multi-target end-to-end e nella gestione permission-aware delle scritture.

**Realignment 2026-04-26:** Fasi 3 e 4 non vanno più trattate come estensioni speculative. La base costruita in Fase 2B (projection SQLite multi-tenant, target-scoped cache, `brain_id` stabile, rename atomici via Git Data API) consente di pianificare il lavoro successivo come evoluzione concreta del runtime esistente. In pratica: Fase 3 diventa la fase del **workspace multi-tenant collaborativo**, Fase 4 quella della **standardizzazione forge/time-travel/local mode**.

**Realignment 2026-05-02:** Fase 3 è ora una fase di **shipping e dogfooding**, non un contenitore per ogni idea corretta emersa dall'architecture review. Il core collaborativo è già shippable: routing multi-target, work item sync, PR fallback, saved views, graph polish, repo structure, target identity, UI posture e sidebar posture sono chiusi. Le nuove superfici grandi (embed analytics, BYOB/blob, FTS, activity stream, advisory lock, local/offline, forge abstraction) restano tracciate, ma non sono gate di Fase 3. Il gate attuale è: usare il prodotto con un contributor limitato, raccogliere attrito reale, correggere solo bug/UX blocker piccoli, poi aprire la prossima fase con priorità esplicita.

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

- [ ] **Pokemon Brain mock con contributor limitato** _(assegnato a `@JacoTube` nel repo Brain, 2026-04-28)_
    - Creare una mini knowledge base Pokemon come target/sandbox config-driven, senza riusare la tassonomia del Brain principale: `.brain-config.yml` custom, tipi come `pokemon`/`trainer`/`gym`/`route`/`battle-report`/`quest`, template dedicati e saved view proprie.
    - Esercitare le feature shippate in Fase 3: routing target-aware, config loader YAML, node types custom, work item mutations, binding a GitHub Issue, saved view/config flow quando permesso, direct-write vs branch+PR fallback, webhook/SSE reconciliation e graph canvas polish.
    - Usare i commenti della GitHub Issue bindata come thread QA/collaborazione e validare la nuova sezione `Comments` del detail panel; eventuali gap rimasti vanno estratti come follow-up (`issue_comment` SSE/cache timeline).
    - Vincolo operativo: `@JacoTube` lavora da contributor non-admin; `main` resta protetto e ogni modifica finale passa da PR review di Andrea/Matteo.
    - Success criterion: PR aperta dal contributor con note QA su cosa ha funzionato, cosa e' stato confuso e quali follow-up vanno estratti prima della prossima fase.
- [ ] **Release note / operator checklist Fase 3**
    - Documentare cosa è shippato, quali caveat restano accettati temporaneamente e quali env/config devono essere verificati prima di far usare il prodotto a un contributor limitato.
    - Aggiornare README/ROADMAP in modo che il prossimo lavoro parta da feedback reale, non da un'altra lista di "sarebbe giusto".
    - Success criterion: un maintainer può spiegare in 10 minuti cosa Brain UI fa oggi, cosa non promette ancora, e come recuperare dai failure mode noti senza scavare nel codice.

---

## 🟤 Next Hardening Lane: prima di nuove superfici esterne

Questa lane nasce dai finding corretti emersi durante l'architecture review, ma **non blocca la chiusura di Fase 3**. Diventa il primo candidato per la prossima fase se il dogfooding conferma che il core collaborativo regge.

- [ ] **Security & Content Trust Baseline** _(pre-embed / pre-blob / pre-AI)_

    Razionale: Brain UI renderizza contenuto controllato dal repository e da provider esterni (Markdown, commenti issue, asset, in futuro iframe e AI-generated text). Finché l'app era un editor interno GitHub-first il rischio era gestibile; con dashboard embed, BYOB e assistant diventa un vero trust boundary. Questo lavoro va fatto prima di embedded analytics, BYOB/blob e AI, ma non è un prerequisito per dogfoodare il core già chiuso.

    - Markdown rendering policy esplicita: decidere se disabilitare raw HTML a monte o sanitizzare l'HTML generato prima di `inner_html`. La policy deve coprire documenti Brain, preview editor e commenti GitHub renderizzati nel detail panel.
    - CSP baseline globale: `default-src 'self'`, `script-src` compatibile con Leptos/hydration, `img-src` limitato a self/data/blob e domini configurati, `frame-src` derivato dall'allowlist futura, `object-src 'none'`, `base-uri 'none'`.
    - Upload safety: validare content-type e magic bytes lato server, non solo estensione; limitare SVG upload finché non esiste un sanitizer/policy dedicata. **Micro-fix 2026-05-01:** `UploadAsset` non accetta più nuovi `.svg`; gli SVG storici restano serviti dal proxy come asset legacy.
    - CSRF per mutazioni server fn: `SameSite=Lax` protegge bene il callback OAuth, ma le mutazioni POST autenticate meritano un token anti-CSRF o un header same-origin verificato, soprattutto prima di iframe e automazioni.
    - Session/token posture: decidere esplicitamente tra OAuth token utente, GitHub App installation token e token scoped per MCP. Tracciare TTL, revoca, cifratura at-rest della session store e rotazione dei secret.
    - Success criterion: esiste una test matrix di sicurezza che prova XSS da Markdown/commenti, upload asset ostili, server fn cross-site e iframe non allowlistati; nessuna nuova superficie esterna può bypassare questa policy.

- [ ] **Failure-Mode Matrix & Operational Readiness**

    Razionale: il roadmap copre bene le capability, ma le feature collaborative diventano affidabili solo se i failure mode sono disegnati come percorsi di prodotto. Ogni nuova slice dovrebbe dichiarare cosa succede quando GitHub non risponde, il branch si muove, il permesso cambia, il provider sync fallisce, la projection è stale o il config è invalido.

    - Aggiungere una tabella `feature → failure mode → UX → audit/log → retry/recovery` per le superfici principali: save, rename, work item sync, webhook rebuild, config editor, embed URL, blob URL, FTS rebuild.
    - Health endpoint operativo (`/healthz`/`/readyz`) che distingua app boot, SQLite raggiungibile, session store migrato, projection pool pronto e config target leggibile quando esiste un target default.
    - Background job/outbox leggero per riconciliazioni costose o retryabili: provider sync, webhook replay, future blob cleanup. Non introdurre un job system pesante finché SQLite + tokio task supervisionato bastano.
    - Success criterion: quando una dipendenza esterna degrada, Brain UI mostra uno stato azionabile invece di un errore generico; gli operatori hanno log/audit sufficienti per capire se serve refresh, retry, re-login o fix di config.

- [ ] **Projection Schema v2 & SQLite Operations**

    Razionale: le prossime feature usano SQLite non più solo come read model minimale. FTS5 richiede corpo/titolo/tag indicizzabili; Activity Stream e co-authorship richiedono commit metadata; Watch/Follow e advisory locks introducono dati user-scoped; Temporal Graph userà projection effimere. Prima di appoggiare tutto questo sulla tabella corrente, serve una piccola maturazione del layer dati.

    - Versionare le migrazioni SQLite invece di affidarsi solo a `CREATE TABLE IF NOT EXISTS`; ogni schema change deve essere ripetibile su deploy Railway/prod e testabile in CI.
    - Estendere `files`/`nodes` con i dati che servono davvero alle feature pianificate: `body_text` o tabella FTS separata, `frontmatter_json`, `commit_author`, `commit_message`, `last_commit_at`, e un modello chiaro per `node_authors`.
    - Definire retention/cleanup per audit log, sessioni scadute, editing locks, watch notifications e projection temporanee. SQLite resta leggero solo se il ciclo di vita dei dati è esplicito.
    - Aggiungere una vista admin/status della projection: schema version, last success/error per target, file/node/work item count, rebuild duration, webhook lag, rate-limit snapshot quando disponibile.
    - Success criterion: FTS5, Activity Stream, Watch/Follow e Temporal Graph hanno una base dati coerente e migrabile; un deploy nuovo o esistente può aggiornarsi senza rebuild manuale cieco.

---

## 🧊 Future Product Expansion: tracked, not current phase

Queste sono direzioni valide, ma tornano in gioco solo dopo il closeout di Fase 3 e la hardening lane minima. Ognuna deve rientrare come slice autonoma con un trigger reale, non come accumulo automatico sulla fase corrente.

- [ ] **Embedded Analytics Views**

    Razionale: Brain UI è un control plane su una knowledge base editoriale, ma una parte crescente del valore aziendale vive in dati operativi pesanti (Postgres/Supabase, ads Instagram via Airbyte, metriche prodotto) che non ha senso ingerire dentro la projection SQLite né renderizzare con charting library lato WASM. La via naturale è far convivere dashboard esterni (Metabase, Superset, PowerBI, Grafana) come *view* di prima classe accanto ai grafi/nodi, embeddati via iframe, configurati dallo stesso `.brain-config.yml` e gated dalla stessa RBAC. Brain UI resta agnostico rispetto al data source: non parla mai col DB, non sa cosa c'è dentro il dashboard, sa solo che esiste, chi può vederlo, e dove punta.

    Obiettivo dichiarativo: usare il pattern `ViewSpec` introdotto da 3.4-α come superficie unificata per tutte le viste Brain (saved filter views *e* embedded analytics), evitando un secondo schema parallelo. Zero charting library aggiunte al bundle WASM. L'iframe è opaco per design.

    **Realignment ex-ante:** la slice viene spezzata in α (whitelist statico, no JWT) → β (JWT signing per provider che lo supportano), coerente col precedente di 3.2/3.4. α deve essere end-to-end shippable da solo — niente "dummy server fn che ritorna URL hardcoded": un dashboard pubblico/whitelistato è già un caso reale.

    Vincoli condivisi (validi per α/β):
    - Lo schema `views` di 3.4-α viene esteso con un campo `type: filter | iframe` (default `filter` per backward compat); le `iframe` view portano `url`, opzionalmente `provider` (per la β), e `requires_role` riusa la capability matrix di 3.3.
    - Allowlist domini embeddabili dichiarata in `.brain-config.yml` (`embed_allowed_origins: [metabase.example.com, ...]`). Validata server-side in fase di config parse e *ri-validata* dalla server fn che restituisce l'URL — il frontend non può aggirarla.
    - CSP `frame-src` calcolato server-side dall'allowlist (non hardcoded), così aggiungere un dominio è un edit di config, non un deploy. Header `sandbox="allow-scripts allow-same-origin"` di default sull'iframe; override esplicito nel `ViewSpec` se il provider lo richiede.
    - Zero charting/visualization library aggiunte al bundle WASM. Se uno use case sembra richiederlo, è il segnale che dovrebbe essere un dashboard esterno embeddato, non nativo.
    - Editor visuale: il pattern `iframe` view va supportato esplicitamente in 3.4-β (o γ se si preferisce raggrupparlo coi long-tail), non lasciato a raw YAML, perché l'allowlist e il `requires_role` sono campi che gli admin devono poter modificare in UI.

    - **α Whitelist-based iframe views** _(scope minimo per validare il pattern)_
        - Estensione schema: `ViewSpec` accetta `type: iframe` con `url`, `requires_role`. Allowlist `embed_allowed_origins` a livello root del config. Validazione `BrainConfig::parse`: ogni `iframe` view deve avere `url` il cui host è in allowlist; slug unici come per 3.4-α.
        - Server fn `GetEmbedUrl(target, slug) → Result<EmbedUrlResponse, ServerFnError>`: legge config cached, verifica esistenza view + match slug, verifica `requires_role` contro la capability matrix dell'utente, ri-valida il dominio contro allowlist (defense in depth), ritorna URL + eventuali sandbox flags. Audit log `embed_url_issued { slug, target }`.
        - Componente `<EmbeddedAnalytics/>` in `brain-app/frontend`: `create_resource` su `GetEmbedUrl`, `<Suspense>` con skeleton, render `<iframe>` full-content-area al success. Errore esplicito (non blank) su `Blocked by permissions` / `Domain not allowlisted` / `View not found`.
        - Router/sidebar: la sidebar dinamica di 3.4-α già renderizza le view; estendere il render per distinguere chip filter (set query params) da chip iframe (navigate a `/{org}/{repo}/views/{slug}`). Route nuova che monta `<EmbeddedAnalytics/>` con lo slug come param.
        - Esplicitamente fuori da α: JWT signing, secret management, provider-specific (Metabase signed embeds, Superset guest tokens, ecc.), audit del *contenuto* visto dall'utente, multi-tenancy a livello dashboard (un dashboard Metabase per org).
        - Success criterion: un admin aggiunge un dashboard Metabase pubblico (o un Grafana share link) all'allowlist e come `iframe` view, lo vede comparire nella sidebar con il giusto `requires_role`, l'utente target apre la view e il dashboard è renderizzato dentro Brain UI senza che il backend abbia mai parlato col data source.

    - **β Signed embeds (provider-aware)** _(post-feedback expansion)_
        - Estensione `ViewSpec` con `provider: metabase | superset | powerbi | grafana | none` e blocco `signing_config` per-view (resource id, embed type, scope params).
        - Secret per-target in env var (`BRAIN_EMBED_SECRET_<TARGET_ID>` o equivalente — schema esatto da disegnare quando arriviamo qui). Mai nel config file, mai nel bundle WASM.
        - `GetEmbedUrl` esteso: per provider che lo supportano, firma payload JWT/HMAC con TTL corto (≤ 10min), include user id e role per provider che fanno row-level security via embed params. URL firmato non cachato lato server (TTL applicato lato provider).
        - Audit esteso: `embed_url_issued` include `provider`, `expires_at`, `signed: true`. Sufficiente per accountability senza loggare il contenuto visto.
        - Forma esatta dei `signing_config` per-provider va disegnata alla luce di α in produzione e dei provider effettivamente richiesti dagli utenti, non pre-pianificata.

    Esplicitamente fuori dagli embedded analytics (entrambi gli slice):
    - Charting nativo / visualizzazioni custom dentro Brain UI — anti-goal esplicito.
    - Embed bidirezionale (iframe che pubblica eventi al parent Brain UI via `postMessage`) — superficie di sicurezza e UX troppo grande per questo scope.
    - Auto-discovery dei dashboard disponibili sul provider (es. listing di tutti i dashboard Metabase visibili all'utente) — manuale via config.
    - Auth federation Brain → provider (SSO, token exchange, OAuth delegation) — vive in fase 5 insieme all'AI Assistant Proxy se mai necessario.
    - Pre-rendering server-side dei dashboard come immagini cachate — fuori scope, è un dominio diverso (snapshot/reporting, non control plane).

    Vincolo trasversale: se l'iframe pattern non basta per uno use case (es. l'utente vuole filtri Brain UI che si propagano nel dashboard), la risposta di default è "documenta il limite" non "estendi Brain UI". L'integrazione cross-frame è una superficie di rischio sproporzionata rispetto al valore di un control plane.

- [ ] **BYOB (Bring Your Own Blob) & External Asset Strategy**

    Razionale: Git è la Single Source of Truth perfetta per semantica, testo e metadati, ma è storicamente pessimo per i file binari. Oggi l'editor di Brain UI committa le immagini in `assets/YYYY/MM/`: questo gonfia il repository e — soprattutto — rompe l'esperienza della Fase 3.3 (follow-up esplicito a [ROADMAP.md:159](docs/ROADMAP.md#L159)): un asset caricato in un branch temporaneo via PR non è immediatamente referenziabile dal markdown live, quindi la preview del draft è rotta finché la PR non viene mergiata. In parallelo, il team ha bisogno di integrare nativamente documenti e spreadsheet ospitati su Google Workspace senza passarli per Git.

    Obiettivo: separare il Data Plane (i blob binari) dal Control Plane (i file Markdown su Git). Introdurre l'astrazione `BlobAdapter` prima del `ForgeAdapter` di 4.1, garantendo che i commit contengano sempre e solo testo. Chiude il debito aperto da 3.3 sul fallback PR per asset.

    **Realignment ex-ante:** la slice viene spezzata in α (R2/S3 + chiusura debito 3.3) → β (frontmatter agnostico + UI consumer) → γ (Google Workspace come provider redirect-only), coerente col pattern di vertical slice già usato nel progetto. α deve essere end-to-end shippabile da solo — la fix del fallback PR ha valore immediato anche senza UI dedicata e senza Google Workspace.

    Vincoli condivisi (validi per α/β/γ):
    - Secret di provider esclusivamente in env var (`BRAIN_BLOB_<PROVIDER_ID>_*` — schema esatto da disegnare in α). Mai nel `.brain-config.yml`, mai nel bundle WASM.
    - `BlobAdapter` chiamato solo server-side: il frontend riceve URL già risolti via server fn dedicata, non chiama mai il provider direttamente. Defense in depth: il config dichiara *che* provider esistono, l'env var dice *come* autenticarsi.
    - `resolve_url` ritorna URL a tempo (presigned R2/S3 o redirect link Google) ma **non** è cachato server-side: il TTL è applicato dal provider, l'auditability vive nel log.
    - Audit `blob_url_issued { provider_id, ref_id, target, user, expires_at }` per ogni risoluzione, sufficiente per accountability senza loggare il contenuto.
    - Capability gating coerente con 3.3: upload asset gated su `can_write_default_branch || can_review_via_pr`; risoluzione URL gated su `can_read`; admin del blocco `blob_providers` nel config gated su `can_admin_config` (fluisce dall'editor visuale di 3.4-γ quando arriva, raw YAML nel frattempo).
    - Editor visuale: il blocco `blob_providers` è long-tail config — vive nell'espansione futura del visual config editor, non blocca BYOB. Raw YAML è l'escape hatch durante α/β.

    - **α BlobAdapter trait + R2/S3 provider + fallback PR fix** _(scope minimo, chiude debito 3.3)_
        - `BlobAdapter` trait in `brain-storage` con responsabilità iniziale singola: `resolve_url(provider_id, ref_id) → Result<ResolvedUrl, BlobError>` + `upload(provider_id, bytes, content_type) → Result<BlobRef, BlobError>`. Implementazione `S3Adapter` (Cloudflare R2 via API S3-compatibile) come unico provider di α.
        - Estensione `BrainConfig`: blocco `blob_providers: [{ id, type: s3, bucket, region, public_base_url? }]`. Validazione in `BrainConfig::parse`: id univoci, `type` riconosciuto, env var corrispondenti presenti al boot (warning, non error, per non bloccare repo che non usano blob).
        - Server fn `UploadAsset(target, provider_id, bytes, content_type) → Result<BlobRef, ServerFnError>`: gated su capability matrix, audita `blob_uploaded`, ritorna `{ provider_id, ref_id, url }` per riuso immediato lato editor.
        - **Fix fallback PR**: l'orchestratore di scrittura permission-aware (3.3) intercetta gli asset upload prima del commit. Se il provider blob è configurato per il target, l'asset finisce su R2 con URL stabile; la PR generata contiene solo il diff Markdown che referenzia l'URL R2. Niente conflitti sui binari, preview live nel branch funzionante. Se nessun provider blob è configurato, fallback al comportamento attuale (commit binario, debito noto).
        - Esplicitamente fuori da α: `external_assets` frontmatter (β), UI dedicata nel detail panel (β), Google Workspace (γ), migrazione asset vecchi.
        - Success criterion: un utente senza write access carica un'immagine nell'editor; il blob finisce su R2 con URL stabile, la PR generata contiene solo il diff Markdown, la preview live nel branch funziona come per i file di testo. Audit `blob_uploaded` e `blob_url_issued` presenti.

    - **β Frontmatter agnostico + UI consumer** _(post-α, abilita allegati strutturati)_
        - Schema frontmatter: blocco `external_assets: [{ provider, ref_id, name, kind? }]` come campo first-class, non più link markdown crudi per gli allegati complessi (preventivi, task con report PDF, ecc.). Round-trip preservato dal save path esistente.
        - Server fn `ResolveAssetUrl(target, provider_id, ref_id) → Result<ResolvedUrl, ServerFnError>` thin wrapper su `BlobAdapter::resolve_url`, gated su `can_read`, audita `blob_url_issued`.
        - UI: sezione "External Assets" nel detail panel e nell'editor che legge il frontmatter, risolve gli URL in tempo reale via server fn, renderizza link diretti o — quando opportuno — iframe riusando l'allowlist di Embedded Analytics (no schema parallelo: il dominio dell'URL risolto deve passare la validazione `embed_allowed_origins` se renderizzato come iframe).
        - Success criterion: un documento con `external_assets: [...]` mostra gli allegati nel detail panel e nell'editor con URL freschi a ogni apertura; un asset con dominio non in allowlist viene reso come link aperto in nuova tab, non iframe; nessuna chiave di provider raggiunge il bundle frontend.

    - **γ Google Workspace provider** _(redirect-only, no upload)_
        - Estensione `BlobAdapter` con `GoogleWorkspaceAdapter`: `resolve_url` ritorna il link di redirect nativo Google Drive/Docs/Sheets per `ref_id` = file id; `upload` non implementato (Google Workspace non è uno store di blob owned, è un sistema esterno).
        - Schema config: `blob_providers: [{ id, type: google_workspace, ... }]`. L'utente referenzia file via `external_assets: [{ provider: "gworkspace", ref_id: "<drive-file-id>", name: "Q3 Plan" }]`.
        - Permessi: l'accesso al file Drive è governato dalle share policy di Google. Brain UI fornisce solo il link/iframe e non sincronizza nulla. Documentato come anti-goal esplicito.
        - Forma esatta del flusso auth (service account vs OAuth delegato) va disegnata alla luce di α/β in produzione e dei requisiti effettivi del team, non pre-pianificata.

    Esplicitamente fuori da BYOB (tutti gli slice):
    - Migrazione automatica dei vecchi asset in `assets/YYYY/MM/` verso i nuovi provider. I vecchi file restano serviti dal proxy Contents API esistente (caveat #16, già DONE 2026-04-29 — il proxy funziona, non c'è urgenza di migrare).
    - Sincronizzazione permessi tra Brain UI e Google Drive (vedi γ, è anti-goal).
    - Versioning custom dei blob (R2 ha le sue versioning policy, non duplicarle).
    - Pre-rendering/thumbnail di asset binari server-side — fuori scope, vive eventualmente in un servizio separato.

- [ ] **Full-Text Search (FTS5)**

    Razionale: i filtri per tag, tipo e cartella (3.7) coprono la navigazione strutturata, ma non il recupero per contenuto. Un utente che ricorda una frase specifica, un codice di errore, o un paragrafo non ha oggi nessun percorso diretto — deve aprire GitHub o fare `grep` locale. La projection SQLite è già il punto di aggancio naturale: FTS5 è un'estensione built-in di SQLite, zero dipendenze aggiuntive, indicizzazione incrementale sopra `nodes.body`/`files.content_path` + frontmatter.

    Vincoli:
    - L'indice FTS5 vive nella stessa projection SQLite per-target. La virtual table viene creata al rebuild e aggiornata in modo incrementale dalle stesse path di write che già aggiornano `nodes` e `files` — nessun secondo store, nessun daemon esterno.
    - La server fn `SearchBrain(target, query) → Vec<SearchHit { path, title, snippet, score }>` riusa il token OAuth dell'utente per il gating `can_read` e la query layer parametrica di 3.5. Snippet generati da FTS5 `snippet()` built-in — nessuna logica di highlighting custom nel bundle WASM.
    - La search bar vive nella Knowledge UI come ingresso globale, separato dal filter panel (che resta dimensionale). I due non sono in competizione: il filtro restringe lo spazio, la ricerca lo attraversa per testo. URL-persistita come `?q=` ortogonale a `?tags=`/`?types=`/`?path_prefix=`.
    - Scope iniziale: body markdown + titolo + tags del frontmatter. Fuori scope: search nei commenti issue bindati (livedata GitHub, non projection locale), ricerca cross-target, ranking semantico/vettoriale (Fase 5 se mai).
    - **Candidato naturale per l'endpoint MCP di Fase 5**: `SearchBrain` esposta come tool MCP-compliant è uno dei casi d'uso più ovvi per un AI assistant che interroga la knowledge base. La firma va disegnata coerentemente con la query layer di 3.5 fin da subito.
    - Success criterion: un utente digita una frase nella search bar e vede in <200ms una lista di nodi con snippet contestuale; i risultati sono filtrabili per tipo/tag via filter panel; l'URL `?q=` è condivisibile e sopravvive a refresh.

- [ ] **Advisory Edit Lock (ottimistico)**

    Razionale: la 3.3 gestisce i conflitti *dopo* che si verificano (direct-write vs PR fallback). Con più contributor che lavorano in parallelo sullo stesso repo, è preferibile segnalare *prima* che qualcuno stia già modificando un nodo, riducendo il numero di PR di conflitto che richiedono review manuale. L'obiettivo non è un lock bloccante (che creerebbe deadlock editoriali) ma un **advisory signal**: visibile, ignorabile, non vincolante.

    Vincoli:
    - Implementazione senza heartbeat e senza WebSocket: una tabella SQLite `editing_locks(target_id, content_path, user_login, opened_at, expires_at)` con TTL di 10 minuti. `LoadNodeForEdit` legge il lock corrente e lo restituisce insieme ai dati del nodo. Il lock viene acquisito al momento dell'apertura in edit mode e rilasciato esplicitamente al save/discard/navigazione; scade automaticamente dopo il TTL senza cleanup daemon.
    - La UI mostra un banner non bloccante nel pannello editor: **"Andrea sta modificando questo file — aperto 3 minuti fa"**. L'utente può ignorarlo e procedere; il comportamento di save/PR fallback rimane invariato (3.3 gestisce il caso di conflitto effettivo).
    - Fuori scope: presenza "live" con cursore in tempo reale, lista utenti online, WebSocket o SSE dedicato al presence — questo è Fase 5 se mai. L'advisory lock è intenzionalmente leggero e non deve evolvere verso infrastruttura di collaborazione real-time all'interno di questa slice.
    - Gating: il lock è visibile solo a utenti con `can_read`; acquisito solo da utenti con `can_write_default_branch || can_review_via_pr`. Un utente senza capability di scrittura non acquisisce il lock ma vede il banner se qualcun altro lo ha.
    - Success criterion: se Andrea apre `runbooks/foo.md` in edit mode, Matteo — aprendo lo stesso nodo entro i 10 minuti successivi — vede il banner "Andrea sta modificando questo file". Nessun comportamento bloccante; il salvataggio successivo di Matteo segue il percorso normale di 3.3.

### Future UX Backlog: post-dogfooding signals
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
    - **Sezione `Recently created/modified` come feed semantico** — derivata da `files.commit_author`/`commit_message`/`last_commit_at` già presenti nella projection. Non una lista di file: un feed in stile "cosa è successo mentre ero via" con avatar dell'autore, file toccato, messaggio commit sintetizzato e — quando FTS5 è disponibile — snippet del contenuto aggiunto/modificato estratto da `fts5_snippet()`. Risponde alla domanda editoriale "chi ha fatto cosa, su cosa, di recente" senza esporre SHA o `git log`.
    - **Ristrutturazione bulk admin-only** — UI per move-by-pattern (es. "tutti i `runbook` di Q1 in `runbooks/2026/q1/`"), riusa `BranchTransaction` di 4.0 per atomicità + preview via `transaction.plan()`. Solo dopo che 4.0 è chiusa.
    - **Path collision detection in edit mode** — durante rename/move, evidenziare in tempo reale se il path target collide con un file esistente, è soggetto a case-insensitive collision (rilevante su filesystem case-folding), o supera limiti di profondità ragionevoli. Reuse parziale della logica `expect_absent` di `GitTransaction`.
    - **Dead link discovery** — la projection ha già `edges` con `source_path` e `target_path`; un edge verso un path non presente in `files.content_path` è un link rotto. Al rebuild, materializzare `broken_edges` come view o colonna flag. Superficie admin: vista diagnostica "Broken Links" con lista `source → target (missing)` e CTA diretta all'editor del nodo sorgente. Complementare al banner orphan di 3.7 (che copre file senza link *entranti*); questo copre link che puntano *a niente*. Da implementare come estensione del rebuild della projection, non come scansione separata.
    - Vincolo trasversale: ognuna di queste evoluzioni deve restare opt-in e non deve trasformare Brain UI in un file manager. Se l'utente vuole un file manager, ha GitHub.
- [ ] **Activity Stream, Co-authorship e Watch/Follow** _(micro-consapevolezza asincrona, post-dogfooding)_
    - **Activity Stream per nodo** — tab "Activity" nel detail panel che fonde due stream già disponibili: (1) eventi Brain dall'audit log (`work_item_transition`, `assign`, `bind`, `propose_write`, ecc.) — semantici, già pronti; (2) commit metadata da `files.commit_author`/`commit_message`/`last_commit_at` nella projection — storia editoriale del file. Il risultato è una timeline leggibile: "Andrea ha aggiornato lo stato a Done (2 ore fa)", "Matteo ha modificato il contenuto (ieri)", "Sistema ha bindato la GitHub Issue #42 (3 giorni fa)". Git rimane invisibile: l'utente percepisce un flusso di attività editoriale, non una lista di SHA.
    - **Co-authorship visibile (face-pile)** — sotto il titolo del nodo nell'editor, avatar sovrapposti degli autori che hanno contribuito al file, derivati da `commit_author` nella projection con cache `node_authors(target_id, content_path, login, avatar_url)` riempita al rebuild. Risponde a "chi è il domain expert di questo documento?" senza richiedere chiamate GitHub live per ogni apertura — solo al rebuild del file.
    - **Watch/Follow con notifiche in Task Inbox** — prerequisito: Task Inbox (già in backlog). Modello `watches(user_login, target_id, content_path_or_prefix, created_at)` in SQLite. Quando `WorkItemUpdated` o `GraphUpdated` arriva via SSE per un path/prefix seguito, l'evento viene inserito nella Task Inbox dell'utente come notifica contestuale. Il fan-out SSE→watch avviene server-side senza nuova infrastruttura: riusa il broadcast bus già esistente. L'utente viene avvisato solo per i sotto-domini di sua competenza, senza polling manuale.
    - Vincolo trasversale: tutti e tre i sotto-feature si basano sugli stessi dati di commit history e audit log già materializzati — nessun nuovo store, nessuna chiamata GitHub live fuori dal rebuild. La presentazione nasconde Git; la meccanica lo riusa.
    - Dipendenza: Watch/Follow richiede Task Inbox come container delle notifiche. Activity Stream e face-pile sono indipendenti.

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

- [ ] **4.0-B Edit Session (Staged Commit)**

    Razionale: oggi ogni azione utente (salva nodo A, rinomina B, aggiorna tag C) produce un commit distinto — oppure, per utenti senza write access diretto, una PR per azione. In un workflow editoriale reale questo inquina la history e spezza la semantica collaborativa: i reviewer ricevono N PR atomiche invece di una che rappresenta l'intento dell'autore. Il Level 1 (atomicità per singola azione multi-file) è già in produzione tramite `GitTransaction`. Questa voce copre il Level 2: raggruppare N azioni distinte di una sessione in un solo commit o PR.

    - **Draft state** — durante la sessione, le modifiche pendenti si accumulano in un buffer lato client. Ogni `save`, `delete`, `rename` aggiunge una `GitTransaction` alla coda invece di committare immediatamente. Il buffer è visibile all'utente come badge "N modifiche in sospeso" nella toolbar. Valutare se il buffer sopravvive al reload (localStorage vs SQLite `edit_session` server-side): la variante server-side è necessaria solo se emerge il caso d'uso "riprendo la sessione dopo un giorno".
    - **Commit session** — l'utente invoca esplicitamente "Commit tutto" o "Proponi modifiche" (per utenti PR fallback). Il runtime serializza le `GitTransaction` pendenti, le compatta in una `BranchTransaction` (4.0) e committa in un solo round trip verso il forge. Il messaggio di commit è scelto esplicitamente dall'utente — non l'aggregato automatico dei messaggi delle singole azioni. `transaction.plan()` viene chiamato prima del commit per mostrare un preview e rilevare precondition failure (file rimosso da un teammate nel frattempo).
    - **Interazione con permessi e PR:**
        - *Write access diretto*: commit session → 1 commit sul branch corrente (o su un branch di sessione dedicato, da valutare in base al feedback Fase 3).
        - *PR fallback (read-only / fork)*: commit session → 1 branch effimero → 1 PR che raccoglie tutte le modifiche della sessione. Oggi ogni salvataggio produce una PR separata — questo è il caso peggiore per i reviewer ed è il trigger principale di questa voce.
        - Il controllo permission avviene al momento del "Commit session", non durante l'accumulo: l'utente edita liberamente, il gate è solo all'uscita. Coerente con il modello `write_orchestrator` esistente.
    - **Conflitti durante accumulo** — se un webhook notifica che il branch è cambiato mentre ci sono modifiche pendenti, il banner `Stale Data` esistente si estende: invece di "reload perdendo tutto", propone "review conflict prima di committare". Il merge semantico completo (4.4) chiude questo in modo pieno; questa voce si limita a non perdere silenziosamente il draft.
    - **Vincoli:** draft TTL esplicita (sessione browser o max 24h) — non può diventare un second source of truth persistente. Non introduce advisory lock o presenza real-time ("l'utente X sta editando questo nodo") — problema separato di collaborazione sincrona.
    - **Prerequisito bloccante:** 4.0 (`BranchTransaction` + `plan()`). Non anticipabile prima. Non blocca 4.1.

- [ ] **4.1 `ForgeAdapter` Trait (capability-driven)**
    - Estrarre un boundary che copra i bisogni reali del runtime, non un semplice wrapper HTTP. Le capability minime previste sono: target discovery/listing, lettura tree/blob, write commit/branch/ref update (sopra le primitive `GitTransaction` / `BranchTransaction` chiuse in 4.0), PR/MR creation, work item issue mutation, webhook verification + event normalization.
    - Valutare esplicitamente se implementarlo come un singolo trait o come famiglia di subtrait/capabilities (`ForgeRepoAdapter`, `ForgeTransactionAdapter`, `ForgeCollaborationAdapter`, `ForgeWebhookAdapter`) per evitare un "lowest common denominator" troppo povero o un god-trait ingestibile. La separazione `ForgeTransactionAdapter` è naturale: GitHub Git Data API, GitLab Repository Files API e Gitea hanno semantica simile ma endpoint diversi, e la transazione con preconditions resta utile a tutti.
    - Estrazione del crate `git-tx-<forge>` (o nome migliore): avviene **qui**, non in 4.0. A questo punto esistono due adapter reali che validano la forma del trait, un error type dedicato (`GitTxError` / `ForgeError`) e un client trait minimo che sostituisce la dipendenza concreta su `GithubClient`. La crate resta interna al workspace finché la superficie API non è stabile in produzione per ≥3 mesi su due adapter; solo allora valutare pubblicazione su crates.io.
    - Adapter secondari: **GitLab** e **Gitea/Forgejo**. GitHub resta reference implementation finché i casi reali non stabilizzano la forma finale.
- [ ] **4.2 Temporal Graph View (Git Time Jump)**
    - La feature non deve limitarsi a mostrare vecchi file. L'obiettivo è una modalità storica completa con slider/timeline che ricostruisce una projection temporanea del repository a una data/SHA e la rende navigabile nella stessa UI a grafo.
    - Reuse intenzionale della pipeline esistente: fetch tree storico → build in-memory/ephemeral SQLite projection → render di graph canvas, detail panel e knowledge base in modalità read-only storica.
    - Estensione naturale: confronto `then vs now` per nodi, backlink e work item bindati, senza introdurre un parser o store storico separato.
    - **Layout host già pronto (vedi 3.7G):** il bottom slot della sidebar di `KnowledgePage` è riservato a una history graph view stile VS Code source-control. Attivazione: flippare `HISTORY_SLOT_ENABLED` in `crates/brain-app/src/knowledge/filter_panel.rs` e popolare il contenuto. A quel punto valutare un draggable splitter tra filter pane e history slot (rinviato apposta da 3.7G per calibrare il rapporto su un componente reale, non su una previsione).
- [ ] **4.3 Local / Offline Execution Context**
    - Permettere l'esecuzione di Brain UI contro un `.git` locale o una working tree locale senza dipendere da Axum come proxy di un forge remoto.
    - Implementare un `LocalFileSystemAdapter`/`LocalGitAdapter` che offra la stessa superficie minima usata dal runtime: lettura snapshot, commit locali, branch locali, eventuale sync successivo verso remoto opzionale.
    - Evitare fork architetturali: stessa UI, stesso projection pipeline, diverso adapter.
    - **Segnale di estrazione crate per `server/projection`** — quando 4.3 introduce `LocalGitAdapter`, `server/projection.rs` diventa consumato da due runtime distinti (forge remoto e filesystem locale). Quel momento è il trigger naturale per estrarre `brain-projection` come crate separato, non prima. Prima di allora il guadagno è zero e il costo è un confine di crate che vincola la firma delle query prima che emerga un secondo consumer reale.
    - **Projection modularization prep** _(safe to do before 4.3, mechanical only,**02/05/2026 DONE**)_:
        - [x] Convertire `crates/brain-app/src/server/projection.rs` in `projection/mod.rs` mantenendo invariato il public API usato da `api/*`, `webhook`, `routing`, `target_registry` e `main`.
        - [x] Estrarre `migrations.rs` per `migrate` + helper schema, senza introdurre ancora versioned migrations.
        - [x] Estrarre `target.rs` / `sync_state.rs` per `ensure_target_id`, `pool_handle` consumers e watermark/error state.
        - [x] Estrarre `rebuild.rs` per `rebuild`, `ProjectionSnapshot`, backlink derivation e snapshot persistence orchestration.
        - [x] Estrarre `bulk_insert.rs` per gli insert chunked SQLite e il limite `SQLITE_MAX_VARIABLES`.
        - [x] Estrarre query modules `nodes.rs`, `files.rs`, `work_items.rs` con filters, row mapping e single-row work item mutations.
        - [x] Lasciare fuori scope crate split, trait projection/repository, schema v2, FTS e behavioral changes; ogni commit deve essere verificabile come refactor-only.
        - [x] Verifica minima per chiudere la prep: `cargo check -p brain-app --features ssr`, `cargo check -p brain-app --features hydrate`, `cargo test -p brain-app --features ssr`.
- [ ] **4.4 Advanced Conflict Resolution**
    - La Fase 2B ha introdotto il banner `Stale Data`; con collaborazione reale e fallback via PR non basta più.
    - Quando webhook o ref update rivelano divergenze rispetto al draft locale o al branch corrente, servono diff e merge espliciti: vista side-by-side, scelta hunk-based o almeno `local / remote / apply anyway`.
    - Riuso esplicito del `transaction.plan()` di 4.0: il preview hunk-based non è una funzione separata, è il dry-run della transazione che evidenzia le precondition fallite e mostra cosa cambia per ciascun path coinvolto.
    - Questo asse è particolarmente importante se la 3.3 porta davvero contributor multipli e branch temporanei: il conflitto non sarà più eccezione, ma percorso operativo ordinario.
    - **Vincolo di presentazione — visual diff semantico:** il diff tecnico (`+`/`-` hunk-based) è corretto per lo sviluppatore ma ostile per l'editor testuale. La presentazione deve usare un diff word-level o sentence-level renderizzato come HTML: testo barrato+rosso per le rimozioni, evidenziato+verde per le aggiunte, inline nel Markdown renderizzato. Crate candidato: `similar` o `dissimilar` (entrambi disponibili in Rust, ~20KB, nessuna dipendenza JS). Questo è il livello che trasforma 4.4 da "feature tecnica" a "feature usabile da chiunque scriva documenti".
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
    - **Segnale di estrazione crate per `knowledge/` UI** — quando l'endpoint MCP consuma componenti Leptos in un contesto headless o in un secondo binary (es. CLI con output strutturato), i componenti `knowledge/graph_canvas.rs`, `knowledge/editor.rs`, `knowledge/detail_panel.rs` diventano candidati a un crate `brain-ui-components` separato. Prima di allora il confine di crate non ha un secondo consumer che ne giustifichi il costo. In preparazione: spezzare i file più grandi (`editor.rs` 1200+ righe, `detail_panel.rs` 1180+ righe) in sottomoduli interni per cartella — stessa logica di `projection` sopra.
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

14. ~~`Stale Data` banner can flash before login~~ — **DONE 2026-05-02**. `LiveSync` now opens `/sse/events` only on protected workspace routes (`/knowledge`, `/admin`, and multi-tenant target routes). Public landing visits no longer trigger the auth-gated SSE endpoint, so anonymous deploy visitors do not see a stale-data banner caused by the expected `401`. The landing page also now states that OAuth works but is still a raw access surface that needs product evaluation.

15. **No auto-binding to external issues/trackers** — Il binding di un `WorkItem` verso un'issue del forge è oggi interamente manuale: l'utente apre il binding form e compila `system / project / item_key / url` a mano (vedi `BindWorkItem` server fn e `DetailPanel::WorkItemCard`). Non esiste un'azione `Create issue from this work item` che apra l'issue su GitHub e popoli `external_binding` in un singolo passaggio, né auto-discovery di issue esistenti che già referenzino il `brain_id` o il `content_path`. Conseguenza: `system_of_record = split | external` richiede che l'issue esista già fuori da Brain UI. La soluzione naturale vive sopra il `ForgeAdapter` di 4.1 ed è tracciata come follow-up in **4.5 Auto-binding work item ↔ issue/tracker esterni**. Workaround attuale: creare l'issue su GitHub, copiare numero/URL, bindare manualmente.

16. ~~Asset proxy 404 silenzioso su repo privati~~ — **DONE 2026-04-29**. `serve_asset` in [crates/brain-app/src/server/assets.rs](../crates/brain-app/src/server/assets.rs) interrogava `raw.githubusercontent.com` con `Authorization: Bearer <token>`, ma quell'host **non accetta** bearer auth su repo privati (richiede `?token=` query param legacy o Contents API). Ritornava 404 silenzioso. Sintomo concreto: il primo asset prodotto dal Brain Clipper (`assets/2026/04/schermata-del-2026-04-29-…png`) era committato correttamente su `main` (Contents API: 200, sha matching, size 515850) ma non si vedeva in Brain UI. **Fix**: il proxy ora chiama la Contents API con `Accept: application/vnd.github.raw`, che restituisce i bytes raw direttamente con la stessa autenticazione bearer di tutte le altre chiamate. Verificato end-to-end con probe curl: 200, content-type corretto, file scaricato byte-identical al locale. Aggiungere test wiremock-based che fallisca se qualcuno reintroduce `gh.raw_base()` nel proxy.

17. ~~Node hover flicker on graph canvas~~ — **DONE 2026-04-30**. Diagnosi confermata lato geometria SVG: l'hit-area del `<g>` era l'unione dei figli pitturati, quindi il `<circle>` visibile che transiva `r = base_r + bump` cambiava anche il target effettivo del puntatore. Fix in [graph_canvas.rs](../crates/brain-app/src/knowledge/graph_canvas.rs): il cerchio visibile ora ha `pointer-events="none"` e resta libero di animare; un secondo `<circle>` trasparente, stabile, con `r = base_r + selected_bump + buffer` riceve hover/click via bubbling sul gruppo. Aggiunto test `node_hit_radius_covers_all_visual_states`.

18. **Graph canvas DOM scalability** — Il canvas SVG renderizza ogni nodo e arco come elemento DOM distinto gestito dalla reattività Leptos. Fino a ~300–500 nodi simultaneamente visibili le performance sono accettabili; oltre quella soglia — su repo reali con 1000+ nodi senza filtro attivo — il layout force-directed e i re-render reattivi possono introdurre frame drop percepibile. **Threshold di intervento:** se il dogfooding su repo reali con filtro rimosso mostra jank a >500 nodi, la risposta è virtualizzazione del viewport (render solo dei nodi dentro il viewBox corrente + buffer) prima di considerare Canvas 2D API. WebGL è overkill per un grafo 2D statico e richiede interop JS non banale da WASM. Da rivalutare dopo 4.2 (Temporal Graph View), che introdurrà un secondo render path e renderà più chiaro il vero collo di bottiglia.

19. **Content trust boundary before embeds/blob/AI** — **PARTIAL 2026-05-01**. Brain UI usa `inner_html` per Markdown renderizzato e commenti issue: corretto per preservare formattazione, ma va trattato come trust boundary esplicito prima di iframe, BYOB e AI-generated content. Tracciato nella **Next Hardening Lane / Security & Content Trust Baseline**. Micro-fix già applicata: i nuovi upload `.svg` non sono più accettati da `UploadAsset` finché non esiste sanitizer/policy dedicata; gli SVG storici restano serviti dal proxy come asset legacy.

20. ~~Target identity still has ambient edges~~ — **DONE 2026-05-01**. Chiuso con **3.7B Canonical TargetRef & Trust Boundary Preflight**: branch nel path canonico, `TargetRef { org, repo, branch }` esplicito nelle server fn target-scoped principali, branch/default branch persistiti nel registry, webhook risolti dal payload reale e SSE filtrata per target.
