<div align="center">

<img src="docs/images/icon.png" alt="Threadock" width="96" height="96" />

# Threadock

**Une archive locale. Tous vos agents de codage IA. Entièrement interrogeable, gouvernable, et à vous.**

Une application de bureau local-first qui rassemble les conversations, appels d'outils, commandes, diffs
et artefacts de ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness — les normalise
en un seul corpus interrogeable, et gouverne en continu l'usage, les coûts et la sécurité.

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Español](README.es.md) · [Русский](README.ru.md)

</div>

<div align="center">

[![Version](https://img.shields.io/badge/version-1.5.0-0071e3?style=flat-square)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri)](https://tauri.app)
[![React](https://img.shields.io/badge/React-18-61dafb?style=flat-square&logo=react)](https://react.dev)
[![Tests](https://img.shields.io/badge/tests-Rust%20510%20%2B%20TS%20377-success?style=flat-square)](#tests)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](#installateurs)
[![Local-first](https://img.shields.io/badge/local--first-100%25-blueviolet?style=flat-square)](docs/privacy.md)

</div>

<div align="center">

[**Démarrage rapide**](#démarrage-rapide) · [**Fonctionnalités**](#fonctionnalités) · [**Architecture**](#architecture) · [Guide utilisateur](docs/user-guide.md) · [Confidentialité](docs/privacy.md) · [Journal des modifications](CHANGELOG.md)

</div>

> 🌐 La version française est une traduction communautaire. La référence à jour est la version [English](README.md).

---

## Pourquoi Threadock

Vous utilisez probablement plus d'un agent de codage IA. ZCode ici, Claude Code là, Cursor pour l'édition,
Codex pour le shell, MiniMax pour les tâches longues. Chacun garde son historique dans son propre format,
sa propre base de données, ses propres règles.

Threadock lit ces historiques **en lecture seule**, les normalise en un modèle canonique unique, et
vous donne :

- **Une seule boîte de recherche** qui couvre tout — avec un langage de requête (`provider:`,
  `workspace:`, `type:`, `after:`, `before:`, `file:`, `model:`) et une tokenisation adaptée au
  chinois (et au japonais/coréen)
- **Une couche de connaissance** qui extrait décisions, TODOs, erreurs, commandes et fichiers de chaque
  session — via un moteur de règles rapide par défaut et un moteur LLM optionnel (endpoints compatibles
  OpenAI, locaux ou dans le cloud) pour un regard plus approfondi
- **Une gouvernance** qui suit les dépenses de tokens par projet, met en évidence les tendances de coût
  et les taux de cache, signale les commandes risquées et les secrets divulgués, et exécute
  rétention/GC planifiées
- **Un client desktop discret** — local-first, sans compte, sans télémétrie, sauvegardes chiffrées AES
  que vous contrôlez

## Captures d'écran

Toutes les captures ci-dessous proviennent de **v1.3.0** exécuté sur un corpus réel de
**48 282 requêtes · 6,48 Md de tokens · 4 553 $ de dépenses, sur 5 agents et 1 184 conversations**.
Thèmes clair et sombre pris en charge ; le thème sombre est affiché.

<table>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/chat.png" alt="Conversations — liste, détail avec événements d'outils en ligne, chronologie des commandes" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/overview.png" alt="Aperçu — cartes KPI, donut d'usage des agents, tendance quotidienne des tokens, répartition par modèle, top 10 des outils" /></td>
    <td><img src="docs/images/screenshots/cost.png" alt="Coût — budget mensuel, projection, top 10 des coûts par projet" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/activity.png" alt="Activité — carte de chaleur, distribution horaire, Gantt des conversations" /></td>
    <td><img src="docs/images/screenshots/security.png" alt="Sécurité — détection d'anomalies, audit, règles de commandes dangereuses" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/knowledge.png" alt="Connaissance — décisions, TODOs et erreurs inter-sessions" /></td>
    <td><img src="docs/images/screenshots/projects.png" alt="Projets — attribution des coûts par espace de travail" /></td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/assets.png" alt="Actifs — skills / plugins / capacités intégrées sur tous les agents" /></td>
  </tr>
</table>

> Voir [docs/optimization-rounds/REPORT.md](docs/optimization-rounds/REPORT.md) pour les 13 tours
> d'optimisation UI/UX qui ont produit la famille de layouts actuelle.

## Architecture

<p align="center">
  <img src="docs/images/architecture.svg" alt="Diagramme d'architecture" />
</p>

Trois couches, un invariant : **les données tierces sont en lecture seule**.

- **Adapters** — huit adaptateurs (markdown, jsonl, claude-code, zcode, cursor, minimax, codex,
  deepseek-harness) tournent en processus enfants isolés via JSON-RPC sur stdio. Un crash n'emporte pas
  l'hôte ; un timeout ne fige pas l'UI
- **Core (`DaemonState`)** — l'unique écrivain. Adressage de contenu BLAKE3, compression zstd,
  idempotence, scoring de complétude, piste d'audit. Plus de vingt-cinq tables SQLite (V16) plus un
  index Tantivy avec tokenisation N-gram pour le chinois (ou jieba, sur inscription)
- **Frontends** — une app desktop Tauri 2 (React + TS, 101 commandes, 7 vues), une CLI `ch`, un daemon
  JSON-RPC persistant (14 méthodes) et une API publique documentée pour les clients externes

## Fonctionnalités

### Hub de sessions

| | |
|---|---|
| 🔌 **8 adaptateurs en lecture seule** | ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness · Markdown · JSONL — isolés par processus, résistants aux crashes |
| 🗄️ **SQLite V16 + WAL** | 25+ tables, repli FTS5, migration automatique du schéma au démarrage |
| 🔍 **Double moteur de recherche** | Tantivy (N-gram chinois + jieba) en primaire, repli FTS5, syntaxe de requête, résultats ordonnés dans le temps |
| 🧬 **Normalisation canonique** | Modèle de domaine unifié 6 sources × 19 types d'événements, haché BLAKE3, idempotent, avec score de complétude |
| 📦 **Raw store adressé par contenu** | Chaque payload original conservé tel quel pour ré-extraction et audit |
| 🏷️ **Fusion automatique des workspaces** | 7 niveaux de priorité + confiance, avec UI de remplacement manuel |
| ⭐ **Favoris, tags, archive, suppression douce + dure** | Avec nettoyage en cascade sur l'index de recherche et le raw store |
| 🔁 **Sync incrémentale** | Suivi de fraîcheur `import_state`, auto-sync toutes les 10 minutes, resync après reset, renommages côté source rattrapés à la sync suivante (vos renommages priment toujours) |
| 💡 **Extraction de connaissance** | Moteur de règles par défaut, moteur LLM optionnel (OpenAI / DeepSeek / GLM / Ollama / LM Studio / llama.cpp), schéma versionné, avec historique d'exécution |
| 🔐 **Sauvegarde / restauration chiffrées** | XChaCha20-Poly1305 + Argon2id, la clé ne quitte jamais le disque |
| 🧪 **Recommandation de sessions similaires** | Ensemble de candidats borné, utile pour redécouvrir un contexte passé |
| 🧠 **Recherches sauvegardées** | Persistance inter-sessions avec ré-exécution en un coup |

### Gouvernance CodeAgentOps

Une deuxième surface pour les équipes et les utilisateurs intensifs : usage, coût, sécurité, automatisation.

- **Capture usage + coût** à la granularité requête sur quatre agents
- **Tableau de bord** — coût, taux de hit cache, tendances de latence P50/P95
- **Audit de sécurité** — motifs de secrets, commandes dangereuses, rapports HTML
- **Politiques + budget + tarifs** avec alertes de dépassement et projection en fin de mois
- **Inventaire d'actifs** — skills / plugins sur tous les agents
- **Attribution des coûts par projet** avec analyse du hit cache
- **Détection d'anomalies** — pics d'erreurs, tempêtes de retries, événements de dépassement de contexte
- **Score de santé des agents** (0–100, pondéré par la stabilité)
- **Détection du gaspillage de tokens** avec comparaison inter-agents
- **Rapport hebdomadaire** (HTML, généré automatiquement)
- **Cycle de vie des données** — tableau de bord stockage, GC des blobs orphelins, politique de rétention, reconstruction d'index
- **Piste d'audit de gouvernance** — chaque action sensible est journalisée

### GUI desktop (7 vues)

`Aperçu` · `Conversations` (chronologie + opérations en masse) · `Connaissance` (inter-sessions +
double moteur) · `Activité` (carte de chaleur style GitHub + Gantt des conversations) · `Projets` ·
`Ops` (coût · actifs · sécurité · automatisation) · `Réglages`.

- Thèmes clair / sombre style Apple, 4 tailles de police (macOS *Larger Text*)
- Interface anglais / chinois simplifié (930+ chaînes, Réglages → Apparence → Langue)
- Palette de commandes, visite d'onboarding, changelog au premier lancement
- Notes privées par conversation, barre latérale épinglée ⌘1–⌘8, dock terminal intégré en bas (⌘J)
- Historique de recherche avec suppression par entrée, Gantt des conversations, défilement horizontal
  de la liste, aperçu natif des images dans les messages
- Repli des conversations homonymes (les exécutions en lot se replient en une ligne `×N`), menu
  contextuel du panneau gauche de recherche (favori / archiver / épingler / tag / supprimer)

## Démarrage rapide

### Desktop (dev)

```bash
git clone https://github.com/SunArthurX/threadock.git
cd threadock
cd apps/desktop
npm install
npx tauri dev          # en mode dev, le Dock affiche la vraie icône de l'app
```

### Tout construire

```bash
# CLI + Daemon + adaptateur Markdown
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Desktop Tauri
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### Installateurs

Push d'un tag `v*` → le [pipeline de release](.github/workflows/release.yml) construit et publie des
installateurs Tauri pour **macOS** (dmg arm64 + x64), **Windows** (nsis + msi) et **Linux**
(appimage + deb), plus les binaires CLI sur quatre plateformes et les SHA256SUMS.

[![GitHub release](https://img.shields.io/github/v/release/SunArthurX/threadock?include_prereleases&style=flat-square)](https://github.com/SunArthurX/threadock/releases)

> Les installateurs ne sont **pas signés** pour l'instant (certificat en cours). macOS : clic droit →
> Ouvrir (ou `xattr -dr com.apple.quarantine <app>`). Windows : *Plus d'infos* → *Exécuter quand même*.
> Vérifiez toujours les SHA256SUMS.

### Tour de la CLI

```bash
CH=./target/release/ch

# Import (détection automatique du format)
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# Depuis de vraies données IDE (lecture seule)
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from zcode <session-id>

# Recherche avec syntaxe de requête
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex gestion des erreurs'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# Extraction de connaissance
$CH --db ./hub.db knowledge <id>            # extraire + afficher
$CH --db ./hub.db knowledge <id> --save     # extraire + persister (versionné)
$CH --db ./hub.db knowledge <id> --show     # afficher le stocké
$CH --db ./hub.db similar <id>              # sessions similaires

# Gouvernance / sauvegarde
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
CH_BACKUP_PASSWORD="..." $CH --db ./hub.db backup hub.chbak

# Daemon (JSON-RPC sur stdio, 14 méthodes)
$CH --db ./data daemon
```

### Daemon JSON-RPC

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

Méthodes : `system.getInfo` · `workspace.list` · `conversation.list` · `conversation.get` ·
`conversation.delete` · `conversation.restore` · `conversation.similar` · `message.list` ·
`event.list` · `search.query` · `knowledge.extract` · `knowledge.save` · `knowledge.get` ·
`provider.sync`. Voir [docs/api.md](docs/api.md) pour le schéma complet.

## Structure du dépôt

```
threadock/
├── crates/
│   ├── domain/              modèle canonique (6 sources × 19 types d'événements)
│   ├── storage/             SQLite V16 + Migration + FTS5 + tables de gouvernance
│   ├── raw-store/           payloads adressés par contenu (BLAKE3 + zstd)
│   ├── search/              Tantivy (N-gram chinois + jieba optionnel)
│   ├── identity-resolver/   fusion automatique des workspaces (7 niveaux de priorité)
│   ├── normalization/       hachage BLAKE3 + idempotence + complétude
│   ├── export/              export Markdown / JSON + masquage
│   ├── backup/              XChaCha20-Poly1305 + Argon2id
│   ├── knowledge/           double moteur règles + LLM (versionné)
│   ├── llm/                 client LLM + coffre de secrets AEAD
│   ├── adapter-sdk/         trait Adapter + protocole JSON-RPC stdio
│   ├── adapter-host/        isolation de processus (spawn / timeout / crash)
│   ├── adapter-markdown/    adaptateur Markdown (binaire séparé)
│   ├── adapter-jsonl/       adaptateur JSONL
│   ├── adapter-claude-code/ adaptateur Claude Code
│   ├── adapter-zcode/       adaptateur ZCode (lecture SQLite)
│   ├── adapter-cursor/      adaptateur Cursor (state.vscdb)
│   ├── adapter-minimax/     adaptateur MiniMax Code
│   ├── adapter-codex/       adaptateur Codex
│   ├── adapter-deepseek-harness/ adaptateur DeepSeek Harness (dsh)
│   ├── ops-metrics/         métriques usage / coût / santé
│   ├── audit/               audit de sécurité (secrets + commandes dangereuses)
│   ├── benchmarks/          perfs, latence de recherche, démarrage à froid
│   ├── daemon/              service persistant (JSON-RPC sur stdio)
│   └── cli/                 ligne de commande `ch`
├── apps/desktop/            desktop Tauri 2 (React + TS)
│   ├── src/                 7 vues · 101 commandes · 90+ icônes SVG
│   └── src-tauri/           backend Rust
├── docs/                    plans, guide utilisateur, confidentialité, API, benchmarks
├── fixtures/                fixtures dorées anonymisées + tests d'adaptateurs
└── .github/workflows/       CI (3 OS × lint × test) · CodeQL · release
```

## Développement

### Pre-commit / pre-push

```bash
scripts/precheck.sh --install-hooks    # une fois : pre-commit=lint, pre-push=test
scripts/precheck.sh                    # manuel : fmt×2 + clippy×2 + tsc + eslint
scripts/precheck.sh test               # lint + tous les tests + build (= CI)
scripts/precheck.sh all                # + cargo audit + vérif MSRV 1.88
```

Le script reflète `.github/workflows/ci.yml` commande pour commande. `clippy` tourne avec les
**mêmes flags que la CI** (`-W clippy::pedantic -D warnings`) — un `cargo clippy` nu en local
manquera les violations pedantic. Échappatoire d'urgence : `git commit --no-verify`.

### Tests

```bash
cargo test --workspace                                          # cœur Rust
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # commandes Tauri
cd apps/desktop && npm test                                     # React + TS
cargo clippy --workspace --all-targets                          # propre niveau pedantic
cargo test -p ch-search --features jieba                        # tokenisation jieba
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture
```

| | |
|---|---|
| **Gate 1** | Recherche FTS5 sur 100k sessions / 500k messages : P95 = **50,9 ms** (cible 300 ms) — voir [docs/benchmark-report-v1.0.0.md](docs/benchmark-report-v1.0.0.md) |
| **Précision de fusion des workspaces** | 11 échantillons étiquetés à la main = 100 % (zéro fusion automatique erronée) |
| **ESLint** | 0 erreur / 0 avertissement |
| **Advisories chaîne d'approvisionnement** | 2 corrigés, 1 bloqué en amont (RUSTSEC-2026-0253, en attente du bump tantivy ; toléré par la CI) |

## Décisions de conception clés

| Décision | Statut | Pourquoi |
|---|---|---|
| Local-first | ✅ | Votre historique de conversations reste sur votre machine. Pas de compte, pas de télémétrie, pas d'upload. |
| SQLite WAL | ✅ (V16) | Durabilité mono-fichier, sauvegardes faciles, prévisible sous charge. |
| Desktop Tauri 2 | ✅ | Fenêtre native + WebView ; un binaire Rust embarquant le daemon. |
| Cœur Rust | ✅ (MSRV 1.88) | Sécurité mémoire + FFI à coût nul ; clippy pedantic comme barre de qualité. |
| Recherche Tantivy | ✅ (N-gram + jieba) | N-gram par défaut (zéro dépendance externe) ; jieba optionnel pour un meilleur rappel chinois. |
| Isolation processus des adaptateurs | ✅ | Les adaptateurs IDE sont en lecture seule et tournent en processus séparés — un adaptateur bogué ne peut pas abattre l'hôte. |
| Double stockage brut + normalisé | ✅ | Payloads originaux conservés (clés BLAKE3) pour ré-extraction ; forme normalisée pour la recherche. |
| Données tierces en lecture seule | ✅ | Règle absolue. Jamais d'écriture dans `~/.claude`, `state.vscdb`, etc. |

Justification complète : [docs/conversation-hub-execution-plan.md §1.3](docs/conversation-hub-execution-plan.md).

## Documentation

- [Guide utilisateur](docs/user-guide.md) — installation, sept vues, sync, recherche, connaissance, sauvegarde
- [Déclaration de confidentialité](docs/privacy.md) — ce qu'on lit, ce qu'on stocke, ce qu'on ne fait **pas**
- [Référence API](docs/api.md) — surface de commandes Tauri auto-générée + méthodes JSON-RPC du daemon
- [Benchmark de performance](docs/benchmark-report-v1.0.0.md) — résultats Gate 1 à grande échelle
- [Plan d'ensemble](docs/ai-ide-conversation-hub-enterprise-plan.md) · [Plan d'exécution](docs/conversation-hub-execution-plan.md) · [Plan de gouvernance](docs/codeagent-ops-plan.md)
- [Rapport d'optimisation UI](docs/optimization-rounds/REPORT.md) — 13 tours × 30+ changements chacun
- [Journal des modifications](CHANGELOG.md)

## Feuille de route

- [ ] Adaptateur OpenCode (7ᵉ source IDE)
- [ ] IPC UDS / Named Pipe du daemon + jeton d'auth locale (aujourd'hui : stdio uniquement)
- [ ] Quotas de ressources de l'adapter host (mémoire / CPU / liste blanche de fichiers / réseau bloqué)
- [ ] Mise à jour auto Tauri Updater + signature / notarisation des installateurs
- [ ] Client Android en lecture seule (PoC Phase 4)
- [ ] Entreprise : SSO / RBAC / KMS / sync chiffrée (Phase 5)

## Contribuer

Issues et PR bienvenues. Avant d'en ouvrir une :

1. Lisez [docs/conversation-hub-execution-plan.md](docs/conversation-hub-execution-plan.md) — la
   section « décisions non négociables » explique pourquoi les choses sont comme elles sont.
2. Lancez `scripts/precheck.sh test` — les mêmes commandes que la CI.
3. Si votre changement touche une API publique (commande Tauri, sous-commande CLI, méthode JSON-RPC),
   mettez à jour la doc concernée sous `docs/`.

Pour les changements importants, ouvrez d'abord une issue pour en discuter. Le mainteneur est le seul
contributeur à plein temps du projet ; merci de votre patience sur la review.

## Licence

Double licence, au choix :

- [MIT](LICENSE-MIT)
- [Apache 2.0](LICENSE-APACHE)

## Remerciements

- [Tantivy](https://github.com/quickwit-oss/tantivy) pour le moteur de recherche
- [Tauri](https://tauri.app) pour l'enveloppe desktop
- [SQLite](https://sqlite.org) pour la couche de stockage
- [React](https://react.dev), [Vite](https://vitejs.dev), [Vitest](https://vitest.dev) pour le frontend
- [xterm.js](https://xtermjs.org) pour le terminal intégré
- Les équipes derrière ZCode, Claude Code, Cursor, MiniMax Code, Codex et DeepSeek Harness

---

<sub align="center">Développé et maintenu par <a href="https://github.com/SunArthurX">SunArthurX</a> · 2026</sub>
