<div align="center">

<img src="docs/images/icon.png" alt="Threadock" width="96" height="96" />

# Threadock

**Un archivo local. Todos tus agentes de codificación con IA. Totalmente buscable, gobernable y tuyo.**

Una app de escritorio local-first que unifica conversaciones, llamadas a herramientas, comandos, diffs
y artefactos de ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness — los normaliza
en un único corpus buscable y gobierna de forma continua el uso, el coste y la seguridad.

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Español](README.es.md) · [Русский](README.ru.md)

</div>

<div align="center">

[![Version](https://img.shields.io/badge/version-1.5.0-0071e3?style=flat-square)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri)](https://tauri.app)
[![React](https://img.shields.io/badge/React-18-61dafb?style=flat-square&logo=react)](https://react.dev)
[![Tests](https://img.shields.io/badge/tests-Rust%20510%20%2B%20TS%20377-success?style=flat-square)](#pruebas)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](#instaladores)
[![Local-first](https://img.shields.io/badge/local--first-100%25-blueviolet?style=flat-square)](docs/privacy.md)

</div>

<div align="center">

[**Inicio rápido**](#inicio-rápido) · [**Características**](#características) · [**Arquitectura**](#arquitectura) · [Guía de usuario](docs/user-guide.md) · [Privacidad](docs/privacy.md) · [Registro de cambios](CHANGELOG.md)

</div>

> 🌐 La versión en español es una traducción de la comunidad. La referencia actualizada es la versión [English](README.md).

---

## Por qué Threadock

Probablemente uses más de un agente de codificación con IA. ZCode aquí, Claude Code allá, Cursor para
editar, Codex para el trabajo de shell, MiniMax para las tareas largas. Cada uno guarda su propio
historial en su propio formato, su propia base de datos, sus propias reglas.

Threadock lee esos historiales **en modo solo lectura**, los normaliza en un único modelo canónico y
te da:

- **Una sola caja de búsqueda** que cubre todos — con un lenguaje de consultas (`provider:`,
  `workspace:`, `type:`, `after:`, `before:`, `file:`, `model:`) y tokenización amigable con el
  chino (y con japonés/coreano)
- **Una capa de conocimiento** que extrae decisiones, TODOs, errores, comandos y ficheros de cada
  sesión — con un motor de reglas rápido por defecto y un motor LLM opcional (endpoints compatibles
  con OpenAI, locales o en la nube) para un análisis más rico
- **Gobernanza** que sigue el gasto de tokens por proyecto, muestra tendencias de coste y tasas de
  acierto de caché, marca comandos arriesgados y secretos filtrados, y ejecuta retención/GC programados
- **Un cliente de escritorio que no molesta** — local-first, sin cuenta, sin telemetría, copias de
  seguridad cifradas con AES que controlas tú

## Capturas de pantalla

Todas las capturas de abajo provienen de **v1.3.0** ejecutándose sobre un corpus real de
**48.282 peticiones · 6.480 M de tokens · 4.553 $ de gasto, en 5 agentes y 1.184 conversaciones**.
Se admiten los temas claro y oscuro; se muestra el oscuro.

<table>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/chat.png" alt="Conversaciones — lista, detalle con eventos de herramientas en línea, línea de tiempo de comandos" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/overview.png" alt="Resumen — tarjetas KPI, dónut de uso de agentes, tendencia diaria de tokens, desglose por modelo, top 10 de herramientas" /></td>
    <td><img src="docs/images/screenshots/cost.png" alt="Coste — presupuesto mensual, proyección, top 10 de coste por proyecto" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/activity.png" alt="Actividad — mapa de calor, distribución horaria, Gantt de conversaciones" /></td>
    <td><img src="docs/images/screenshots/security.png" alt="Seguridad — detección de anomalías, auditoría, reglas de comandos peligrosos" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/knowledge.png" alt="Conocimiento — decisiones, TODOs y errores entre sesiones" /></td>
    <td><img src="docs/images/screenshots/projects.png" alt="Proyectos — atribución de coste por espacio de trabajo" /></td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/assets.png" alt="Activos — skills / plugins / capacidades integradas en todos los agentes" /></td>
  </tr>
</table>

> Consulta [docs/optimization-rounds/REPORT.md](docs/optimization-rounds/REPORT.md) para ver las 13
> rondas de ajuste UI/UX que produjeron la familia de layouts actual.

## Arquitectura

<p align="center">
  <img src="docs/images/architecture.svg" alt="Diagrama de arquitectura" />
</p>

Tres capas, un invariante: **los datos de terceros son de solo lectura**.

- **Adaptadores** — ocho adaptadores (markdown, jsonl, claude-code, zcode, cursor, minimax, codex,
  deepseek-harness) corren como procesos hijos aislados vía JSON-RPC sobre stdio. Un cuelgue no tira
  el host; un timeout no congela la UI
- **Core (`DaemonState`)** — el único escritor. Direccionamiento de contenido BLAKE3, compresión zstd,
  idempotencia, puntuación de completitud, pista de auditoría. Más de veinticinco tablas SQLite (V16)
  más un índice Tantivy con tokenización N-gram para chino (o jieba, opcional)
- **Frontales** — una app de escritorio Tauri 2 (React + TS, 101 comandos, 7 vistas), una CLI `ch`,
  un daemon JSON-RPC persistente (14 métodos) y una API pública documentada para clientes externos

## Características

### Centro de sesiones

| | |
|---|---|
| 🔌 **8 adaptadores de solo lectura** | ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness · Markdown · JSONL — aislados por proceso, a prueba de cuelgues |
| 🗄️ **SQLite V16 + WAL** | 25+ tablas, respaldo FTS5, migración automática de esquema al arrancar |
| 🔍 **Doble motor de búsqueda** | Tantivy (N-gram chino + jieba) primario, respaldo FTS5, sintaxis de consultas, resultados ordenados en el tiempo |
| 🧬 **Normalización canónica** | Modelo de dominio unificado 6 fuentes × 19 tipos de evento, hash BLAKE3, idempotente, con puntuación de completitud |
| 📦 **Raw store direccionado por contenido** | Cada payload original se conserva textualmente para reextracción y auditoría |
| 🏷️ **Fusión automática de workspaces** | 7 niveles de prioridad + confianza, con UI de anulación manual |
| ⭐ **Favoritos, etiquetas, archivo, borrado suave + duro** | Con limpieza en cascada sobre el índice de búsqueda y el raw store |
| 🔁 **Sincronización incremental** | seguimiento de frescura `import_state`, autosync cada 10 minutos, resync tras reset, los renombrados en el origen se captan en la siguiente sincronización (tus renombrados siempre ganan) |
| 💡 **Extracción de conocimiento** | Motor de reglas por defecto, motor LLM opcional (OpenAI / DeepSeek / GLM / Ollama / LM Studio / llama.cpp), esquema versionado, con historial de ejecuciones |
| 🔐 **Copia de seguridad / restauración cifradas** | XChaCha20-Poly1305 + Argon2id, la clave nunca sale del disco |
| 🧪 **Recomendación de sesiones similares** | Conjunto acotado de candidatos, útil para redescubrir contexto pasado |
| 🧠 **Búsquedas guardadas** | Persistencia entre sesiones con reejecución en un clic |

### Gobernanza CodeAgentOps

Una segunda superficie para equipos y usuarios intensivos: uso, coste, seguridad, automatización.

- **Captura de uso + coste** con granularidad de petición en cuatro agentes
- **Panel** — coste, tasa de acierto de caché, tendencias de latencia P50/P95
- **Auditoría de seguridad** — patrones de secretos, comandos peligrosos, informes HTML
- **Políticas + presupuesto + precios** con alertas de desbordamiento y proyección a fin de mes
- **Inventario de activos** — skills / plugins en todos los agentes
- **Atribución de coste por proyecto** con análisis de acierto de caché
- **Detección de anomalías** — picos de errores, tormentas de reintentos, eventos de desbordamiento de contexto
- **Puntuación de salud del agente** (0–100, ponderada por estabilidad)
- **Detección de desperdicio de tokens** con comparación entre agentes
- **Informe semanal** (HTML, generado automáticamente)
- **Ciclo de vida de los datos** — panel de almacenamiento, GC de blobs huérfanos, política de retención, reconstrucción de índice
- **Pista de auditoría de gobernanza** — cada acción sensible queda registrada

### GUI de escritorio (7 vistas)

`Resumen` · `Conversaciones` (línea de tiempo + operaciones en lote) · `Conocimiento` (entre
sesiones + doble motor) · `Actividad` (mapa de calor estilo GitHub + Gantt de conversaciones) ·
`Proyectos` · `Ops` (coste · activos · seguridad · automatización) · `Ajustes`.

- Temas claro / oscuro estilo Apple, 4 tamaños de fuente (macOS *Larger Text*)
- Interfaz en inglés / chino simplificado (930+ cadenas, Ajustes → Apariencia → Idioma)
- Paleta de comandos, tour de bienvenida, registro de cambios en el primer arranque
- Notas privadas por conversación, barra lateral fija ⌘1–⌘8, dock de terminal integrado abajo (⌘J)
- Historial de búsquedas con borrado por entrada, Gantt de conversaciones, desplazamiento horizontal
  de la lista, vista previa nativa de imágenes en los mensajes
- Plegado de conversaciones con el mismo título (los lotes se pliegan en una fila `×N`), menú
  contextual del panel izquierdo de búsqueda (favorito / archivar / fijar / etiquetar / borrar)

## Inicio rápido

### Escritorio (desarrollo)

```bash
git clone https://github.com/SunArthurX/threadock.git
cd threadock
cd apps/desktop
npm install
npx tauri dev          # en modo dev el Dock muestra el icono real de la app
```

### Compilar todo

```bash
# CLI + Daemon + adaptador Markdown
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Escritorio Tauri
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### Instaladores

Push de un tag `v*` → la [pipeline de release](.github/workflows/release.yml) compila y publica
instaladores Tauri para **macOS** (dmg arm64 + x64), **Windows** (nsis + msi) y **Linux**
(appimage + deb), más binarios de CLI para cuatro plataformas y los SHA256SUMS.

[![GitHub release](https://img.shields.io/github/v/release/SunArthurX/threadock?include_prereleases&style=flat-square)](https://github.com/SunArthurX/threadock/releases)

> Los instaladores **no están firmados** por ahora (certificado en trámite). macOS: clic derecho →
> Abrir (o `xattr -dr com.apple.quarantine <app>`). Windows: *Más información* → *Ejecutar de todos
> modos*. Verifica siempre los SHA256SUMS.

### Paseo por la CLI

```bash
CH=./target/release/ch

# Importar (autodetecta el formato)
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# Desde datos reales del IDE (solo lectura)
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from zcode <session-id>

# Búsqueda con sintaxis de consultas
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex gestión de errores'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# Extracción de conocimiento
$CH --db ./hub.db knowledge <id>            # extraer + mostrar
$CH --db ./hub.db knowledge <id> --save     # extraer + persistir (versionado)
$CH --db ./hub.db knowledge <id> --show     # mostrar lo guardado
$CH --db ./hub.db similar <id>              # sesiones similares

# Gobernanza / copia de seguridad
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
CH_BACKUP_PASSWORD="..." $CH --db ./hub.db backup hub.chbak

# Daemon (JSON-RPC sobre stdio, 14 métodos)
$CH --db ./data daemon
```

### Daemon JSON-RPC

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

Métodos: `system.getInfo` · `workspace.list` · `conversation.list` · `conversation.get` ·
`conversation.delete` · `conversation.restore` · `conversation.similar` · `message.list` ·
`event.list` · `search.query` · `knowledge.extract` · `knowledge.save` · `knowledge.get` ·
`provider.sync`. El esquema completo está en [docs/api.md](docs/api.md).

## Estructura del repositorio

```
threadock/
├── crates/
│   ├── domain/              modelo canónico (6 fuentes × 19 tipos de evento)
│   ├── storage/             SQLite V16 + Migration + FTS5 + tablas de gobernanza
│   ├── raw-store/           payloads direccionados por contenido (BLAKE3 + zstd)
│   ├── search/              Tantivy (N-gram chino + jieba opcional)
│   ├── identity-resolver/   fusión automática de workspaces (7 niveles de prioridad)
│   ├── normalization/       hash BLAKE3 + idempotencia + completitud
│   ├── export/              exportación Markdown / JSON + enmascaramiento
│   ├── backup/              XChaCha20-Poly1305 + Argon2id
│   ├── knowledge/           doble motor reglas + LLM (versionado)
│   ├── llm/                 cliente LLM + bóveda de secretos AEAD
│   ├── adapter-sdk/         trait Adapter + protocolo JSON-RPC stdio
│   ├── adapter-host/        aislamiento de procesos (spawn / timeout / cuelgue)
│   ├── adapter-markdown/    adaptador Markdown (binario separado)
│   ├── adapter-jsonl/       adaptador JSONL
│   ├── adapter-claude-code/ adaptador Claude Code
│   ├── adapter-zcode/       adaptador ZCode (lectura SQLite)
│   ├── adapter-cursor/      adaptador Cursor (state.vscdb)
│   ├── adapter-minimax/     adaptador MiniMax Code
│   ├── adapter-codex/       adaptador Codex
│   ├── adapter-deepseek-harness/ adaptador DeepSeek Harness (dsh)
│   ├── ops-metrics/         métricas de uso / coste / salud
│   ├── audit/               auditoría de seguridad (secretos + comandos peligrosos)
│   ├── benchmarks/          rendimiento, latencia de búsqueda, arranque en frío
│   ├── daemon/              servicio persistente (JSON-RPC sobre stdio)
│   └── cli/                 línea de comandos `ch`
├── apps/desktop/            escritorio Tauri 2 (React + TS)
│   ├── src/                 7 vistas · 101 comandos · 90+ iconos SVG
│   └── src-tauri/           backend Rust
├── docs/                    planes, guía de usuario, privacidad, API, benchmarks
├── fixtures/                fixtures doradas anonimizadas + tests de adaptadores
└── .github/workflows/       CI (3 OS × lint × test) · CodeQL · release
```

## Desarrollo

### Pre-commit / pre-push

```bash
scripts/precheck.sh --install-hooks    # una vez: pre-commit=lint, pre-push=test
scripts/precheck.sh                    # manual: fmt×2 + clippy×2 + tsc + eslint
scripts/precheck.sh test               # lint + todos los tests + build (= CI)
scripts/precheck.sh all                # + cargo audit + comprobación de MSRV 1.88
```

El script replica `.github/workflows/ci.yml` comando a comando. `clippy` corre con las
**mismas flags que la CI** (`-W clippy::pedantic -D warnings`) — un `cargo clippy` a pelo en local
se dejará las violaciones pedantic. Vía de escape de emergencia: `git commit --no-verify`.

### Pruebas

```bash
cargo test --workspace                                          # núcleo Rust
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # comandos Tauri
cd apps/desktop && npm test                                     # React + TS
cargo clippy --workspace --all-targets                          # limpio a nivel pedantic
cargo test -p ch-search --features jieba                        # tokenización jieba
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture
```

| | |
|---|---|
| **Gate 1** | Búsqueda FTS5 sobre 100k sesiones / 500k mensajes: P95 = **50,9 ms** (objetivo 300 ms) — ver [docs/benchmark-report-v1.0.0.md](docs/benchmark-report-v1.0.0.md) |
| **Precisión de fusión de workspaces** | 11 muestras etiquetadas a mano = 100 % (cero autofusiones erróneas) |
| **ESLint** | 0 errores / 0 avisos |
| **Avisos de cadena de suministro** | 2 corregidos, 1 bloqueado aguas arriba (RUSTSEC-2026-0253, a la espera del bump de tantivy; la CI lo tolera) |

## Decisiones de diseño clave

| Decisión | Estado | Por qué |
|---|---|---|
| Local-first | ✅ | Tu historial de conversaciones se queda en tu máquina. Sin cuenta, sin telemetría, sin subidas. |
| SQLite WAL | ✅ (V16) | Durabilidad en un solo fichero, copias fáciles, predecible bajo carga. |
| Escritorio Tauri 2 | ✅ | Ventana nativa + WebView; un binario Rust que embebe el daemon. |
| Núcleo Rust | ✅ (MSRV 1.88) | Seguridad de memoria + FFI a coste cero; clippy pedantic como listón de calidad. |
| Búsqueda Tantivy | ✅ (N-gram + jieba) | N-gram por defecto (cero dependencias externas); jieba opcional para mejor recall en chino. |
| Aislamiento de procesos en adaptadores | ✅ | Los adaptadores de IDE son de solo lectura y corren como procesos separados — un adaptador con bugs no puede tirar el host. |
| Doble almacenamiento raw + normalizado | ✅ | Los payloads originales se conservan (claves BLAKE3) para reextraer; la forma normalizada sirve para buscar. |
| Los datos de terceros son de solo lectura | ✅ | Regla férrea. Nunca escribimos en `~/.claude`, `state.vscdb`, etc. |

Justificación completa: [docs/conversation-hub-execution-plan.md §1.3](docs/conversation-hub-execution-plan.md).

## Documentación

- [Guía de usuario](docs/user-guide.md) — instalación, siete vistas, sync, búsqueda, conocimiento, backup
- [Declaración de privacidad](docs/privacy.md) — qué leemos, qué guardamos, qué **no** hacemos
- [Referencia de API](docs/api.md) — superficie de comandos Tauri autogenerada + métodos JSON-RPC del daemon
- [Benchmark de rendimiento](docs/benchmark-report-v1.0.0.md) — resultados Gate 1 a gran escala
- [Plan general](docs/ai-ide-conversation-hub-enterprise-plan.md) · [Plan de ejecución](docs/conversation-hub-execution-plan.md) · [Plan de gobernanza](docs/codeagent-ops-plan.md)
- [Informe de optimización de UI](docs/optimization-rounds/REPORT.md) — 13 rondas × 30+ cambios cada una
- [Registro de cambios](CHANGELOG.md)

## Hoja de ruta

- [ ] Adaptador OpenCode (7.ª fuente IDE)
- [ ] IPC UDS / Named Pipe del daemon + token de autenticación local (hoy: solo stdio)
- [ ] Cuotas de recursos del adapter host (memoria / CPU / lista blanca de ficheros / red denegada)
- [ ] Autoactualización con Tauri Updater + firma / notarización de instaladores
- [ ] Cliente Android de solo lectura (PoC Fase 4)
- [ ] Empresa: SSO / RBAC / KMS / sync cifrada (Fase 5)

## Contribuir

Issues y PRs son bienvenidos. Antes de abrir uno:

1. Lee [docs/conversation-hub-execution-plan.md](docs/conversation-hub-execution-plan.md) — la
   sección de «decisiones no negociables» explica por qué las cosas son como son.
2. Ejecuta `scripts/precheck.sh test` — los mismos comandos que la CI.
3. Si tu cambio toca una API pública (comando Tauri, subcomando CLI, método JSON-RPC), actualiza
   la doc correspondiente bajo `docs/`.

Para cambios grandes, abre primero un issue para discutir el enfoque. El mantenedor es el único
contribuidor a tiempo completo del proyecto; ten paciencia con la revisión.

## License

Doble licencia, a elegir:

- [MIT](LICENSE-MIT)
- [Apache 2.0](LICENSE-APACHE)

## Agradecimientos

- [Tantivy](https://github.com/quickwit-oss/tantivy) por el motor de búsqueda
- [Tauri](https://tauri.app) por el shell de escritorio
- [SQLite](https://sqlite.org) por la capa de almacenamiento
- [React](https://react.dev), [Vite](https://vitejs.dev), [Vitest](https://vitest.dev) por el frontend
- [xterm.js](https://xtermjs.org) por la terminal integrada
- Los equipos detrás de ZCode, Claude Code, Cursor, MiniMax Code, Codex y DeepSeek Harness

---

<sub align="center">Construido y mantenido por <a href="https://github.com/SunArthurX">SunArthurX</a> · 2026</sub>
