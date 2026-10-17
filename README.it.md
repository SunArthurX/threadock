<div align="center">

<img src="docs/images/icon.png" alt="Threadock" width="96" height="96" />

# Threadock

**Un archivio locale. Tutti i tuoi agenti di coding AI. Ricerca completa, governabile, e tutto tuo.**

Un'app desktop local-first che unisce conversazioni, chiamate agli strumenti, comandi, diff e artefatti
provenienti da ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness — li normalizza
in un unico corpus ricercabile e governa continuamente utilizzo, costi e sicurezza.

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Español](README.es.md) · [Русский](README.ru.md)

</div>

<div align="center">

[![Version](https://img.shields.io/badge/version-1.5.0-0071e3?style=flat-square)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri)](https://tauri.app)
[![React](https://img.shields.io/badge/React-18-61dafb?style=flat-square&logo=react)](https://react.dev)
[![Tests](https://img.shields.io/badge/tests-Rust%20510%20%2B%20TS%20377-success?style=flat-square)](#test)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](#installatori)
[![Local-first](https://img.shields.io/badge/local--first-100%25-blueviolet?style=flat-square)](docs/privacy.md)

</div>

<div align="center">

[**Per iniziare**](#per-iniziare) · [**Funzionalità**](#funzionalità) · [**Architettura**](#architettura) · [Guida utente](docs/user-guide.md) · [Privacy](docs/privacy.md) · [Changelog](CHANGELOG.md)

</div>

> 🌐 La versione italiana è una traduzione della comunità. Il riferimento aggiornato è la versione [English](README.md).

---

## Perché Threadock

Probabilmente usi più di un agente di coding AI. ZCode qui, Claude Code là, Cursor per la modifica,
Codex per il lavoro da shell, MiniMax per i task lunghi. Ognuno conserva il proprio storico nel proprio
formato, nel proprio database, con le proprie regole.

Threadock legge questi storici **in sola lettura**, li normalizza in un unico modello canonico e ti dà:

- **Una sola casella di ricerca** che li interroga tutti — con un linguaggio di query (`provider:`,
  `workspace:`, `type:`, `after:`, `before:`, `file:`, `model:`) e tokenizzazione friendly per il
  cinese (e per giapponese/coreano)
- **Un layer di conoscenza** che estrae decisioni, TODO, errori, comandi e file da ogni sessione —
  con un motore a regole veloce di default e un motore LLM opzionale (endpoint compatibili OpenAI,
  locali o cloud) per analisi più ricche
- **Governance** che traccia la spesa di token per progetto, evidenzia trend di costo e tassi di cache
  hit, segnala comandi rischiosi e segreti trapelati, ed esegue retention/GC schedulati
- **Un client desktop che non ti intralcia** — local-first, senza account, senza telemetria, backup
  cifrati AES sotto il tuo controllo

## Screenshot

Tutti gli screenshot qui sotto sono catturati dalla **v1.3.0** su un corpus reale di
**48.282 richieste · 6,48 miliardi di token · 4.553 $ di spesa, su 5 agenti e 1.184 conversazioni**.
Temi chiaro e scuro entrambi supportati; è mostrato il tema scuro.

<table>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/chat.png" alt="Conversazioni — lista, dettaglio con eventi tool inline, timeline dei comandi" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/overview.png" alt="Panoramica — schede KPI, donut uso agenti, trend token giornaliero, ripartizione modelli, top 10 strumenti" /></td>
    <td><img src="docs/images/screenshots/cost.png" alt="Costi — budget mensile, proiezione, top 10 costi per progetto" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/activity.png" alt="Attività — heatmap, distribuzione oraria, Gantt delle conversazioni" /></td>
    <td><img src="docs/images/screenshots/security.png" alt="Sicurezza — rilevamento anomalie, audit, regole comandi pericolosi" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/knowledge.png" alt="Conoscenza — decisioni, TODO ed errori cross-sessione" /></td>
    <td><img src="docs/images/screenshots/projects.png" alt="Progetti — attribuzione costi per workspace" /></td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/assets.png" alt="Asset — skills / plugin / capacità integrate su tutti gli agenti" /></td>
  </tr>
</table>

> Vedi [docs/optimization-rounds/REPORT.md](docs/optimization-rounds/REPORT.md) per i 13 giri di
> rifinitura UI/UX che hanno prodotto la famiglia di layout attuale.

## Architettura

<p align="center">
  <img src="docs/images/architecture.svg" alt="Diagramma dell'architettura" />
</p>

Tre livelli, un invariante: **i dati di terze parti sono in sola lettura**.

- **Adapter** — otto adapter (markdown, jsonl, claude-code, zcode, cursor, minimax, codex,
  deepseek-harness) girano come processi figli isolati via JSON-RPC su stdio. Un crash non abbatte
  l'host; un timeout non blocca la UI
- **Core (`DaemonState`)** — l'unico scrittore. Content addressing BLAKE3, compressione zstd,
  idempotenza, punteggio di completezza, audit trail. Venticinque-plus tabelle SQLite (V16) più un
  indice Tantivy con tokenizzazione N-gram per il cinese (o jieba, opt-in)
- **Frontend** — un'app desktop Tauri 2 (React + TS, 101 comandi, 7 viste), una CLI `ch`, un daemon
  JSON-RPC longevo (14 metodi) e un'API pubblica documentata per client esterni

## Funzionalità

### Hub delle sessioni

| | |
|---|---|
| 🔌 **8 adapter in sola lettura** | ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness · Markdown · JSONL — isolati per processo, crash-safe |
| 🗄️ **SQLite V16 + WAL** | 25+ tabelle, fallback FTS5, migrazione automatica dello schema all'avvio |
| 🔍 **Doppio motore di ricerca** | Tantivy (N-gram cinese + jieba) primario, fallback FTS5, sintassi di query, hit ordinati nel tempo |
| 🧬 **Normalizzazione canonica** | Modello di dominio unificato 6 sorgenti × 19 tipi di evento, hash BLAKE3, idempotente, con punteggio di completezza |
| 📦 **Raw store content-addressed** | Ogni payload originale conservato verbatim per ri-estrazione e audit |
| 🏷️ **Merge automatico dei workspace** | 7 livelli di priorità + confidenza, con UI di override manuale |
| ⭐ **Preferiti, tag, archivio, cancellazione soft + hard** | Con pulizia a cascata su indice di ricerca e raw store |
| 🔁 **Sync incrementale** | tracciamento freschezza `import_state`, auto-sync ogni 10 minuti, resync dopo reset, rinomine lato sorgente recuperate alla sync successiva (le tue rinomine vincono sempre) |
| 💡 **Estrazione della conoscenza** | Motore a regole di default, motore LLM opt-in (OpenAI / DeepSeek / GLM / Ollama / LM Studio / llama.cpp), schema versionato, con storico delle esecuzioni |
| 🔐 **Backup / ripristino cifrati** | XChaCha20-Poly1305 + Argon2id, la chiave non lascia mai il disco |
| 🧪 **Suggerimenti di sessioni simili** | Insieme di candidati limitato, utile per riscoprire contesti passati |
| 🧠 **Ricerche salvate** | Persistenza cross-sessione con ri-esecuzione one-shot |

### Governance CodeAgentOps

Una seconda superficie per team e power user: utilizzo, costi, sicurezza, automazione.

- **Cattura utilizzo + costi** a granularità richiesta su quattro agenti
- **Dashboard** — costi, cache hit rate, trend di latenza P50/P95
- **Audit di sicurezza** — pattern di segreti, comandi pericolosi, report HTML
- **Policy + budget + prezzi** con allarmi di superamento e proiezione di fine mese
- **Inventario degli asset** — skills / plugin su tutti gli agenti
- **Attribuzione costi per progetto** con analisi cache hit
- **Rilevamento anomalie** — picchi di errori, retry storm, eventi di context overflow
- **Health score degli agenti** (0–100, pesato sulla stabilità)
- **Rilevamento spreco di token** con confronto cross-agente
- **Report settimanale** (HTML, generato automaticamente)
- **Ciclo di vita dei dati** — dashboard storage, GC dei blob orfani, policy di retention, rebuild indice
- **Audit trail di governance** — ogni azione sensibile viene loggata

### GUI desktop (7 viste)

`Panoramica` · `Conversazioni` (timeline + operazioni in blocco) · `Conoscenza` (cross-sessione +
doppio motore) · `Attività` (heatmap in stile GitHub + Gantt conversazioni) · `Progetti` ·
`Ops` (costi · asset · sicurezza · automazione) · `Impostazioni`.

- Temi chiaro / scuro in stile Apple, 4 preset di dimensione font (macOS *Larger Text*)
- Interfaccia inglese / cinese semplificato (930+ stringhe, Impostazioni → Aspetto → Lingua)
- Command palette, tour di onboarding, changelog al primo avvio
- Note private per conversazione, sidebar fissa ⌘1–⌘8, dock terminale in basso in-app (⌘J)
- Cronologia ricerche con cancellazione per voce, Gantt conversazioni, scroll orizzontale della lista,
  anteprima nativa delle immagini nei messaggi
- Raggruppamento delle conversazioni con lo stesso titolo (i batch si comprimono in una riga `×N`),
  menu contestuale del pannello sinistro di ricerca (preferito / archivia / fissa / tag / elimina)

## Per iniziare

### Desktop (sviluppo)

```bash
git clone https://github.com/SunArthurX/threadock.git
cd threadock
cd apps/desktop
npm install
npx tauri dev          # in modalità dev il Dock mostra la vera icona dell'app
```

### Build completa

```bash
# CLI + Daemon + adapter Markdown
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Desktop Tauri
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### Installatori

Push di un tag `v*` → la [pipeline di release](.github/workflows/release.yml) compila e pubblica
installer Tauri per **macOS** (dmg arm64 + x64), **Windows** (nsis + msi) e **Linux**
(appimage + deb), più binari CLI per quattro piattaforme e gli SHA256SUMS.

[![GitHub release](https://img.shields.io/github/v/release/SunArthurX/threadock?include_prereleases&style=flat-square)](https://github.com/SunArthurX/threadock/releases)

> Gli installer per ora **non sono firmati** (certificato in corso). macOS: clic destro → Apri
> (oppure `xattr -dr com.apple.quarantine <app>`). Windows: *Altre informazioni* → *Esegui comunque*.
> Verifica sempre gli SHA256SUMS.

### Tour della CLI

```bash
CH=./target/release/ch

# Import (rileva il formato automaticamente)
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# Dai veri dati IDE (sola lettura)
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from zcode <session-id>

# Ricerca con sintassi di query
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex gestione errori'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# Estrazione della conoscenza
$CH --db ./hub.db knowledge <id>            # estrai + mostra
$CH --db ./hub.db knowledge <id> --save     # estrai + persisti (versionato)
$CH --db ./hub.db knowledge <id> --show     # mostra quello salvato
$CH --db ./hub.db similar <id>              # sessioni simili

# Governance / backup
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
CH_BACKUP_PASSWORD="..." $CH --db ./hub.db backup hub.chbak

# Daemon (JSON-RPC su stdio, 14 metodi)
$CH --db ./data daemon
```

### Daemon JSON-RPC

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

Metodi: `system.getInfo` · `workspace.list` · `conversation.list` · `conversation.get` ·
`conversation.delete` · `conversation.restore` · `conversation.similar` · `message.list` ·
`event.list` · `search.query` · `knowledge.extract` · `knowledge.save` · `knowledge.get` ·
`provider.sync`. Vedi [docs/api.md](docs/api.md) per lo schema completo.

## Struttura del repository

```
threadock/
├── crates/
│   ├── domain/              modello canonico (6 sorgenti × 19 tipi di evento)
│   ├── storage/             SQLite V16 + Migration + FTS5 + tabelle governance
│   ├── raw-store/           payload content-addressed (BLAKE3 + zstd)
│   ├── search/              Tantivy (N-gram cinese + jieba opt-in)
│   ├── identity-resolver/   merge automatico workspace (7 livelli di priorità)
│   ├── normalization/       hash BLAKE3 + idempotenza + completezza
│   ├── export/              export Markdown / JSON + redaction
│   ├── backup/              XChaCha20-Poly1305 + Argon2id
│   ├── knowledge/           doppio motore regole + LLM (versionato)
│   ├── llm/                 client LLM + vault di segreti AEAD
│   ├── adapter-sdk/         trait Adapter + protocollo JSON-RPC stdio
│   ├── adapter-host/        isolamento processo (spawn / timeout / crash)
│   ├── adapter-markdown/    adapter Markdown (binario separato)
│   ├── adapter-jsonl/       adapter JSONL
│   ├── adapter-claude-code/ adapter Claude Code
│   ├── adapter-zcode/       adapter ZCode (lettura SQLite)
│   ├── adapter-cursor/      adapter Cursor (state.vscdb)
│   ├── adapter-minimax/     adapter MiniMax Code
│   ├── adapter-codex/       adapter Codex
│   ├── adapter-deepseek-harness/ adapter DeepSeek Harness (dsh)
│   ├── ops-metrics/         metriche utilizzo / costi / salute
│   ├── audit/               audit di sicurezza (segreti + comandi pericolosi)
│   ├── benchmarks/          prestazioni, latenza ricerca, cold start
│   ├── daemon/              servizio long-running (JSON-RPC su stdio)
│   └── cli/                 riga di comando `ch`
├── apps/desktop/            desktop Tauri 2 (React + TS)
│   ├── src/                 7 viste · 101 comandi · 90+ icone SVG
│   └── src-tauri/           backend Rust
├── docs/                    piani, guida utente, privacy, API, benchmark
├── fixtures/                fixture golden anonimizzate + test adapter
└── .github/workflows/       CI (3 OS × lint × test) · CodeQL · release
```

## Sviluppo

### Pre-commit / pre-push

```bash
scripts/precheck.sh --install-hooks    # una tantum: pre-commit=lint, pre-push=test
scripts/precheck.sh                    # manuale: fmt×2 + clippy×2 + tsc + eslint
scripts/precheck.sh test               # lint + tutti i test + build (= CI)
scripts/precheck.sh all                # + cargo audit + check MSRV 1.88
```

Lo script rispecchia `.github/workflows/ci.yml` comando per comando. `clippy` gira con le
**stesse flag della CI** (`-W clippy::pedantic -D warnings`) — un `cargo clippy` nudo in locale
si perde le violazioni pedantic. Via di fuga d'emergenza: `git commit --no-verify`.

### Test

```bash
cargo test --workspace                                          # core Rust
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # comandi Tauri
cd apps/desktop && npm test                                     # React + TS
cargo clippy --workspace --all-targets                          # pulito livello pedantic
cargo test -p ch-search --features jieba                        # tokenizzazione jieba
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture
```

| | |
|---|---|
| **Gate 1** | Ricerca FTS5 su 100k sessioni / 500k messaggi: P95 = **50,9 ms** (target 300 ms) — vedi [docs/benchmark-report-v1.0.0.md](docs/benchmark-report-v1.0.0.md) |
| **Accuratezza merge workspace** | 11 campioni etichettati a mano = 100% (zero auto-merge errati) |
| **ESLint** | 0 errori / 0 warning |
| **Advisory supply chain** | 2 corretti, 1 bloccati upstream (RUSTSEC-2026-0253, in attesa del bump di tantivy; la CI lo tollera) |

## Decisioni di progettazione chiave

| Decisione | Stato | Perché |
|---|---|---|
| Local-first | ✅ | Lo storico delle conversazioni resta sul tuo computer. Nessun account, nessuna telemetria, nessun upload. |
| SQLite WAL | ✅ (V16) | Durabilità su file singolo, backup facili, prevedibile sotto carico. |
| Desktop Tauri 2 | ✅ | Finestra nativa + WebView; un singolo binario Rust che incorpora il daemon. |
| Core Rust | ✅ (MSRV 1.88) | Memory safety + FFI a costo zero; clippy pedantic come barra di qualità. |
| Ricerca Tantivy | ✅ (N-gram + jieba) | N-gram è il default (zero dipendenze esterne); jieba opt-in per un recall cinese migliore. |
| Isolamento processo degli adapter | ✅ | Gli adapter IDE sono read-only e girano come processi separati — un adapter buggato non può abbattere l'host. |
| Doppio storage raw + normalizzato | ✅ | I payload originali sono conservati (chiavi BLAKE3) per la ri-estrazione; la forma normalizzata serve alla ricerca. |
| Dati di terze parti in sola lettura | ✅ | Regola ferrea. Non scriviamo mai in `~/.claude`, `state.vscdb`, ecc. |

Razionale completa: [docs/conversation-hub-execution-plan.md §1.3](docs/conversation-hub-execution-plan.md).

## Documentazione

- [Guida utente](docs/user-guide.md) — installazione, sette viste, sync, ricerca, conoscenza, backup
- [Dichiarazione sulla privacy](docs/privacy.md) — cosa leggiamo, cosa conserviamo, cosa **non** facciamo
- [Riferimento API](docs/api.md) — superficie dei comandi Tauri auto-generata + metodi JSON-RPC del daemon
- [Benchmark delle prestazioni](docs/benchmark-report-v1.0.0.md) — risultati Gate 1 su larga scala
- [Piano complessivo](docs/ai-ide-conversation-hub-enterprise-plan.md) · [Piano di esecuzione](docs/conversation-hub-execution-plan.md) · [Piano di governance](docs/codeagent-ops-plan.md)
- [Report ottimizzazione UI](docs/optimization-rounds/REPORT.md) — 13 giri × 30+ cambiamenti ciascuno
- [Changelog](CHANGELOG.md)

## Roadmap

- [ ] Adapter OpenCode (7ª sorgente IDE)
- [ ] IPC UDS / Named Pipe del daemon + token di auth locale (oggi: solo stdio)
- [ ] Quote di risorse per l'adapter host (memoria / CPU / whitelist file / rete negata)
- [ ] Auto-update con Tauri Updater + firma / notarizzazione degli installer
- [ ] Client Android in sola lettura (PoC Fase 4)
- [ ] Enterprise: SSO / RBAC / KMS / sync cifrata (Fase 5)

## Contribuire

Issue e PR sono benvenute. Prima di aprirne una:

1. Leggi [docs/conversation-hub-execution-plan.md](docs/conversation-hub-execution-plan.md) — la
   sezione «decisioni non negoziabili» spiega perché le cose sono come sono.
2. Esegui `scripts/precheck.sh test` — gli stessi comandi della CI.
3. Se la tua modifica tocca un'API pubblica (comando Tauri, sottocomando CLI, metodo JSON-RPC),
   aggiorna la relativa doc sotto `docs/`.

Per cambiamenti più grandi, apri prima una issue per discutere l'approccio. Il maintainer è l'unico
contributor full-time del progetto; abbi pazienza per la review.

## License

Doppia licenza, a tua scelta:

- [MIT](LICENSE-MIT)
- [Apache 2.0](LICENSE-APACHE)

## Ringraziamenti

- [Tantivy](https://github.com/quickwit-oss/tantivy) per il motore di ricerca
- [Tauri](https://tauri.app) per lo shell desktop
- [SQLite](https://sqlite.org) per il layer di storage
- [React](https://react.dev), [Vite](https://vitejs.dev), [Vitest](https://vitest.dev) per il frontend
- [xterm.js](https://xtermjs.org) per il terminale in-app
- I team dietro ZCode, Claude Code, Cursor, MiniMax Code, Codex e DeepSeek Harness

---

<sub align="center">Sviluppato e mantenuto da <a href="https://github.com/SunArthurX">SunArthurX</a> · 2026</sub>
