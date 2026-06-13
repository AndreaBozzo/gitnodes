# Brain UI Roadmap

> Closed phases live in [ROADMAP_ARCHIVE.md](ROADMAP_ARCHIVE.md). This file tracks current + future work only. When a phase or hardening item closes, its detailed rationale moves to the archive in the same PR that flips the last checkbox.

Questo documento delinea l'evoluzione di **Brain UI** da un visualizzatore di grafi Markdown a un CMS distribuito, collaborativo, platform-agnostic e potenziato dall'Intelligenza Artificiale.

**Stato architetturale corrente:** Git e il repo target restano la *Single Source of Truth*. SQLite copre sessioni, audit log, target registry, outbox provider-sync e una projection locale target-scoped per `nodes`, `edges`, `files`, `backlinks`, `work_items` e `work_item_bindings`, con rebuild esplicito e watermark/error state. Il core collaborativo è chiuso: routing multi-target canonico via `TargetRef`, mutazioni work item bidirezionali, PR fallback permission-aware, webhook/SSE per-target, health/readiness, errori API tipati e retry outbox sono shippati. Il focus si sposta su open-sourcing del core e Fase 4 (forge independence + deep history) con le insight estratte da [INSIGHTS_OMNIGRAPH_COMPARISON.md](INSIGHTS_OMNIGRAPH_COMPARISON.md) come hardening lane.

---

## ✅ Closed phases

- **Fase 1: Astrazione e Configurabilità** _(closed 2026-04-24)_ — config loader + default migration, `NodeType` → String, `GithubClient` URL-only, `WorkItem` model + label taxonomy. [Archive](ROADMAP_ARCHIVE.md#-fase-1-astrazione-e-configurabilità-closed-2026-04-24).
- **Fase 2: Operatività, Sync e Dati** _(closed 2026-04-25)_ — 2A hardening (cache scoping, pooling, baseline refresh) + 2B projection SQLite multi-tenant, rebuild/reconciliation, webhook+SSE baseline, rename atomici via Git Data API, fondazione operativa Work Item. [Archive](ROADMAP_ARCHIVE.md#-fase-2-operatività-sync-e-dati-closed-2026-04-25).
- **Fase 3: Workspace Collaborativo, Work Item & RBAC** _(scope freeze 2026-05-02; dogfooding gate)_ — 3.1 multi-tenant routing, 3.2 bidirectional work-item sync, 3.3 RBAC + write orchestrator permission-aware, 3.4-α Saved Views + visual config baseline, 3.5 parametric query layer, 3.6 canvas polish, 3.7 repo structure transparency, 3.7B canonical `TargetRef`, 3.7F/G frontend baseline + sidebar real-estate posture. [Archive](ROADMAP_ARCHIVE.md#-fase-3-workspace-collaborativo-work-item--rbac-closed-2026-05-02-scope-freeze-dogfooding-gate).
- **Next Hardening Lane (chiusa 2026-05-23):** Security & Content Trust Baseline, Failure-Mode Matrix & Operational Readiness (α/β/γ/δ + matrix doc), Projection Schema v2 & SQLite Operations, Presentation UI Polish Pass. [Archive](ROADMAP_ARCHIVE.md#-next-hardening-lane--closed-items-through-2026-05-23).

### Carryover dai phase chiusi (open)

- [ ] **Brain mock con contributor limitato** _(assegnato a un tester esterno, 2026-04-28)_ — sandbox isolata operativa. Prima iterazione **Git/GitHub-first** (vincoli autorizzativi temporanei della configurazione multi-tenant); il contributor lavora su un branch QA dedicato, PR review su main. Secondo passaggio Brain UI-first quando i permessi non falsano più il test.
- [x] **Release note / operator checklist Fase 3** _(DONE 2026-06-12)_ — shippato come [docs/OPERATOR_NOTES.md](OPERATOR_NOTES.md): cosa è shippato, caveat accettati (con rimando ai numeri), checklist env/config di deploy e tabella sintomo→azione condensata dalla failure-mode matrix. Success criterion coperto: leggibile in 10 minuti, nessun riferimento al codice necessario per il recovery.
- [x] **BrainSwitcher revalidate cache** _(follow-up Presentation Polish 2026-05-23; DONE 2026-05-25)_ — cache client-side della lista target con refresh esplicito; `ListAccessibleTargets` non riparte ad ogni semplice riapertura del menu.

### Hardening lane insight da Omnigraph (open, S-sized)

Estratti da [INSIGHTS_OMNIGRAPH_COMPARISON.md](INSIGHTS_OMNIGRAPH_COMPARISON.md); shippabili insieme in una sola PR di follow-up Schema v2.

- [x] **Content-hash signatures per drift detection** _(insight 3.1; DONE 2026-05-25)_ — aggiunti `blob_sha` a `files`/`nodes`, popolati in rebuild e loggato changed-set size (`added`/`changed`/`deleted`) per rebuild. Schema v2 aveva già `body_text`/`frontmatter_json` riservati: questa è la colonna mancante che rende il diff Git-tree ↔ projection un'operazione O(changed). Precondizione per qualunque incremental rebuild futuro (6.1).
- [x] **Typed `ConflictKind` enum** _(insight 3.2 — prerequisito 4.4; DONE 2026-05-25)_ — `BrainError::Conflict` ora conserva enum tipato (`PathTaken`, `BlobShaMoved`, `RefNonFastForward`, `RemotePathDeletedUnderUs`) e `ApiError::Conflict` lo porta fino al boundary UI. `git_transaction.rs` tagga i casi che prima venivano flattenati.

### Hardening lane: review full-stack 2026-05-29 (open)

Findings da una review architetturale dell'intero workspace. Gli item ad alto impatto sono stati risolti nella stessa sessione — #1 CSP locked-down sulle risposte dell'asset proxy (SVG same-origin non esegue più inline script), #2 cleanup dell'error-handling morto in `installation_token`, #3 jitter di retry decorrelato in `git_transaction`, #4 fetch concorrente in `fetch_raw_files`. Questi due restano tracciati come follow-up minori, non bloccanti.

- [x] **Scope del MutationObserver Mermaid** _(review 2026-05-29, #5; XS, frontend perf; DONE 2026-06-12)_ — implementata la seconda opzione: `renderBrainMermaid` ora apre con un singolo probe `querySelector` ("esistono blocchi mermaid non processati?") e ritorna subito se vuoto, invece di eseguire l'intera pipeline di transform su ogni mutazione DOM. Shippato insieme al lazy-load α del Mermaid load strategy (stesso blocco di codice in `app.rs`).
- [ ] **Scope del token OAuth utente (`repo`)** _(review 2026-05-29, #6; security baseline)_ — il flow OAuth in `brain-auth` richiede `scope=repo+read:org`: `repo` concede R/W completo su *tutti* i repo privati dell'utente, non solo il target (limite intrinseco degli OAuth App, non risolvibile per-repo). Il token è cifrato a riposo (`with_private`) e usato solo server-side; la **GitHub App** (`installation_token.rs`, già in tree) è la remediation propriamente scoped. Tracciare il ritiro del path di scrittura OAuth una volta completato il rollout App. Allineato alla Security & Content Trust Baseline (§ Open-Sourcing del Core) e al boundary forge di Fase 4.

---

## 🚀 Open-Sourcing del Core (target Luglio 2026)

Razionale: rilasciare il core dell'app come repository pubblica pulita, mantenendo l'istantanea attuale come deploy downstream per Dritara. La Security & Content Trust Baseline è il prerequisito di maturità: i trust boundary devono essere chiusi prima di esporre il codice. La separazione multi-tenant (`TargetRef`, Fase 3.7B) ha già slegato grafo, sessioni SQLite e canali SSE da un singolo repo d'ambiente, quindi il disaccoppiamento logico è in gran parte fatto; resta la bonifica di config, dati operativi e tracciamento interno. _(Spunto: review architetturale esterna 2026-05-20.)_

- [ ] **Disaccoppiamento sorgente da infrastruttura proprietaria**
    - Verificare che le env di fallback al boot statico (`TARGET_GITHUB_ORG`, `TARGET_GITHUB_REPO`, e simili) agiscano solo come default esemplificativi e non incorporino riferimenti a dati di produzione privati.
    - `.brain-config.yml` nella repo pubblica deve essere un blueprint agnostico (es. una tassonomia di prova generica sul modello della sandbox Pokemon), senza la mappatura operativa o i flussi interni Dritara.
    - Confermare che nessun crate (`brain-app`, `brain-domain`, `brain-storage`, `brain-graph`, `brain-auth`) contenga costanti/segreti accoppiati all'infrastruttura. Mantenere l'invariante del test di auto-registrazione delle server fn (guard contro lo strip LTO in release).

- [x] **Modalità org-less (account personale)** _(abilitatore di adozione OSS; DONE 2026-06-06)_

    Razionale: una Brain UI open-source che *richiede* una GitHub org è poco adottabile — chi la valuta la punta su un repo personale. Oggi il blocco è il login gate (`is_org_member(required_org(), login)` in `server/auth.rs`): per un owner personale `GET /orgs/{utente}/members/...` ritorna 404 → login negato. Quasi tutto il resto **già** funziona per repo personali: il campo `org` di `TargetConfig` è in realtà l'owner (API GitHub owner-agnostica), la discovery usa già `affiliation=owner`, e l'autorizzazione reale passa ovunque da `repository_permissions` (pull/push/admin), che GitHub calcola correttamente sui repo personali.

    - **Login gate**: `GITHUB_LOGIN_ORG` separa la policy di login dall'owner del target. Le env split storiche mantengono il fallback a `TARGET_GITHUB_ORG`; il nuovo `TARGET_GITHUB_REPOSITORY` parte org-less; un valore esplicito sceglie un'org allowlist.
    - **Target gate**: i read path target-scoped su projection SQLite, config cache e SSE verificano `permissions.pull` con cache breve (15s). L'asset proxy usa direttamente il Contents API autenticato come gate equivalente, evitando un preflight per immagine. Le write mantengono direct-vs-PR permission-aware; gli upload diretti richiedono esplicitamente `push`.
    - **Admin gate** (`require_target_admin_session`): richiede solo `permissions.admin || maintain` sul target, senza membership org ridondante. Le superfici operator globali restano temporaneamente dietro questo gate per backward compatibility; lo split deployment-admin è la slice OSS successiva.
    - **Caveat GitHub App**: il path installation-token (sync inbound via webhook) va verificato per account personali (le App si installano anche su utenti; resta il fallback PAT/OAuth). Non bloccante, da confermare a parte.
    - Success criterion raggiunto: con `GITHUB_LOGIN_ORG=` un utente senza org può loggare e usare un repo personale secondo `repository_permissions`; un utente autenticato senza `pull` non può leggere dati live o proiettati del target.

- [x] **Bootstrap runtime minimo** _(adozione OSS; DONE 2026-06-06)_

    - Il percorso raccomandato richiede solo credenziali OAuth e `TARGET_GITHUB_REPOSITORY=owner/repo`; branch (`main`) e branding hanno default generici.
    - La chiave di cifratura sessione viene generata con permessi privati in `data/session.key` e persiste sullo stesso volume SQLite; `SESSION_ENCRYPTION_KEY` resta disponibile per secret management esterno.
    - Il webhook non blocca più il boot quando non configurato: in release resta disabilitato finché non viene fornito `WEBHOOK_SECRET`.
    - Le env split storiche restano compatibili e mantengono il precedente fallback di membership org; il nuovo locator compatto parte org-less.

- [x] **Bonifica della roadmap pubblica** _(DONE 2026-06-12)_
    - Espunti da ROADMAP, archive e README i riferimenti a repo sandbox privati, ai tag degli account dei tester chiusi e i link a runbook rimossi; il carryover dogfooding parla ora di "sandbox isolata" e "branch QA dedicato".
    - Decisione: **un solo file, già anonimizzato** — nessun ROADMAP interno separato da mantenere in sync.
    - Residuo tracciato sotto "Disaccoppiamento": i fixture dei test usano ancora l'org reale come dummy (`markdown.rs`, `assets.rs`, `main.rs`, `draft.rs`); rename meccanico da fare con quell'item.

- [x] **Supply chain & licenza per il rilascio pubblico** _(licenza DONE 2026-06-12; CONTRIBUTING/security policy rimandati a ridosso del rilascio)_
    - Mantenere `.cargo/audit.toml` + il workflow `rustsec/audit-check` in CI così com'è: gli ignore `RUSTSEC-2023-0071` (rsa via sqlx-mysql, assente dal binario sqlite) e `RUSTSEC-2024-0436` (paste via Leptos) sono documentati e circoscritti — buon biglietto da visita per utenti esterni.
    - Licenza scelta e applicata: **Apache-2.0** (coerente con gli altri tool Rust pubblicati). `LICENSE` al root, `license.workspace` in tutti e cinque i crate, sezione License del README allineata. `CONTRIBUTING.md` e security policy restano da scrivere al momento del rilascio, quando il flusso contributor è definito.

- [ ] **Strategia di sdoppiamento repo (upstream pubblico / downstream Dritara)**

    Approccio raccomandato: **un repo pubblico (core) + un mirror privato Dritara** che contiene lo stesso codice più gli override di config/secret (`.brain-config.yml` reale, `SESSION_ENCRYPTION_KEY`, env Railway, audit log persistiti sul volume SQLite), sincronizzato via `git remote` + merge. Le patch di sicurezza upstream si propagano con un merge, senza bump di versione né allineamenti manuali.

    - Alternative valutate e **scartate**: Git submodule e crate su registry. Per un workspace Leptos full-stack (con `bin-features`, profili WASM, `package.metadata.leptos`) il submodule rende il deploy Railway fragile — il root del workspace dovrebbe essere il submodule — e il registry impone un ciclo publish/bump a ogni patch. Il mirror+merge è più semplice e robusto per questo layout. _(Meccanismo finale da confermare al momento del rilascio.)_
    - Success criterion: una patch di sicurezza applicata a monte arriva al deploy di produzione con un singolo merge, e nessun segreto/dato operativo Dritara vive nella repo pubblica.

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

    Razionale: Git è la Single Source of Truth perfetta per semantica, testo e metadati, ma è storicamente pessimo per i file binari. Oggi l'editor di Brain UI committa le immagini in `assets/YYYY/MM/`: questo gonfia il repository e — soprattutto — rompe l'esperienza della Fase 3.3: un asset caricato in un branch temporaneo via PR non è immediatamente referenziabile dal markdown live, quindi la preview del draft è rotta finché la PR non viene mergiata. In parallelo, il team ha bisogno di integrare nativamente documenti e spreadsheet ospitati su Google Workspace senza passarli per Git.

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
    - Migrazione automatica dei vecchi asset in `assets/YYYY/MM/` verso i nuovi provider. I vecchi file restano serviti dal proxy Contents API esistente.
    - Sincronizzazione permessi tra Brain UI e Google Drive (vedi γ, è anti-goal).
    - Versioning custom dei blob (R2 ha le sue versioning policy, non duplicarle).
    - Pre-rendering/thumbnail di asset binari server-side — fuori scope, vive eventualmente in un servizio separato.

- [x] **Full-Text Search (FTS5) con RRF fusion** _(DONE 2026-05-26)_

    Razionale: i filtri per tag, tipo e cartella (3.7) coprono la navigazione strutturata, ma non il recupero per contenuto. Un utente che ricorda una frase specifica, un codice di errore, o un paragrafo non ha oggi nessun percorso diretto — deve aprire GitHub o fare `grep` locale. La projection SQLite è già il punto di aggancio naturale: FTS5 è un'estensione built-in di SQLite, zero dipendenze aggiuntive, indicizzazione incrementale sopra `nodes.body`/`files.content_path` + frontmatter.

    Vincoli:
    - L'indice FTS5 vive nella stessa projection SQLite per-target. La virtual table viene creata al rebuild e aggiornata in modo incrementale dalle stesse path di write che già aggiornano `nodes` e `files` — nessun secondo store, nessun daemon esterno.
    - La server fn `SearchBrain(target, query) → Vec<SearchHit { path, title, snippet, score }>` riusa il token OAuth dell'utente per il gating `can_read` e la query layer parametrica di 3.5. Snippet generati da FTS5 `snippet()` built-in — nessuna logica di highlighting custom nel bundle WASM.
    - La search bar vive nella Knowledge UI come ingresso globale, separato dal filter panel (che resta dimensionale). I due non sono in competizione: il filtro restringe lo spazio, la ricerca lo attraversa per testo. URL-persistita come `?q=` ortogonale a `?tags=`/`?types=`/`?path_prefix=`.
    - Scope iniziale: body markdown + titolo + tags del frontmatter. Fuori scope: search nei commenti issue bindati (livedata GitHub, non projection locale), ricerca cross-target, ranking semantico/vettoriale (Fase 6.6 se mai).
    - **Ranking via RRF fusion** _(insight 3.4 da Omnigraph comparison)_: scoring per **Reciprocal Rank Fusion (k=60)** di due ranker cheap — FTS5/BM25 su `body_text` + structured-filter overlap (tag/type/path-prefix match count). RRF richiede solo liste ordinate, non modelli; ~15 righe di fusione. Prototipare la funzione di fusione su liste sintetiche prima dello schema FTS5 per lockare il contratto. Nessun vector backend.
    - **Candidato naturale per l'endpoint MCP di Fase 5**: `SearchBrain` esposta come tool MCP-compliant è uno dei casi d'uso più ovvi per un AI assistant che interroga la knowledge base. La firma va disegnata coerentemente con la query layer di 3.5 fin da subito.
    - Success criterion: un utente digita una frase nella search bar e vede in <200ms una lista di nodi con snippet contestuale; i risultati sono filtrabili per tipo/tag via filter panel; l'URL `?q=` è condivisibile e sopravvive a refresh.

- [x] **Graph edges tipizzati e layout config-driven** _(DONE 2026-05-27)_

    Razionale: i Brain custom con molti `node_types` (es. mock Pokémon) espongono relazioni strutturate in frontmatter che il grafo body-link non vedeva. Il risultato era visivamente piatto: cluster hardcoded per i tipi storici e archi indistinguibili tra citazioni narrative, relazioni geografiche, ownership, evoluzioni e tag.

    Implementazione:
    - `Edge` acquisisce `kind: EdgeKind` (`Body`, `Frontmatter(field)`, `Tag`) e la projection SQLite conserva il kind con una migration su `edges`.
    - `NodeTypeSpec.link_fields` dichiara i campi YAML che contengono slug verso un tipo target; il graph builder risolve `slug -> directory/slug.md` e materializza edge tipizzati senza introdurre store o daemon aggiuntivi.
    - `layout` deriva i cluster dai `node_types` del `BrainConfig` invece che da una lista hardcoded, distribuendoli su cerchio e aggiungendo una lieve gravità intra-cluster durante il force-directed pass.
    - Il canvas stila gli archi per kind e aggiunge una legenda/toggle in basso-sinistra; il budget label overview sale a 25-30 grazie alla separazione spaziale dei cluster.

    Vincoli mantenuti: config backward-compatible (`link_fields` assente = nessun link frontmatter), body link Markdown invariati, tag virtuali ancora derivati dalla soglia di condivisione esistente.

### Future UX Backlog: post-dogfooding signals
- [ ] **Task Inbox e segnali operativi sui nodi** _(follow-up UX emerso dal dogfooding, 2026-04-29)_
    - Aggiungere una sezione o pannello "My Tasks" nella Knowledge UI che usa il read model già esistente (`list_work_items`) per mostrare le task assegnate all'utente corrente GitHub, escludendo di default `done` e `cancelled`.
    - MVP: lista compatta con titolo, stato, assignee, path/binding GitHub e click-to-focus sul nodo/file. Deve riusare `WorkItem.assignees`, `WorkItem.state`, `content_path` ed eventuale `external_binding`, senza introdurre un secondo modello task.
    - Evoluzione: tab/filtri `Mine`, `Open`, `Blocked`, `Unassigned`, `Done`, quick action per transizioni di stato e assignee update riusando `TransitionWorkItem` / `AssignWorkItem`.
    - Aggiungere segnali visivi direttamente sul graph canvas: piccoli badge/ring/banner sui nodi work item per stato interno (`blocked`, `in-progress`, `done`), assignee presenti e binding esterno. Il segnale deve essere leggibile ma non trasformare il grafo in una board kanban.
    - Estendere la legenda/toolbar del grafo per spiegare questi segnali e permettere toggle rapidi ("show blocked", "show mine", "dim done"), mantenendo URL/localStorage coerenti con i filtri esistenti.
    - Success criterion MVP: un utente vede immediatamente quali task gli sono assegnate e quali nodi sono bloccati/in progress senza aprire uno per uno il detail panel.

- [ ] **Work Item Relations e subtasks agnostici** _(follow-up UX/dominio emerso dal dogfooding, 2026-05-08)_
    - Introdurre relazioni esplicite tra work item senza trasformare Brain UI in un project-management tool rigido: una subtask è un normale `WorkItem` con `brain_id`, stato, assignee, file, history ed eventuale `external_binding`, collegato a un altro work item tramite relazione `parent/child`.
    - Schema frontmatter candidato, da validare con dogfooding: `work_item_relations: [{ type: parent, target: task-... }]`, estendibile a `blocks`, `blocked_by`, `relates_to`, `duplicates` senza legarsi a GitHub sub-issues, checklist Markdown o a un provider specifico.
    - Projection: materializzare una tabella `work_item_relations(target_id, source_brain_id, relation_type, target_brain_id)` derivata dai frontmatter; non modificare il parent file quando cambia un child. Il rebuild resta la fonte di verità iniziale, con eventuali mutazioni dedicate più avanti.
    - UI MVP: nel detail panel mostrare "Child work items" e "Related work items"; nella Task Inbox aggiungere un toggle flat/grouped e progress count derivato (`3/7 done`) senza imporre una board kanban.
    - Azione futura: `Create child task` genera un normale documento task con relazione parent, passando dal write orchestrator esistente. L'eventuale creazione/binding di issue esterne resta separata e vive sopra 4.5.
    - Vincolo: checklist nel corpo Markdown restano valide per micro-step locali; diventano work item solo quando servono assegnazione, stato, binding esterno o tracciamento indipendente.

- [ ] **Repo structure: evoluzioni post-MVP** _(follow-up di 3.7, da pianificare dopo dogfooding)_
    - **Heatmap strutturale** — overlay nel tree view che mostra densità di link entranti/uscenti per cartella o età media dei file. Distingue cartelle "vive" da cartelle stagnanti senza richiedere drill-down manuale.
    - **Vista "directory ↔ node type" come matrice** — diagnostica admin che mostra distribuzione: quali tipi di nodo finiscono in quali cartelle, quali cartelle hanno mix sospetti (es. `runbook` in `concepts/`).
    - **Sezione `Recently created/modified` come feed semantico** — dipende dalla futura estensione `files.commit_author`/`commit_message`/`last_commit_at` nella projection. Feed in stile "cosa è successo mentre ero via" con avatar dell'autore, file toccato, messaggio commit sintetizzato e — quando FTS5 è disponibile — snippet del contenuto estratto da `fts5_snippet()`.
    - **Ristrutturazione bulk admin-only** — UI per move-by-pattern (es. "tutti i `runbook` di Q1 in `runbooks/2026/q1/`"), riusa `BranchTransaction` di 4.0 per atomicità + preview via `transaction.plan()`. Solo dopo che 4.0 è chiusa.
    - **Path collision detection in edit mode** — durante rename/move, evidenziare in tempo reale se il path target collide con un file esistente, è soggetto a case-insensitive collision, o supera limiti di profondità ragionevoli. Reuse parziale di `expect_absent` di `GitTransaction`.
    - **Dead link discovery** — la projection ha già `edges`; un edge verso un path non presente in `files.content_path` è un link rotto. Al rebuild, materializzare `broken_edges`. Superficie admin: vista diagnostica "Broken Links" complementare al banner orphan di 3.7.
    - Vincolo trasversale: ognuna di queste evoluzioni deve restare opt-in e non deve trasformare Brain UI in un file manager.

- [ ] **Activity Stream, Co-authorship e Watch/Follow** _(micro-consapevolezza asincrona, post-dogfooding)_
    - **Activity Stream per nodo** — tab "Activity" nel detail panel che fonde due stream: (1) eventi Brain dall'audit log (`work_item_transition`, `assign`, `bind`, `propose_write`, ecc.); (2) commit metadata da aggiungere a `files` (`commit_author`/`commit_message`/`last_commit_at`). Timeline leggibile, Git invisibile.
    - **Co-authorship visibile (face-pile)** — sotto il titolo del nodo nell'editor, avatar sovrapposti degli autori che hanno contribuito al file. `node_authors` materializzato da v2 schema (frontmatter `author:`/`authors:`); face-pile richiede ancora metadata commit/avatar nel rebuild.
    - **Watch/Follow con notifiche in Task Inbox** — prerequisito: Task Inbox. Modello `watches(user_login, target_id, content_path_or_prefix, created_at)` in SQLite. Fan-out SSE→watch server-side sul broadcast bus esistente; notifiche scoped per i sotto-domini di sua competenza.
    - Vincolo trasversale: tutti e tre i sotto-feature si basano sugli stessi dati di commit history e audit log materializzati nella projection — nessun nuovo store, nessuna chiamata GitHub live fuori dal rebuild.
    - Dipendenza: Watch/Follow richiede Task Inbox come container delle notifiche. Activity Stream e face-pile sono indipendenti.

- [ ] **Detail-panel & view presentation: deferred small wins** _(spin-out della "Small Wins" PR 2026-05-25)_
    - Razionale: nella prima passata abbiamo shippato `cover:`, backlink raggruppati per tipo e `weight:` per ordinare le saved views. Le quattro voci sotto sono state escluse dalla PR perché toccano CSP/sanitizer, decisioni di prodotto o nuove dipendenze JS — non sono "additive read-mostly" come le prime tre.
    - **Mermaid theme sync con il tema Brain UI** — oggi ogni diagramma usa `style X fill:#...` hard-coded. Iniettare il tema corrente (light/dark) come variabile a `mermaid.initialize` quando il renderer parte e re-renderizzare al cambio tema. Vincolo: Mermaid è self-hosted sotto CSP stretto (vedi commit `b4ca8f5` e seguenti); decidere se forzare il re-render globale o solo su nuovi diagrammi.
    - **KaTeX in code-fence `math`** — stessa famiglia di Mermaid (lib client + whitelist sanitizer Ammonia + CSP). Pattern già stabilito da `language-mermaid` in `crates/brain-app/src/markdown.rs:sanitize`. Costo iniziale di setup non triviale, da fare in PR dedicata.
    - **Render automatico del diagramma "tipi e relazioni" dalle saved views** — generare lato server un diagramma Mermaid dalla struttura di `.brain-config.yml` e mostrarlo in una landing/home della knowledge. Decisione di prodotto da fare prima (dove vive, quando si aggiorna).
    - **GFM task-list aggregator nei backlink panel** — i `.md` di tipo work item sono pieni di `- [ ]` / `- [x]`. Aggiungere un counter `3/7 done` accanto a ogni backlink (o nel group header) renderebbe immediatamente leggibile la salute di una vista. Estensione naturale del lavoro sui backlink raggruppati di questa PR.

- [ ] **Mermaid load strategy: lazy / SSR / CDN** _(spin-out 2026-05-26 dalla valutazione asset non-Rust)_
    - Razionale: `public/vendor/mermaid-10.9.3.min.js` pesa 3.2 MB ed è bundle-time del binario Brain (servito da Axum), caricato in `<head>` di ogni pagina via `app.rs:35`. È l'asset non-Rust più pesante del progetto — ~30x daisyUI prima della rimozione, ~10x il bundle WASM. Sulla maggior parte delle pagine (landing, list view senza diagrammi) il costo è puro overhead di TTI.
    - [x] **α Lazy-load on demand** _(scope minimo, zero rischio; SHIPPED 2026-06-12)_ — Mermaid è uscito dal `<head>` statico di `app.rs`. `window.loadBrainMermaid` inietta lo `<script async>` solo quando il probe trova blocchi mermaid non processati; la pipeline `window.renderBrainMermaid` resta intatta (il check di presenza fa anche da gate per la ri-scansione dell'observer — chiude review 2026-05-29 #5 nello stesso blocco). Una pagina senza diagrammi non scarica più i 3.2 MB; CSP invariata (`script-src 'self'` copre l'inject same-origin).
    - **β Server-side render (opzionale, decisione di prodotto)** — pre-render dei diagrammi in SVG lato server via `mermaid-cli` o equivalente Rust. Zero JS client per pagine read-only. Trade-off: perdi interattività (zoom/pan/click sui nodi) — accettabile solo se confermato che gli utenti non interagiscono coi diagrammi. Da valutare dopo α in produzione.
    - **γ CDN federation con SRI** _(probabilmente da rigettare)_ — Mermaid via jsdelivr/unpkg con `integrity` hash. Vince ~3.2 MB nel binario Brain e in Dockerfile. Confligge con il content trust boundary del caveat 19 (PARTIAL): introdurre una dipendenza CDN esterna nella stessa fase in cui stiamo chiudendo trust boundary su `inner_html` è incoerente. Listata come anti-goal probabile, non come slice da aprire.
    - Vincolo trasversale: nessuna delle tre opzioni deve toccare la CSP corrente né rimuovere Mermaid dalle feature shippate (work item, concepts visualization).

- [ ] **UI surface bifurcation: Knowledge UI vs Admin/Operator UI** _(direction note 2026-05-26, triggered by 4.6 Multi-Tab)_
    - Razionale: oggi tutto vive in `crates/brain-app` come app Leptos SSR monolitica. Le superfici hanno però visual-language e use-case divergenti: la Knowledge UI è esplorativa/editoriale (graph canvas, detail panel, editor — design Tailwind raw maturo); l'Admin (Views, Brand, future Node Maintenance, Watch/Follow, Workflow policy) è tabulare/operativa con pattern stile Linear/Notion-admin. Forzare un singolo design language sta già creando attrito su quest'ultima superficie.
    - Direzione: promuovere il segnale già scritto nella riga "Segnale di estrazione crate per `knowledge/` UI" (Fase 5, AI Assistant Proxy) a **trigger anticipato** sotto 4.6 Multi-Tab. Quel refactor di stato (`selected_path: RwSignal<Option<String>>` → `Vec<TabState>`) è il momento naturale per disaccoppiare le superfici editor dalla shell di routing in un crate `brain-ui-components`.
    - L'Admin/Operator UI può evolvere indipendentemente con un visual language proprio (denser, tabular, più affordance keyboard-first), senza toccare la Knowledge UI. La decisione su un design system di seconda generazione (daisyUI reintrodotta in modo coerente solo per admin, oppure un altro headless library) si prende **solo quando** Admin guadagna abbastanza superficie da giustificarlo — non ora.
    - **Anti-goal:** un design system condiviso che impone le stesse primitive a Knowledge e Admin. Sono use-case diversi e meritano linguaggi visivi diversi.
    - Trigger: apertura di 4.6 Multi-Tab. Prerequisito non bloccante: `detail_panel.rs` split (già tracciato sopra).
    - Success criterion: dopo lo split, una nuova superficie admin può essere disegnata senza importare componenti `knowledge/` né viceversa; nessun behavioral change su Knowledge UI.

- [ ] **Canvas viewport culling** _(promoted da caveat 18, 2026-05-26)_
    - Razionale: il caveat 18 traccia >500 nodi DOM come threshold di jank sul graph canvas SVG. La risposta è virtualizzazione viewport (render solo dei nodi dentro il `viewBox` corrente + buffer ~20%), non WebGL (anti-goal: interop JS dal WASM non banale) né Canvas2D (perdi DOM interop con Leptos reattività e hover/select states).
    - Implementazione: introdurre un `Memo` che filtra `nodes`/`edges` in base al `viewBox` corrente. Tutta la pipeline force-directed + label collision in `graph_canvas.rs:433` resta intatta — il layout calcola tutte le posizioni, ma il render Leptos itera solo sui nodi visibili. Edges parzialmente fuori viewBox restano renderizzate se almeno uno dei due endpoint è dentro il buffer.
    - **Sinergia con 4.2 Temporal Graph View:** 4.2 introduce un secondo render path (live vs historical SHA). Implementare il culler una sola volta su un componente helper riusabile da entrambi i path evita di accoppiare lo stato dei due viewer.
    - **Anti-goal trasversale:** zero WebGL, zero Canvas2D, zero dipendenze JS aggiuntive. Pure SVG con cull reattivo.
    - Trigger esplicito (già nel caveat 18): jank confermato a >500 nodi su repo reale durante dogfooding con filtro rimosso.
    - Success criterion: un repo con 1000+ nodi senza filtro mantiene 60fps su pan/zoom; il rebuild della projection non rallenta perché il layout non è stato toccato; nessuna regressione visiva su repo piccoli (<200 nodi) dove il culler è effettivamente un no-op.

- [ ] **Graph readability sub-500 nodes** _(opened 2026-05-27 from typed-edges dogfooding)_
    - Razionale: distinto dal caveat 18 ("perf a >500 nodi"). Anche a ~100 nodi il grafo del Pokémon mock dopo PR #19 (typed edges + cluster layout) risultava illeggibile in overview: i tag virtuali contribuiscono il ~25% dei nodi visibili e producono archi stellari che attraversano il grafo; i typed edges cross-cluster hanno la stessa edge-attraction dei body link e dissolvono i cluster type-based che la gravità intra-cluster cerca di tenere; il layout era compresso nel ~40% della superficie del canvas, dando una sensazione iniziale di "muro di nodi" che scoraggia l'esplorazione.
    - **α — Tag-collapsed default** _(shipped 2026-05-27)_: i nodi tag virtuali sono renderizzati di default solo come chip nella legenda dei filtri, non come nodi nel canvas. Un toggle "Tags" nella legenda edge li riporta visibili durante la sessione. Effetto immediato sul mock: 102 → ~76 nodi e ~50 archi stellari in meno. Brain engineering ha 0 tag→ no-op. Persistenza localStorage del toggle rimandata a v2 se il segnale arriva.
    - **β — Edge-attraction per kind** _(shipped 2026-05-27)_: il loop force-directed in `brain-graph/src/layout.rs` applica un moltiplicatore per `EdgeKind`. Body resta al coefficiente attuale (`×1.0`, la storia narrativa modella le distanze), Frontmatter è dampened a `×0.5` (metadata strutturale, non devono spostare i nodi quanto le citazioni), Tag a `×0.25` (membership debole, non vicinanza fisica). I cluster type-based reggono perché la gravità intra-cluster smette di essere sopraffatta dai typed edges cross-cluster.
    - **γ — Breathing layout** _(shipped 2026-05-27)_: cluster ring radius `30 → 40` per spingere i centri cluster verso i bordi invece che ammassarli al centro; ideal edge spring length `10 → 13` per dare alle molle un target più rilassato; hub radius scaling `min(degree, 6) → min(degree, 4)` per evitare che i super-hub (Pikachu, Onix sul mock) coprano i loro vicini diretti. Cumulativamente il grafo usa più superficie del canvas e i nodi leaf non spariscono sotto gli hub.
    - **δ — State cleanup su toggle** _(shipped 2026-05-27)_: quando l'utente nasconde i tag, eventuali `selected`/`hovered` su un tag node vengono ripuliti via `Effect` per evitare di centrare il viewport su un nodo non renderizzato.
    - **Anti-goal:** clustering "expand/collapse" tipo Cytoscape — è un cambio di paradigma del canvas, non un tweak. Resta off-roadmap.
    - **Differenza dal viewport culling**: culling = "render meno cose perché sono fuori schermo" (perf-driven, caveat 18). Questa slice = "rendi più leggibile quello che c'è" (UX-driven, indipendente dallo zoom). Si compongono: a >500 nodi vuoi entrambe.
    - Success criterion: sul mock Pokémon (~100 nodi, 213 typed edges) l'utente che apre il grafo in overview distingue immediatamente cluster per tipo (cluster gravity vince contro edge attraction); il body link narrativo resta visibile contro lo sfondo di typed metadata; i tag virtuali non gonfiano più la conta nodi finché l'utente non li richiama esplicitamente; il layout occupa visibilmente più del 60% della superficie del canvas. Da chiudere quando il dogfooding conferma la sensazione di "respirare" su almeno un secondo brain non-Pokémon.

- [ ] **`detail_panel.rs` split** _(audit follow-up 2026-05-09)_
    - Stato: `crates/brain-app/src/knowledge/detail_panel.rs` è 1199 LoC con ~14 funzioni, 70+ `.clone()`, ~10 `RwSignal<String>` per stato UI transitorio. Già "next candidate" dopo la split di `editor.rs`.
    - Direzione: stessa pattern di `editor/{mod,frontmatter,location,markdown,related,tags}.rs` — estrarre `work_item_card.rs`, `comments.rs`, `delete_dialog.rs`, `rename_dialog.rs`, `backlinks.rs` come componenti separati. Niente nuovi state container globali.
    - Trigger di apertura: la prossima feature che tocca il detail panel non-trivialmente (Activity Stream tab, advisory edit lock banner, multi-tab di 4.6, conflict resolution di 4.4). Fare prima il split chirurgico, poi la feature, in due commit distinti.
    - Success criterion: `detail_panel/mod.rs` < 300 LoC; ogni sub-component testabile in isolamento; nessun behavioral change visibile.

- [ ] **Admin Node Control / manutenzione nodi** _(riduzione frizione editoriale, 2026-04-29)_
    - Aggiungere una superficie admin-only per controllo completo dei nodi, distinta dall'editor standard: l'utente normale continua a modificare contenuto e campi sicuri, mentre admin/maintainer possono correggere struttura, metadati e path senza aprire GitHub o un editor locale.
    - MVP: pannello "Node maintenance" nel detail/editor con raw frontmatter editor validato, preview/diff prima del salvataggio, edit dei campi non esposti dal form standard e recovery dei frontmatter malformati che oggi bloccano il save.
    - Operazioni strutturali: cambio tipo nodo/directory con move sicuro del file, rename path-aware, normalizzazione slug/titolo, aggiornamento backlink quando possibile e avviso esplicito quando l'impatto non è risolvibile automaticamente.
    - Guardrail per azioni distruttive: delete/rename/move devono mostrare impatto su backlink, binding esterni e work item collegati; salvataggio via orchestratore Git già esistente con commit diretto o PR fallback, mai bypassando la source-of-truth del repo.
    - Estensione post-MVP: manutenzione bulk per retag, merge duplicati, correggere nodi orphan/unknown type, aggiungere campi richiesti mancanti e audit trail delle modifiche admin.
    - Success criterion MVP: un admin corregge da UI un nodo con tipo sbagliato o frontmatter rotto, vede il diff, salva con commit/PR esplicito e il grafo/detail si aggiornano senza interventi manuali sul repository.

- [ ] **Advisory Edit Lock (ottimistico)** _(moved from Future Product Expansion 2026-05-25 — signal-driven, UX-shape, not platform-shape)_
    - Razionale: la 3.3 gestisce i conflitti *dopo* che si verificano. Con più contributor che lavorano in parallelo sullo stesso repo, è preferibile segnalare *prima* che qualcuno stia già modificando un nodo, riducendo il numero di PR di conflitto che richiedono review manuale. L'obiettivo non è un lock bloccante (che creerebbe deadlock editoriali) ma un **advisory signal**: visibile, ignorabile, non vincolante.
    - Implementazione senza heartbeat e senza WebSocket: tabella SQLite `editing_locks(target_id, content_path, user_login, opened_at, expires_at)` con TTL 10 minuti. `LoadNodeForEdit` legge il lock corrente e lo restituisce insieme ai dati del nodo. Acquisito all'apertura in edit mode, rilasciato al save/discard/navigazione; scade automaticamente dopo TTL.
    - UI: banner non bloccante nel pannello editor ("Andrea sta modificando questo file — aperto 3 minuti fa"). L'utente può ignorarlo e procedere; comportamento save/PR fallback invariato.
    - Fuori scope: presenza "live" con cursore real-time, lista utenti online, WebSocket/SSE dedicato al presence — vive in Fase 5 se mai. L'advisory lock è leggero e non deve evolvere verso infrastruttura di collaborazione real-time.
    - Gating: lock visibile a utenti con `can_read`; acquisito solo da utenti con `can_write_default_branch || can_review_via_pr`.
    - Success criterion: se Andrea apre `runbooks/foo.md` in edit mode, Matteo — aprendo lo stesso nodo entro i 10 minuti — vede il banner. Nessun comportamento bloccante.

- [ ] **Frontmatter round-trip lossy** _(debito strutturale — review esterna 2026-05-20)_
    - Stato: `merge_frontmatter` in `crates/brain-app/src/api/files.rs` fa overlay deserializzando lo YAML in `BTreeMap`. Conseguenza: l'ordine originale delle chiavi va perso (riordino alfabetico), e commenti inline + stile di quoting scritti a mano dall'utente vengono distrutti al primo save da UI.
    - Impatto: deterrente reale per chi alterna editing da Brain UI ed editing locale da IDE — un round-trip UI riscrive il frontmatter e "sporca" il diff Git con riordini/perdite non semantiche. Più pressante in ottica open source, dove gli utenti vivono nei propri repo.
    - Direzione: migrare a un overlay YAML-aware che preserva ordine/commenti/quoting (parser AST o lens-based, es. `yaml-rust2` con round-trip o un merge mirato che tocca solo le chiavi gestite dal form invece di re-serializzare l'intero documento). Coerente col requisito già dichiarato in 3.4 ("sezioni non toccate devono sopravvivere round-trip senza riformattazione lossy") — qui lo si estende al frontmatter.
    - Trigger: feedback contributor sul rumore nei diff, o prima del rilascio open source se si vuole evitare l'attrito al primo contatto. Success criterion: salvare da UI un file con frontmatter commentato e chiavi in ordine custom non altera né i commenti né l'ordine delle chiavi non toccate.

- [ ] **Contesa Mutex sulle cache per-target** _(debito di scala — review esterna 2026-05-20)_
    - Stato: quattro cache usano `OnceLock<Mutex<HashMap<TargetKey, …>>>` — `graph_cache`/`template_cache` in `brain-storage::lib`, `config_loader::cache` e `installation_token` in `brain-app`. Corrette e prive di lock tenuti attraverso await, ma un singolo `Mutex` serializza tutti gli accessi cross-target.
    - Impatto: trascurabile oggi; diventa un bottleneck solo se l'istanza pubblica scala su molti utenti/target concorrenti. Stessa classe del debito già tracciato per il broadcast SSE (sezione 2A).
    - Direzione: quando il segnale di carico arriva, sostituire con `DashMap` (lock-free per-key) o `moka` (se serve TTL/eviction gestita). Da fare insieme — non separatamente — al refactor `DashMap` del broadcast SSE.
    - Trigger: profiling sotto carico reale multi-tenant, non preventivamente. _(NB: il debito perf del canvas SVG su >1k nodi è già tracciato in 3.7F — D3/Cytoscape hand-off.)_

---

## 🔴 Fase 4: Forge Independence & Deep History

Sfruttare la maturazione del backend GitHub-first per standardizzare il boundary verso altri forge e usare la projection SQLite come motore per la vista temporale e il local/offline mode.

**Principio guida:** il trait non va estratto da `GithubClient` in modo cosmetico. Va ricavato dalle capability realmente saturate in Fase 3: repository discovery, snapshot tree/blob, mutazioni branch/PR, work item sync, webhook normalization e policy di rate limit. Il pezzo più maturo è il transaction layer (`GitTransaction` in `brain-storage::git_transaction`) — la Fase 4 parte da lì (vedi 4.0), non dal client HTTP.

**Reshape 2026-05-25 — tre tracks parallele dopo 4.0:**
- **Track 1 — Transaction maturation:** 4.0 → 4.0-B → 4.0-C (PR-as-Choice) → 4.4 (Conflict Resolution).
- **Track 2 — Forge boundary (gated):** 4.1 `ForgeAdapter` → 4.3 Local/Offline. Si apre solo quando un secondo forge è davvero in scope.
- **Track 3 — User-facing surfaces:** 4.2 Temporal Graph → 4.6 Multi-Tab. 4.5 Auto-binding work item (richiede sidecar outbox recovery — vedi 6.x trigger).

**Reshape 2026-06-13 — transaction layer maturato:**

Grounding aggiornato: save, delete, rename, work-item mutation, config write, asset upload e fallback PR passano tutti da `GitTransaction` o `BranchTransaction`. Il retry duplicato del vecchio orchestrator è stato rimosso; branch upstream/fork, commit e rollback usano una sola policy nel transaction layer. Il config editor usa `transaction.plan()` per una preview YAML prima/dopo legata allo SHA osservato.

- **Keystone completato.** `BranchTransaction` possiede il lifecycle del branch temporaneo, concatena una o più `GitTransaction`, elimina il branch se un commit fallisce e offre rollback esplicito quando fallisce l'apertura della PR. La PR resta intenzionalmente un'azione applicativa separata.

- **Sequenza di ingresso consigliata:**
    1. **4.0-C PR-as-Choice** _(✅ DONE 2026-06-04, #22)_ — smallest spike shippato: il dispatch `WriteIntent` è in tree, l'editor lo usa. Punto di ingresso live ora = il keystone 4.0.
    2. **4.0 keystone** _(✅ DONE 2026-06-13)_ — write path unificato, `BranchTransaction` e `plan()` live.
    3. **Next:** Track 1 collab depth (4.0-B → 4.4); 4.2 Temporal Graph resta indipendente.

- **Risoluzione del fork → Track 1 prima.** Legame con l'open-sourcing (target Luglio 2026): aprire il repo carica esattamente il write/PR/conflict path coi PR dei contributor esterni. Track 1 indirizza quel carico; 4.2 è alta visibilità ma non regge nessuno stress reale introdotto dall'open-sourcing. Tenere 4.2 come ricompensa visibile dopo che 4.0-B/4.4 hanno irrobustito il path.

- **Track 2 (4.1 / 4.3): non aprire ora.** Anche se un `ForgeAdapter` pulito sembrerebbe attraente per i contributor di Luglio, estrarlo prima che il keystone dia 2-3 forme d'uso reali cementa la forma sbagliata. La 4.0 *è* il prep che rende 4.1 sicuro. Resta gated su "secondo forge davvero in scope" (decisione di prodotto).

### Track 1 — Transaction maturation (sequential)

- [x] **4.0 Git Transaction Layer Maturation** _(DONE 2026-06-13; prerequisito di 4.1)_

    Razionale iniziale: `crates/brain-storage/src/git_transaction/mod.rs` era già il pezzo più maturo del workspace — builder fluente, preconditions duali (`expect_absent` / `expect_sha`), retry mirato solo su 422 fast-forward, riuso blob content-addressed, outcome osservabile, invariante No Dual-Write rispetto alla projection. Prima della 4.0 era però validato contro un solo call site applicativo (`api/file_ops.rs::rename_brain_file`) e contro un fallback PR composto inline nel `write_orchestrator`. La slice ha aggiunto le forme d'uso reali necessarie senza anticipare il futuro `ForgeAdapter`.

    Obiettivo della 4.0: chiudere quelle forme nel runtime esistente, **senza ancora** estrarre un crate esterno o un trait multi-forge. Le primitive di sotto vengono disegnate in modo che il loro nome e contratto siano riusabili tali e quali da un futuro `ForgeTransactionAdapter`.

    **Shipped:** `BranchTransaction`, `TransactionPlan`, preview config a due passaggi, migrazione di tutti i write path e test pubblici in `crates/brain-storage/tests/git_transaction/`. I regression test low-level restano colocati nel modulo per accedere agli helper privati.

    - **`BranchTransaction` esplicita — ✅ shipped.** `BranchTransaction::new(base_sha, branch_name).add(GitTransaction).commit_all(...)` crea il ref, concatena i commit sul branch e restituisce `BranchTransactionOutcome { branch, head_sha, commits }`. Rollback automatico su commit failure ed esplicito su PR failure.

    - **`expect_tree_sha(prefix)`** _(consumer-driven: lo tira Admin Node Control / 4.5)_ — generalizzazione di `expect_sha` a livello directory. Oggi puoi dichiarare "il file X è a sha Y", non "la sottocartella Z non è cambiata sotto di me". Serve a Admin Node Control (move/rename di intere directory, retag bulk, fix orphan) e al futuro auto-binding 4.5 quando la mutazione tocca più file derivati dal work item. Implementazione: leggere `base_tree` recursive (già fatto) e calcolare un hash deterministico delle entry sotto `prefix`, oppure verificare che il `tree.sha` della subdir non sia cambiato.

    - **`transaction.plan(...) → TransactionPlan` — ✅ shipped.** Il dry-run usa solo GET, espone head/tree, upsert/delete e precondition tipizzate. Il Saved Views editor mostra YAML prima/dopo e la conferma rifiuta uno SHA diventato stale.

    - **`TransactionObserver` trait** _(consumer-driven, basso valore finché audit/intent inline non fa male)_ — hook su `attempt_started / precondition_failed / fast_forward_retry / committed`. Oggi i log/audit sono inline. Estrarre l'observer chiarisce cosa appartiene al transaction layer (eventi tecnici) e cosa appartiene all'audit applicativo (intent: `propose_write`, `propose_rename`, ecc.).

    - **Idempotency keys (opzionale, da valutare)** — hash deterministico di `(path-set, expected-shas, message-prefix)` per de-dup di replay webhook in 3.2-β bidirectional. Da introdurre solo se in produzione vediamo replay reali; non designare in astratto.

    - **Crate split — non ancora.** L'estrazione in crate esterno `git-tx-github` (o nome migliore) viene **rinviata a 4.1**, perché solo a quel punto avremo: (a) un secondo adapter forge che valida la forma del trait, (b) un error type proprio (oggi dipendenza concreta su `BrainError`), (c) un client trait minimo (oggi `GithubClient` è una struct concreta). Estrarre prima cementerebbe scelte locali. Pubblicazione su crates.io ulteriormente rinviata: solo dopo ≥3 mesi di superficie API stabile in produzione su due adapter reali.

    - **Crate Rust di terze parti — valutati e scartati per questa slice.** Per archiviare la decisione: `octocrab` ha solo wrapper sottili sulla Git Data API e zero transazionalità — sostituirebbe `GithubHttp` (50 righe) con ~5 KLOC nel binario senza vantaggi. `gix`/`git2` parlano protocollo Git locale, non REST forge: rilevanti per 4.3 (Local mode), non qui. `backon`/`backoff` sostituirebbero le ~30 righe di `BackoffPolicy` con una dipendenza che capisco peggio. `reqwest-retry` non distingue retry semantici (422 fast-forward = retry, 422 precondition = abort) e quindi non è usabile.

    - **Deferred consumer-driven:** `expect_tree_sha(prefix)` resta per Admin Node Control/4.5; `TransactionObserver` resta rinviato finché audit inline non crea attrito; idempotency keys richiedono evidenza di replay reali. Nessun crate split o trait forge in questa slice.

- [ ] **4.0-B Edit Session (Staged Commit)**

    Razionale: oggi ogni azione utente (salva nodo A, rinomina B, aggiorna tag C) produce un commit distinto — oppure, per utenti senza write access diretto, una PR per azione. In un workflow editoriale reale questo inquina la history e spezza la semantica collaborativa: i reviewer ricevono N PR atomiche invece di una che rappresenta l'intento dell'autore. Il Level 1 (atomicità per singola azione multi-file) è già in produzione tramite `GitTransaction`. Questa voce copre il Level 2: raggruppare N azioni distinte di una sessione in un solo commit o PR.

    - **Draft state** — durante la sessione, le modifiche pendenti si accumulano in un buffer lato client. Ogni `save`, `delete`, `rename` aggiunge una `GitTransaction` alla coda invece di committare immediatamente. Il buffer è visibile all'utente come badge "N modifiche in sospeso" nella toolbar. Valutare se il buffer sopravvive al reload (localStorage vs SQLite `edit_session` server-side): la variante server-side è necessaria solo se emerge il caso d'uso "riprendo la sessione dopo un giorno".
    - **Commit session** — l'utente invoca esplicitamente "Commit tutto" o "Proponi modifiche" (per utenti PR fallback). Il runtime serializza le `GitTransaction` pendenti, le compatta in una `BranchTransaction` (4.0) e committa in un solo round trip verso il forge. Il messaggio di commit è scelto esplicitamente dall'utente — non l'aggregato automatico dei messaggi delle singole azioni. `transaction.plan()` viene chiamato prima del commit per mostrare un preview e rilevare precondition failure (file rimosso da un teammate nel frattempo).
    - **Interazione con permessi e PR:** Write access diretto: commit session → 1 commit; PR fallback: 1 branch effimero → 1 PR che raccoglie tutte le modifiche. Il controllo permission avviene al "Commit session", non durante l'accumulo. Coerente col modello `write_orchestrator` esistente. **Sinergia con 4.0-C:** quando 4.0-C ship la scelta esplicita "Commit directly vs Propose via PR", "Commit session as PR" diventa la default editoriale per batch review.
    - **Conflitti durante accumulo** — se un webhook notifica che il branch è cambiato mentre ci sono modifiche pendenti, il banner `Stale Data` esistente si estende: invece di "reload perdendo tutto", propone "review conflict prima di committare". Il merge semantico completo (4.4) chiude questo in modo pieno; questa voce si limita a non perdere silenziosamente il draft.
    - **Vincoli:** draft TTL esplicita (sessione browser o max 24h) — non può diventare un second source of truth persistente. Non introduce advisory lock o presenza real-time.
    - **Prerequisito bloccante:** 4.0 (`BranchTransaction` + `plan()`). Non anticipabile prima. Non blocca 4.1.

- [x] **4.0-C PR-as-Choice (Opt-in Review Flow)** _(DONE 2026-06-04 via #22 — smallest spike; estensioni deferred)_

    **Shipped (smallest spike, #22):** `WriteIntent { Direct, ProposeViaPr }` su `BrainFilePayload` (serde-default → backward compat); `save_file_permission_aware` instrada sul PR orchestrator anche con push rights quando `ProposeViaPr` (branch su upstream, no fork); toggle "Propose via PR" nell'editor gated su `can_write_default_branch`, label Save coerente con la postura; audit `propose_write` tagga `(explicit)`/`(fallback)` così la metrica del success criterion è ricavabile dal log. UX validata in dogfooding.

    **Deferred (follow-up, riaprire con segnale reale):** estensione `ProposeViaPr { reviewers?, draft?, title_override? }` con reviewer picker + draft toggle inline; per-target default in user-prefs ("preferisco sempre proporre via PR su questo repo"); routing del path work-item-card (`work_items.rs`) attraverso `WriteIntent` (oggi solo editor). Il body sotto resta come spec di questi follow-up.

    Razionale: oggi il PR fallback è raggiungibile solo *perdendo* write access. Questo conflate *posture* con *capability*: gli utenti reali di Git aprono PR anche quando potrebbero committare direttamente, per invitare review, draftare lavoro, o aderire a norme di team. Promuovere "Propose via PR" a scelta esplicita su ogni mutating surface allinea Brain UI a come i team usano Git davvero, e sblocca affordance impossibili oggi (draft PR intenzionale, reviewer picker, "commit session as PR" da 4.0-B).

    - **Pattern:** estendere `save_file_permission_aware` a ricevere un `WriteIntent::Direct | ProposeViaPR { reviewers?, draft?, title_override? }` invece di derivare il branch dalla sola capability matrix. Il default preserva il comportamento attuale (direct se `can_write_default_branch`, PR altrimenti) — additivo, nessuno perde flow.
    - **UI shape:** in editor e work-item card, accanto a "Save" appare un secondary control "Propose via PR" attivo per utenti con `can_write_default_branch` (chi non ha write access vede già solo PR e nulla cambia). Per-target default in user prefs ("io preferisco sempre proporre via PR su questo repo"). Reviewer picker e draft toggle inline nel form di proposta.
    - **Sinergia:**
        - **4.0-B Edit Session:** "Commit session" guadagna un toggle Direct vs Propose, default Propose per batch editoriali. La modalità batch-as-PR diventa la posture naturale invece di un fallback forzato.
        - **4.4 Conflict Resolution:** un utente che *sa* di toccare un'area di alta contesa può scegliere PR upfront per evitare il round-trip conflict-detect/conflict-resolve, usando la review come merge negotiation.
    - **Touches:** [crates/brain-app/src/api/files.rs](../crates/brain-app/src/api/files.rs) (`WriteIntent` enum + dispatch), [crates/brain-app/src/server/write_orchestrator.rs] (route on intent invece di solo capability), editor + work-item-card UI, user-prefs SQLite (per-target default).
    - **Anti-goal:** non è un workflow-policy engine — niente "questa cartella richiede sempre PR" via config. La policy enforcement resta alla branch protection lato repo; questa è puramente scelta utente-side.
    - **Prerequisiti:** Phase 3.3 (✅), `ApiError` typed (✅). Beneficia di 4.0 `BranchTransaction` per rollback robusto su reviewer/draft fields, ma core feature ship indipendente.
    - **Size:** M. **Smallest spike:** checkbox "Propose via PR" accanto a Save nell'editor per utenti con `can_write_default_branch`, route attraverso il PR orchestrator esistente, niente reviewer picker / draft toggle / per-target default. Validare la UX prima di estendere.
    - **Success criterion:** un admin con full write access può scegliere consapevolmente di aprire una PR (anche draft, anche con reviewer assegnati) per una sua modifica senza dover rinunciare ai privilegi o forkare il repo; la metrica osservabile è il rapporto direct-commit/PR-proposed per utenti con `can_write_default_branch` che cresce sopra zero.

- [ ] **4.4 Advanced Conflict Resolution**

    **Prerequisito hardening:** typed `ConflictKind` enum (vedi hardening lane sopra, insight 3.2). Senza questo, la UX "vedi che conflitto è" è un'illusione di tipizzazione su una string.

    - La Fase 2B ha introdotto il banner `Stale Data`; con collaborazione reale e fallback via PR non basta più.
    - Quando webhook o ref update rivelano divergenze rispetto al draft locale o al branch corrente, servono diff e merge espliciti: vista side-by-side, scelta hunk-based o almeno `local / remote / apply anyway`.
    - Riuso esplicito del `transaction.plan()` di 4.0: il preview hunk-based non è una funzione separata, è il dry-run della transazione che evidenzia le precondition fallite e mostra cosa cambia per ciascun path coinvolto.
    - Questo asse è particolarmente importante se la 3.3 porta davvero contributor multipli e branch temporanei: il conflitto non sarà più eccezione, ma percorso operativo ordinario.
    - **Vincolo di presentazione — visual diff semantico:** il diff tecnico (`+`/`-` hunk-based) è corretto per lo sviluppatore ma ostile per l'editor testuale. La presentazione deve usare un diff word-level o sentence-level renderizzato come HTML: testo barrato+rosso per le rimozioni, evidenziato+verde per le aggiunte, inline nel Markdown renderizzato. Crate candidato: `similar` o `dissimilar` (entrambi disponibili in Rust, ~20KB, nessuna dipendenza JS). Questo è il livello che trasforma 4.4 da "feature tecnica" a "feature usabile da chiunque scriva documenti".

### Track 2 — Forge boundary (gated)

- [ ] **4.1 `ForgeAdapter` Trait (capability-driven)**
    - Estrarre un boundary che copra i bisogni reali del runtime, non un semplice wrapper HTTP. Le capability minime previste sono: target discovery/listing, lettura tree/blob, write commit/branch/ref update (sopra le primitive `GitTransaction` / `BranchTransaction` chiuse in 4.0), PR/MR creation, work item issue mutation, webhook verification + event normalization.
    - Valutare esplicitamente se implementarlo come un singolo trait o come famiglia di subtrait/capabilities (`ForgeRepoAdapter`, `ForgeTransactionAdapter`, `ForgeCollaborationAdapter`, `ForgeWebhookAdapter`) per evitare un "lowest common denominator" troppo povero o un god-trait ingestibile. La separazione `ForgeTransactionAdapter` è naturale: GitHub Git Data API, GitLab Repository Files API e Gitea hanno semantica simile ma endpoint diversi, e la transazione con preconditions resta utile a tutti.
    - Estrazione del crate `git-tx-<forge>` (o nome migliore): avviene **qui**, non in 4.0. A questo punto esistono due adapter reali che validano la forma del trait, un error type dedicato (`GitTxError` / `ForgeError`) e un client trait minimo che sostituisce la dipendenza concreta su `GithubClient`. La crate resta interna al workspace finché la superficie API non è stabile in produzione per ≥3 mesi su due adapter; solo allora valutare pubblicazione su crates.io.
    - Adapter secondari: **GitLab** e **Gitea/Forgejo**. GitHub resta reference implementation finché i casi reali non stabilizzano la forma finale.

- [ ] **4.3 Local / Offline Execution Context**

    Carryover: prep work `projection/` modularization **DONE 2026-05-02** — `projection.rs` spezzato in `projection/{mod,migrations,target,sync_state,rebuild,bulk_insert,nodes,files,work_items,tests}.rs` senza behavioral changes. La projection è pronta per essere consumata da un secondo runtime.

    - Permettere l'esecuzione di Brain UI contro un `.git` locale o una working tree locale senza dipendere da Axum come proxy di un forge remoto.
    - Implementare un `LocalFileSystemAdapter`/`LocalGitAdapter` che offra la stessa superficie minima usata dal runtime: lettura snapshot, commit locali, branch locali, eventuale sync successivo verso remoto opzionale.
    - Evitare fork architetturali: stessa UI, stesso projection pipeline, diverso adapter.
    - **Segnale di estrazione crate per `server/projection`** — quando 4.3 introduce `LocalGitAdapter`, `server/projection` diventa consumato da due runtime distinti (forge remoto e filesystem locale). Quel momento è il trigger naturale per estrarre `brain-projection` come crate separato, non prima.

### Track 3 — User-facing surfaces (independent)

- [ ] **PR Management surface (propose → link → view → merge)** _(organic Track 3 thread, emerso dal lavoro 4.0-C, 2026-06-04)_

    Razionale: una volta che Brain UI può *proporre* via PR (4.0-C), il passo naturale è chiudere il ciclo nella stessa UI invece di mandare l'utente su GitHub — vedere le PR aperte del target e mergiarle. Traiettoria a gradini, ognuno autonomo e shippabile.

    - **propose** _(✅ #22, via 4.0-C)_ — `WriteIntent::ProposeViaPr` opt-in nell'editor.
    - **link** _(✅ #23)_ — link cliccabile "View pull request #N" dopo una scrittura via PR (editor + work-item card); `pr_link` azzerato a inizio submit per non mostrare link stale.
    - **view** _(✅ #24)_ — route dedicata `/{org}/{repo}[/{branch}]/pulls`, lista read-only delle PR aperte scoped sul base branch del target, link "Pulls" nell'header. `list_open_prs` gated su `can_read`.
    - **merge** _(✅ #25)_ — bottone "Merge" per riga gated su `can_write_default_branch`, conferma a due click, refetch al successo, banner che surfaccia il motivo d'errore di GitHub. Squash-only. `merge_pull_request` gated server-side; la branch protection resta enforced da GitHub (noi facciamo solo il gate d'ingresso e surfacciamo i 405/409).

    Deferred (non bloccanti): scelta del merge method (merge/squash/rebase), close/decline di una PR, preview per-PR di `mergeable` + check status (serve un fetch per-PR), bump immediato di `graph_version` al merge (oggi la freshness passa per webhook/SSE), unit test mock-server sul path di merge (sul modello di `git_transaction`).

    Anti-goal: Brain UI non diventa un client forge completo — niente review/commenti/diff inline sulle PR, niente gestione branch. Resta un control plane editoriale che chiude il ciclo propose→merge sulle proprie scritture.

- [ ] **4.2 Temporal Graph View (Git Time Jump)**
    - La feature non deve limitarsi a mostrare vecchi file. L'obiettivo è una modalità storica completa con slider/timeline che ricostruisce una projection temporanea del repository a una data/SHA e la rende navigabile nella stessa UI a grafo.
    - Reuse intenzionale della pipeline esistente: fetch tree storico → build in-memory/ephemeral SQLite projection → render di graph canvas, detail panel e knowledge base in modalità read-only storica.
    - Estensione naturale: confronto `then vs now` per nodi, backlink e work item bindati, senza introdurre un parser o store storico separato.
    - **Layout host già pronto (vedi 3.7G):** il bottom slot della sidebar di `KnowledgePage` è riservato a una history graph view stile VS Code source-control. Attivazione: flippare `HISTORY_SLOT_ENABLED` in `crates/brain-app/src/knowledge/filter_panel.rs` e popolare il contenuto. A quel punto valutare un draggable splitter tra filter pane e history slot (rinviato apposta da 3.7G per calibrare il rapporto su un componente reale, non su una previsione).

- [ ] **4.6 Multi-Tab Detail/Editor Workspace**
    - Sostituire `selected_path: RwSignal<Option<String>>` con uno stato a tabs (`Vec<TabState>` + active index), così l'utente può aprire più nodi simultaneamente senza perdere contesto durante cross-reference.
    - Le tab vivono nel pannello destro esistente (no two-pane split): cambia il modello di stato, non il layout. Esc cascade si estende al close della tab attiva prima del clear della selezione.
    - URL contract: `?tabs=path1,path2&active=1` sopra il routing multi-tenant già introdotto in 3.1.
    - Prerequisito reale: 4.2 Temporal Graph View, che già introduce un secondo "modo" del detail panel (live vs historical SHA). Disegnare tabs dopo 4.2 evita di astrarre nel buio.

- [ ] **4.5 Auto-binding work item ↔ issue/tracker esterni**

    **Prerequisito hardening:** sidecar-based outbox recovery (vedi Fase 6 trigger / insight 3.3). Auto-create-issue + write-file è esattamente il multi-step provider write il cui mid-flight failure oggi finisce nell'outbox opaco. Senza intent journal, l'operatore non può decidere se l'issue è stata creata.

    - Oggi il binding di un `WorkItem` verso un'issue GitHub è interamente manuale: l'utente compila `system / project / item_key / url` nel form di `BindWorkItem`. Non c'è creazione automatica di issue dal documento Brain, né auto-discovery di issue già esistenti che referenziano il `brain_id` o il path del nodo.
    - Conseguenza pratica: `system_of_record = split | external` richiede oggi che l'issue esista già, sia stata creata fuori da Brain UI e che l'utente conosca il numero. Per workflow `Brain-first` (creo il task in UI → voglio che l'issue venga creata su GitHub e bindata automaticamente) non c'è ancora un percorso supportato.
    - Direzione: introdurre un'azione `Create & bind issue` lato UI che, sopra l'orchestratore di scrittura permission-aware (3.3) e l'adapter forge (4.1), apra l'issue con titolo/body derivati dal documento, applichi le label `brain:*` da `label_taxonomy` e popoli `external_binding` in un singolo flusso atomico. In parallelo, esplorare auto-match su issue esistenti come hint nel binding form invece che come legame implicito.
    - Vincolo: l'auto-binding non deve diventare implicito o silenzioso — `system_of_record` resta esplicito, e la creazione issue richiede comunque conferma utente e capability di scrittura sul forge target.

---

## 🟣 Fase 5: AI & Automations Ecosystem

Trasformare la Brain UI in un assistente attivo tramite IA e trigger di automazione esterni.

- [ ] **AI Assistant Proxy (Copilot Integration)**
    - Assistente AI nell'editor Leptos: generazione markdown, autocompletamento, summarization, tagging.
    - Proxy sicuro in Axum che usa l'OAuth token dell'utente per le API AI di GitHub/Copilot (RBAC + accounting corretto).
    - **MCP plumbing decision (Fase 3.3 closeout):** l'esposizione read-side della projection (graph, work_items, nodi per tag/tipo) avviene via endpoint MCP-compliant (SSE/HTTP) sopra l'infra SSE già in [crates/brain-app/src/server/sse.rs](../crates/brain-app/src/server/sse.rs) e l'OAuth in [crates/brain-app/src/server/auth.rs](../crates/brain-app/src/server/auth.rs), riutilizzando le query parametriche introdotte in 3.5. **Auth via PAT/scoped token (separato dalle session OAuth)** — così tool MCP-compatibili (Cursor, Claude Desktop, ecc.) possono interrogare la knowledge base senza riusare la sessione browser. Design rinviato a quando RBAC di 3.3 è stabile (✅).
    - **Primo tool MCP naturale = `SearchBrain` (FTS5 + RRF).** L'endpoint search di Future Product Expansion è il primo candidato a essere esposto come MCP tool: la firma è già pensata per la query layer parametrica di 3.5. Cross-reference esplicita per non costruire un'API parallela.
    - **Segnale di estrazione crate per `knowledge/` UI** — quando l'endpoint MCP consuma componenti Leptos in un contesto headless o in un secondo binary (es. CLI con output strutturato), i componenti `knowledge/graph_canvas.rs`, `knowledge/editor/`, `knowledge/detail_panel.rs` diventano candidati a un crate `brain-ui-components` separato. Prep 2026-05-04: `editor.rs` è stato spezzato in `knowledge/editor/{mod,frontmatter,location,markdown,related,tags}.rs` senza behavioral changes; `detail_panel.rs` resta il prossimo candidato quando viene toccato.

- [ ] **Outbound Webhooks Engine**
    - Motore di eventi in background per inviare webhook a sistemi esterni (GitHub Actions, Zapier, CI/CD).
    - Trigger configurabili in `.brain-config.yml` (es. `on_work_item_done: https://...`).

---

## 🔵 Fase 6: Storage Substrate Evolution _(long-horizon, exploratory)_

Phase 6 è la casa per idee che **cambierebbero** il substrate di storage o introdurrebbero un query/ranking engine — il territorio Omnigraph-shaped che abbiamo esplicitamente *rifiutato* per la ladder corrente (vedi [INSIGHTS_OMNIGRAPH_COMPARISON.md](INSIGHTS_OMNIGRAPH_COMPARISON.md)). Sono numerate, non backlog, per tre motivi: (1) le phase numerate restano lette, i backlog decadono in wishlist; (2) segnala a contributor esterni post-open-source che l'evoluzione del substrate ha un piano; (3) centralizza il "cosa ci farebbe riconsiderare X" in un posto solo, invece di sparpagliarlo nei rationale di rejection.

**Principio:** niente in Phase 6 parte senza un trigger reale da Phase 4–5. Non è una wishlist, è un'**escalation ladder** per evoluzione substrate-level, gated sul fatto che Brain UI superi la sua shape corrente.

**Anti-goal trasversale:** Phase 6 non è il posto per rilassare zero-lock-in. Ogni item sotto o (a) preserva "rebuildable from `git clone`", o (b) porta un trigger esplicito che mette il vincolo in rinegoziazione con contesto pieno. La default answer resta "Git as SoT".

- [ ] **6.1 Incremental projection rebuild** _(trigger: median rebuild latency > 2s su un repo dogfooded reale, OR il content-hash drift detection mostra drift giornaliero non triviale)_
    - Costruito sopra `blob_sha` drift detection (hardening lane). Una volta che il changed set si computa cheap, sostituire full-upsert rebuild con `apply(diff)`. Rebuildable-from-Git-alone resta — full rebuild rimane fallback e correctness oracle. Non Lance, non un nuovo substrate. Solo una smarter SQLite write path.
    - **Anti-goal:** rendere la projection uno store primario. La garanzia "rebuild from `git clone`" deve reggere.

- [ ] **6.2 Multi-database-per-target SQLite split** _(trigger: lock contention reale osservata in produzione multi-target hosting, per la reassessment in Schema v2)_
    - Split `data/projections/{target}.db` così write su un target non contendono con read su un altro. WAL + `busy_timeout=5s` è il floor corrente, fine fino a evidenza di contention.
    - **Connessione a Phase 4:** quando 4.3 Local/Offline ship, la projection-per-target shape diventa l'unità naturale comunque. 6.2 può finire come effetto collaterale gratuito di 4.3 invece che come track indipendente.

- [ ] **6.3 Read-snapshot pinning per long renders** _(trigger: report di graph flicker da collaborazione multi-utente — vedi insight 3.5)_
    - SQLite snapshot API per pinnare la projection view per la durata di un render. Standalone-shippable quando il trigger scatta.

- [ ] **6.4 Provider-agnostic policy abstraction** _(trigger: un secondo forge adapter da 4.1 ship davvero AND esiste una differenza policy non triviale fra adapter)_
    - Liftare `WriteCapabilities` e il permissions resolver in un trait `PolicyProvider`. NON Cedar — stessa shape da una-GitHub-call, solo astratta al punto giusto. Designare prima che esistano due adapter shape produrrebbe l'abstraction sbagliata.

- [ ] **6.5 RRF-with-additional-signals** _(trigger: insight 3.4 FTS5+RRF ship e vediamo una classe di query dove due segnali non bastano)_
    - Aggiungere più input ranked-list alla RRF fusion (backlink count, recency, work-item activity, author overlap). Ancora niente vector backend — RRF è provider-agnostic e qualunque segnale cheap si aggrega. Il pattern si estende, non il dependency tree.

- [ ] **6.6 Embedded vector search** _(trigger: domanda utente esplicita per retrieval semantico AND un path che preserva `git clone`-only reproducibility — es. derived index che rebuilda da markdown body, mai source of truth)_
    - L'entry honest a lungo orizzonte per il pattern vector-search di Omnigraph. Listata qui, non rifiutata, perché in 18 mesi il calcolo può cambiare (local embedding models sono small enough oggi che l'argomento dependency footprint si indebolisce ogni anno). Il vincolo hard è invariato: la projection deve restare rebuildable from `git clone` alone, quindi qualunque vector index è un artefatto derivato, mai un SoT.
    - **Probabilmente non parte mai** unless un utente filea recall pain concreto. Listata come honest counterweight a "abbiamo rifiutato questo" — per un lettore futuro, "l'abbiamo messa in Phase 6 con queste condizioni" è più credibile di "non l'abbiamo mai considerata".

---

> **ROADMAP IS ALWAYS SUBJECT TO CHANGES AND REALIGNMENTS** — this sketch is indicative of direction, not a commitment.

---

## Known caveats _(open only — closed caveats moved to [archive](ROADMAP_ARCHIVE.md#closed-caveats-archived))_

1. **CSRF `state_mismatch` diagnostics** — **PARTIAL 2026-04-26**. `SameSite=Lax` is correct for the top-level GitHub callback redirect; `SESSION_COOKIE_SECURE=1` is now confirmed set on Railway, eliminating the most likely cause of dropped state cookies in prod. `oauth_callback` now logs `login_fail/state_missing` (cookie absent → SameSite/Secure/session-store problem) separately from `login_fail/state_mismatch` (cookie present but value differs → replay or stale link), so the two failure modes can be distinguished from the audit log without guessing. Residual risk: a horizontal scale-out on Railway without a shared session store would still drop state; revisit if `state_missing` shows up in audit despite the Secure cookie.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

11. **UI limitations (canvas)** — No animated transitions between viewBox states (snap is instant). Nodes near graph edges show empty area outside the data space. Hover does not recenter, only selection does. No zoom: scale stays 100×100. **Mitigation 2026-04-26**: hover/selection states on individual nodes and edges now crossfade via CSS `transition`. The full viewBox tween + zoom controls remain Phase 3.6.

15. **No auto-binding to external issues/trackers** — Il binding di un `WorkItem` verso un'issue del forge è oggi interamente manuale. La soluzione naturale vive sopra il `ForgeAdapter` di 4.1 ed è tracciata come follow-up in **4.5 Auto-binding work item ↔ issue/tracker esterni**. Workaround attuale: creare l'issue su GitHub, copiare numero/URL, bindare manualmente.

18. **Graph canvas DOM scalability** — Il canvas SVG renderizza ogni nodo e arco come elemento DOM distinto gestito dalla reattività Leptos. Fino a ~300–500 nodi simultaneamente visibili le performance sono accettabili; oltre quella soglia — su repo reali con 1000+ nodi senza filtro attivo — il layout force-directed e i re-render reattivi possono introdurre frame drop percepibile. **Promosso 2026-05-26 a slice tracciata** in Future UX Backlog ("Canvas viewport culling"): risposta è virtualizzazione del viewport (render solo dei nodi dentro il viewBox corrente + buffer), prima di considerare Canvas 2D API. WebGL resta anti-goal: interop JS non banale da WASM. Trigger di apertura: jank a >500 nodi confermato in dogfooding. Da rivalutare insieme a 4.2 (Temporal Graph View), che introdurrà un secondo render path e renderà più chiaro il vero collo di bottiglia.

19. **Content trust boundary before embeds/blob/AI** — **PARTIAL 2026-05-01**. Brain UI usa `inner_html` per Markdown renderizzato e commenti issue: corretto per preservare formattazione, ma va trattato come trust boundary esplicito prima di iframe, BYOB e AI-generated content. Tracciato nella **Next Hardening Lane / Security & Content Trust Baseline** (chiusa 2026-05-20, [archive](ROADMAP_ARCHIVE.md#-next-hardening-lane--closed-items-through-2026-05-23)). Micro-fix già applicata: i nuovi upload `.svg` non sono più accettati da `UploadAsset`.

21. **Frontmatter round-trip preserves keys, not formatting** — _emerged from audit 2026-05-09._ `merge_frontmatter` overlay parses YAML via `serde_yaml::from_str` into `BTreeMap<String, serde_yaml::Value>` and re-serializes on save. I valori sopravvivono, ma `BTreeMap` ordina alfabeticamente e `serde_yaml` non preserva commenti/blank line/quoting style. Conseguenza: un utente che apre un file nel proprio IDE, aggiunge un commento, e poi salva via Brain UI, vede il commento sparire. Non è regression — è il comportamento del parser. Tracked come slice esplicita in Future UX Backlog ("Frontmatter round-trip lossy"); finché non chiusa, documenta la limitazione.
