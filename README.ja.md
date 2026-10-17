<div align="center">

<img src="docs/images/icon.png" alt="Threadock" width="96" height="96" />

# Threadock

**ひとつのローカルアーカイブに、すべてのAIコーディングエージェントを。全文検索・ガバナンス・完全にあなたのもの。**

ZCode・Claude Code・Cursor・MiniMax Code・Codex・DeepSeek Harness から会話・ツール呼び出し・
コマンド・diff・成果物を収集するローカルファーストのデスクトップアプリ。それらを単一の
検索可能なコーパスへ正規化し、利用量・コスト・セキュリティを継続的にガバナンスします。

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Español](README.es.md) · [Русский](README.ru.md)

</div>

<div align="center">

[![Version](https://img.shields.io/badge/version-1.5.0-0071e3?style=flat-square)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri)](https://tauri.app)
[![React](https://img.shields.io/badge/React-18-61dafb?style=flat-square&logo=react)](https://react.dev)
[![Tests](https://img.shields.io/badge/tests-Rust%20510%20%2B%20TS%20377-success?style=flat-square)](#テスト)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](#インストーラー)
[![Local-first](https://img.shields.io/badge/local--first-100%25-blueviolet?style=flat-square)](docs/privacy.md)

</div>

<div align="center">

[**はじめ方**](#はじめ方) · [**主な機能**](#主な機能) · [**アーキテクチャ**](#アーキテクチャ) · [ユーザーガイド](docs/user-guide.md) · [プライバシー](docs/privacy.md) · [変更履歴](CHANGELOG.md)

</div>

> 🌐 日本語版はコミュニティ翻訳です。最新・最速の更新は [English](README.md) 版を参照してください。

---

## Threadock を選ぶ理由

あなたはおそらく複数のAIコーディングエージェントを使っています。ZCode はこちら、Claude Code はあちら、
編集には Cursor、シェル作業には Codex、長時間ジョブには MiniMax。それぞれが独自のフォーマット・
独自のデータベース・独自のルールで、自分の履歴を保存しています。

Threadock はそれらの履歴を**読み取り専用で**収集し、単一の標準モデルへ正規化して、次のものを提供します:

- **検索ボックスひとつで全体を検索** —— クエリ構文(`provider:`、`workspace:`、`type:`、
  `after:`、`before:`、`file:`、`model:`)と日本語・中国語に優しいトークン化(N-gram、オプションで jieba)に対応
- **ナレッジ層** —— 各セッションから意思決定・TODO・エラー・コマンド・ファイルを抽出。デフォルトは
  高速なルールエンジン、オプションで LLM エンジン(OpenAI 互換エンドポイント、ローカルまたはクラウド)により
  より深い洞察を提供
- **ガバナンス** —— プロジェクトごとのトークン支出、コスト傾向とキャッシュヒット率、危険なコマンドと
  機密漏洩の検出、定期実行のリテンション/GC
- **邪魔しないデスクトップクライアント** —— ローカルファースト、アカウント不要、テレメトリなし、
  自分で管理する AES 暗号化バックアップ

## スクリーンショット

以下のスクリーンショットは、実際のコーパス(**48,282 リクエスト・6.48B トークン・$4,553 の支出・
5 エージェント・1,184 会話**)に対して **v1.3.0** を実行したものです。
ライト/ダーク両テーマに対応しています(ダークテーマを表示)。

<table>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/chat.png" alt="会話 — リスト、ツールイベントをインライン表示する詳細、コマンドタイムライン" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/overview.png" alt="概要 — KPI カード、エージェント使用量、日別トークン傾向、モデル内訳、ツール Top 10" /></td>
    <td><img src="docs/images/screenshots/cost.png" alt="コスト — 月次予算、予測、プロジェクト別コスト Top 10" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/activity.png" alt="アクティビティ — ヒートマップ、時間帯分布、会話ガントチャート" /></td>
    <td><img src="docs/images/screenshots/security.png" alt="セキュリティ — 異常検出、監査、危険コマンドルール" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/knowledge.png" alt="ナレッジ — セッション横断の意思決定・TODO・エラー" /></td>
    <td><img src="docs/images/screenshots/projects.png" alt="プロジェクト — ワークスペース別コスト帰属" /></td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/assets.png" alt="アセット — エージェント横断の skills / plugins / 組み込み機能" /></td>
  </tr>
</table>

> 現在のレイアウトに至る 13 ラウンドの UI/UX 改善については
> [docs/optimization-rounds/REPORT.md](docs/optimization-rounds/REPORT.md) を参照してください。

## アーキテクチャ

<p align="center">
  <img src="docs/images/architecture.svg" alt="アーキテクチャ図" />
</p>

3 層構造、ただひとつの不変のルール:**サードパーティデータは読み取り専用**。

- **Adapters** —— 8 個のアダプター(markdown、jsonl、claude-code、zcode、cursor、minimax、codex、
  deepseek-harness)が stdio JSON-RPC 経由で隔離された子プロセスとして実行されます。
  クラッシュしてもホストをダウンさせず、タイムアウトしても UI を固めません
- **Core(`DaemonState`)** —— 唯一の書き込み者。BLAKE3 コンテンツアドレッシング、zstd 圧縮、
  冪等性、完全性スコアリング、監査証跡。25 以上の SQLite テーブル(V16)+ N-gram 日本語・中国語
  トークン化(またはオプトインで jieba)の Tantivy インデックス
- **Frontends** —— Tauri 2 デスクトップアプリ(React + TS、101 コマンド、7 ビュー)、`ch` CLI、
  常駐 JSON-RPC デーモン(14 メソッド)、外部クライアント向けの公開 API

## 主な機能

### セッションハブ

| | |
|---|---|
| 🔌 **8 個の読み取り専用アダプター** | ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness · Markdown · JSONL —— プロセス隔離、クラッシュセーフ |
| 🗄️ **SQLite V16 + WAL** | 25+ テーブル、FTS5 フォールバック、起動時の自動スキーママイグレーション |
| 🔍 **デュアル検索エンジン** | Tantivy(N-gram + jieba オプション)プライマリ、FTS5 フォールバック、クエリ構文、時系列順のヒット |
| 🧬 **標準正規化** | 6 ソース × 19 イベント型の統一ドメインモデル、BLAKE3 ハッシュ、冪等、完全性スコアリング |
| 📦 **コンテンツアドレス型 Raw Store** | すべての元ペイロードをそのまま保存し、再抽出と監査に備える |
| 🏷️ **ワークスペース自動マージ** | 7 段階の優先度 + 信頼度、手動オーバーライド UI 付き |
| ⭐ **お気に入り・タグ・アーカイブ・ソフト/ハード削除** | 検索インデックスと Raw Store にわたるカスケードクリーンアップ付き |
| 🔁 **インクリメンタル同期** | `import_state` の鮮度追跡、10 分自動同期、リセット後の再同期、ソース側のタイトル変更を次回同期で反映(手動リネームが常に優先) |
| 💡 **ナレッジ抽出** | デフォルトはルールエンジン、オプションで LLM エンジン(OpenAI / DeepSeek / GLM / Ollama / LM Studio / llama.cpp)、スキーマのバージョン管理と実行履歴付き |
| 🔐 **暗号化バックアップ / 復元** | XChaCha20-Poly1305 + Argon2id、鍵はディスク外に出ない |
| 🧪 **類似セッション推奨** | 有界候補セット、過去のコンテキスト再発見に便利 |
| 🧠 **保存済み検索** | セッション横断で永続化、ワンショット再実行 |

### CodeAgentOps ガバナンス

チームとヘビーユーザーのための第二の画面:利用量・コスト・セキュリティ・自動化。

- **利用量・コスト取得** —— 4 エージェントにわたるリクエスト粒度
- **ダッシュボード** —— コスト、キャッシュヒット率、レイテンシ P50/P95 傾向
- **セキュリティ監査** —— 機密パターン、危険コマンド、HTML レポート
- **ポリシー・予算・価格設定** —— 超過アラートと月末予測
- **アセットインベントリ** —— エージェント横断の skills / plugins
- **プロジェクトコスト帰属** + キャッシュヒット分析
- **異常検出** —— エラースパイク、リトライストーム、コンテキストオーバーフロー
- **エージェントヘルススコア**(0–100、安定度加重)
- **トークン無駄遣い検出** + エージェント横断比較
- **週次レポート**(HTML、自動生成)
- **データライフサイクル** —— ストレージダッシュボード、孤立 blob の GC、リテンションポリシー、インデックス再構築
- **ガバナンス監査証跡** —— すべての機密操作を記録

### デスクトップ GUI(7 ビュー)

`概要` · `会話`(タイムライン + 一括操作)· `ナレッジ`(セッション横断 + デュアルエンジン)·
`アクティビティ`(GitHub 風ヒートマップ + 会話ガントチャート)· `プロジェクト` · `Ops`(コスト · アセット ·
セキュリティ · 自動化)· `設定`。

- Apple 風ライト/ダークテーマ、4 段階のフォントサイズ(macOS *Larger Text*)
- 英語 / 簡体字中国語 UI(930+ 文言、設定 → 外観 → 言語)
- コマンドパレット、オンボーディングツアー、初回起動時の変更履歴
- 会話ごとのプライベートメモ、⌘1–⌘8 サイドバーピン留め、アプリ内下部ターミナルドック(⌘J)
- 検索履歴ドロップダウン(個別削除)、会話ガントチャート、リスト横スクロール、メッセージ内画像のネイティブプレビュー
- 同名会話の折りたたみ(バッチ実行を `×N` 1 行に)、検索左パネルのコンテキストメニュー(お気に入り / アーカイブ / ピン / タグ / 削除)

## はじめ方

### デスクトップ(開発モード)

```bash
git clone https://github.com/SunArthurX/threadock.git
cd threadock
cd apps/desktop
npm install
npx tauri dev          # dev モードでは Dock に実際のアプリアイコンが表示されます
```

### 全体のビルド

```bash
# CLI + Daemon + Markdown アダプター
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Tauri デスクトップ
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### インストーラー

`v*` タグを push → [リリースパイプライン](.github/workflows/release.yml) が
**macOS**(dmg arm64 + x64)、**Windows**(nsis + msi)、**Linux**(appimage + deb)向けの
Tauri インストーラーに加え、4 プラットフォームの CLI バイナリと SHA256SUMS をビルド・公開します。

[![GitHub release](https://img.shields.io/github/v/release/SunArthurX/threadock?include_prereleases&style=flat-square)](https://github.com/SunArthurX/threadock/releases)

> インストーラーは現在**未署名**です(証明書対応中)。macOS:右クリック → 開く
> (または `xattr -dr com.apple.quarantine <app>`)。Windows:*詳細情報* → *実行*。
> 必ず SHA256SUMS を検証してください。

### CLI ツアー

```bash
CH=./target/release/ch

# インポート(フォーマット自動判定)
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# 実際の IDE データから(読み取り専用)
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from zcode <session-id>

# クエリ構文付き検索
$CH --db ./hub.db search 後台任務
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex エラー処理'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# ナレッジ抽出
$CH --db ./hub.db knowledge <id>            # 抽出して表示
$CH --db ./hub.db knowledge <id> --save     # 抽出して保存(バージョン管理)
$CH --db ./hub.db knowledge <id> --show     # 保存済みを表示
$CH --db ./hub.db similar <id>              # 類似セッション

# ガバナンス / バックアップ
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
CH_BACKUP_PASSWORD="..." $CH --db ./hub.db backup hub.chbak

# デーモン(stdio JSON-RPC、14 メソッド)
$CH --db ./data daemon
```

### Daemon JSON-RPC

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

メソッド:`system.getInfo` · `workspace.list` · `conversation.list` · `conversation.get` ·
`conversation.delete` · `conversation.restore` · `conversation.similar` · `message.list` ·
`event.list` · `search.query` · `knowledge.extract` · `knowledge.save` · `knowledge.get` ·
`provider.sync`。完全なスキーマは [docs/api.md](docs/api.md) を参照。

## リポジトリ構成

```
threadock/
├── crates/
│   ├── domain/              統一ドメインモデル(6 ソース × 19 イベント型)
│   ├── storage/             SQLite V16 + Migration + FTS5 + ガバナンステーブル
│   ├── raw-store/           コンテンツアドレス型ペイロード(BLAKE3 + zstd)
│   ├── search/              Tantivy(N-gram + jieba オプション)
│   ├── identity-resolver/   ワークスペース自動マージ(7 優先度)
│   ├── normalization/       BLAKE3 ハッシュ + 冪等性 + 完全性
│   ├── export/              Markdown / JSON エクスポート + マスキング
│   ├── backup/              XChaCha20-Poly1305 + Argon2id
│   ├── knowledge/           ルール + LLM デュアルエンジン(バージョン管理)
│   ├── llm/                 LLM クライアント + AEAD シークレットボールト
│   ├── adapter-sdk/         Adapter trait + stdio JSON-RPC プロトコル
│   ├── adapter-host/        プロセス隔離(spawn / タイムアウト / クラッシュ)
│   ├── adapter-markdown/    Markdown アダプター(独立バイナリ)
│   ├── adapter-jsonl/       JSONL アダプター
│   ├── adapter-claude-code/ Claude Code アダプター
│   ├── adapter-zcode/       ZCode アダプター(SQLite 読み取り)
│   ├── adapter-cursor/      Cursor アダプター(state.vscdb)
│   ├── adapter-minimax/     MiniMax Code アダプター
│   ├── adapter-codex/       Codex アダプター
│   ├── adapter-deepseek-harness/ DeepSeek Harness (dsh) アダプター
│   ├── ops-metrics/         利用量 / コスト / ヘルス指標
│   ├── audit/               セキュリティ監査(機密 + 危険コマンド)
│   ├── benchmarks/          パフォーマンス・検索レイテンシ・コールドスタート
│   ├── daemon/              常駐サービス(stdio JSON-RPC)
│   └── cli/                 `ch` コマンドライン
├── apps/desktop/            Tauri 2 デスクトップ(React + TS)
│   ├── src/                 7 ビュー · 101 コマンド · 90+ SVG アイコン
│   └── src-tauri/           Rust バックエンド
├── docs/                    企画書、ユーザーガイド、プライバシー、API、ベンチマーク
├── fixtures/                匿名化ゴールデンフィクスチャ + アダプターテスト
└── .github/workflows/       CI(3 OS × lint × test)· CodeQL · リリース
```

## 開発

### コミット前チェック

```bash
scripts/precheck.sh --install-hooks    # 初回のみ:pre-commit=lint、pre-push=test
scripts/precheck.sh                    # 手動:fmt×2 + clippy×2 + tsc + eslint
scripts/precheck.sh test               # lint + 全テスト + ビルド(CI と同一)
scripts/precheck.sh all                # + cargo audit + MSRV 1.88 チェック
```

スクリプトは `.github/workflows/ci.yml` をコマンド単位で再現します。`clippy` は
**CI と同じフラグ**(`-W clippy::pedantic -D warnings`)で実行されます —— ローカルで
素の `cargo clippy` を実行すると pedantic 違反を見逃します。緊急時の回避策:
`git commit --no-verify`。

### テスト

```bash
cargo test --workspace                                          # Rust コア
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # Tauri コマンド
cd apps/desktop && npm test                                     # React + TS
cargo clippy --workspace --all-targets                          # pedantic レベルでクリーン
cargo test -p ch-search --features jieba                        # jieba トークン化
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture
```

| | |
|---|---|
| **Gate 1** | 10 万セッション / 50 万メッセージの FTS5 検索 P95 = **50.9 ms**(目標 300 ms)—— [docs/benchmark-report-v1.0.0.md](docs/benchmark-report-v1.0.0.md) 参照 |
| **ワークスペースマージ精度** | 手ラベル付き 11 サンプル = 100%(誤自動マージ 0 件) |
| **ESLint** | 0 エラー / 0 警告 |
| **サプライチェーンアドバイザリ** | 2 件修正、1 件は上流ブロック(RUSTSEC-2026-0253、tantivy 更新待ち、CI は許容) |

## 主要な設計判断

| 判断 | 状態 | 理由 |
|---|---|---|
| ローカルファースト | ✅ | 会話履歴はあなたのマシンに留まる。アカウント不要、テレメトリなし、アップロードなし |
| SQLite WAL | ✅(V16) | 単一ファイルの耐久性、簡単なバックアップ、負荷下での予測可能性 |
| Tauri 2 デスクトップ | ✅ | ネイティブウィンドウ + WebView;デーモンを内蔵する単一の Rust バイナリ |
| Rust コア | ✅(MSRV 1.88) | メモリ安全性 + ゼロコスト FFI;品質基準としての pedantic clippy |
| Tantivy 検索 | ✅(N-gram + jieba) | N-gram がデフォルト(外部依存なし);中国語の再現率向上に jieba をオプトイン |
| アダプターのプロセス隔離 | ✅ | IDE アダプターは読み取り専用かつ独立プロセスで実行 —— バグのあるアダプターがホストを落とせない |
| Raw + 正規化の二重ストレージ | ✅ | 元ペイロードを(BLAKE3 キーで)保存して再抽出可能に、検索には正規化形式を使用 |
| サードパーティデータは読み取り専用 | ✅ | 鉄の掟。`~/.claude`、`state.vscdb` などには一切書き戻さない |

完全な背景:[docs/conversation-hub-execution-plan.md §1.3](docs/conversation-hub-execution-plan.md)。

## ドキュメント

- [ユーザーガイド](docs/user-guide.md) —— インストール、7 ビュー、同期、検索、ナレッジ、バックアップ
- [プライバシー声明](docs/privacy.md) —— 何を読み、何を保存し、何を**しない**か
- [API リファレンス](docs/api.md) —— 自動生成された Tauri コマンド一覧 + Daemon JSON-RPC メソッド
- [パフォーマンスベンチマーク](docs/benchmark-report-v1.0.0.md) —— Gate 1 の大規模結果
- [全体計画](docs/ai-ide-conversation-hub-enterprise-plan.md) · [実行計画](docs/conversation-hub-execution-plan.md) · [ガバナンス計画](docs/codeagent-ops-plan.md)
- [UI 改善レポート](docs/optimization-rounds/REPORT.md) —— 13 ラウンド × 各 30+ 変更
- [変更履歴](CHANGELOG.md)

## ロードマップ

- [ ] OpenCode アダプター(7 番目の IDE ソース)
- [ ] Daemon UDS / Named Pipe IPC + ローカル認証トークン(現在:stdio のみ)
- [ ] アダプターホストのリソースクォータ(メモリ / CPU / ファイルホワイトリスト / ネットワーク拒否)
- [ ] Tauri Updater 自動更新 + インストーラーのコード署名 / 公証
- [ ] Android 閲覧専用クライアント(Phase 4 PoC)
- [ ] エンタープライズ:SSO / RBAC / KMS / 暗号化同期(Phase 5)

## コントリビューション

Issue と PR を歓迎します。開く前に:

1. [docs/conversation-hub-execution-plan.md](docs/conversation-hub-execution-plan.md) を読む ——
   「譲れない決定」のセクションが現状の理由を説明しています。
2. `scripts/precheck.sh test` を実行 —— CI と同じコマンドです。
3. 公開 API(Tauri コマンド、CLI サブコマンド、JSON-RPC メソッド)を変更する場合は
   `docs/` 配下の該当ドキュメントを更新してください。

大きな変更については、まず Issue を立てて方針を議論してください。メンテナーはプロジェクト唯一の
フルタイム貢献者です。レビューまでしばらくお待ちください。

## License

デュアルライセンスから選択:

- [MIT](LICENSE-MIT)
- [Apache 2.0](LICENSE-APACHE)

## 謝辞

- [Tantivy](https://github.com/quickwit-oss/tantivy) —— 検索エンジン
- [Tauri](https://tauri.app) —— デスクトップシェル
- [SQLite](https://sqlite.org) —— ストレージ層
- [React](https://react.dev)、[Vite](https://vitejs.dev)、[Vitest](https://vitest.dev) —— フロントエンド
- [xterm.js](https://xtermjs.org) —— アプリ内ターミナル
- ZCode、Claude Code、Cursor、MiniMax Code、Codex、DeepSeek Harness の開発チーム

---

<sub align="center">Built and maintained by <a href="https://github.com/SunArthurX">SunArthurX</a> · 2026</sub>
