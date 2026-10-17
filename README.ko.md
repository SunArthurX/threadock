<div align="center">

<img src="docs/images/icon.png" alt="Threadock" width="96" height="96" />

# Threadock

**하나의 로컬 아카이브에 모든 AI 코딩 에이전트를. 전문 검색, 거버넌스, 그리고 완전한 당신의 것.**

ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness의 대화, 도구 호출, 명령,
diff, 산출물을 하나로 모으는 로컬 우선(local-first) 데스크톱 앱입니다. 이를 단일 검색 가능한
코퍼스로 정규화하고, 사용량·비용·보안을 지속적으로 관리(거버넌스)합니다.

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Español](README.es.md) · [Русский](README.ru.md)

</div>

<div align="center">

[![Version](https://img.shields.io/badge/version-1.5.0-0071e3?style=flat-square)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri)](https://tauri.app)
[![React](https://img.shields.io/badge/React-18-61dafb?style=flat-square&logo=react)](https://react.dev)
[![Tests](https://img.shields.io/badge/tests-Rust%20510%20%2B%20TS%20377-success?style=flat-square)](#테스트)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](#설치-프로그램)
[![Local-first](https://img.shields.io/badge/local--first-100%25-blueviolet?style=flat-square)](docs/privacy.md)

</div>

<div align="center">

[**빠른 시작**](#빠른-시작) · [**주요 기능**](#주요-기능) · [**아키텍처**](#아키텍처) · [사용 가이드](docs/user-guide.md) · [프라이버시](docs/privacy.md) · [변경 이력](CHANGELOG.md)

</div>

> 🌐 한국어판은 커뮤니티 번역입니다. 최신 내용은 [English](README.md) 판을 참고하세요.

---

## Threadock을 만든 이유

아마 여러 개의 AI 코딩 에이전트를 함께 쓰고 있을 겁니다. 여기선 ZCode, 저기선 Claude Code,
편집은 Cursor, 셸 작업은 Codex, 오래 걸리는 작업은 MiniMax로. 각 도구는 저마다의 형식,
저마다의 데이터베이스, 저마다의 규칙으로 자기 기록을 따로 보관합니다.

Threadock은 이 기록들을 **읽기 전용으로** 수집해 하나의 표준 모델로 정규화하고, 다음을 제공합니다:

- **검색창 하나로 전부 검색** —— 쿼리 문법(`provider:`, `workspace:`, `type:`, `after:`,
  `before:`, `file:`, `model:`)과 CJK 친화적 토큰화(N-gram, 선택적으로 jieba) 지원
- **지식 레이어** —— 모든 세션에서 결정 사항, TODO, 오류, 명령, 파일을 추출. 기본은 빠른
  규칙 엔진, 선택 시 LLM 엔진(OpenAI 호환 엔드포인트, 로컬 또는 클라우드)으로 더 깊은 통찰 제공
- **거버넌스** —— 프로젝트별 토큰 지출 추적, 비용 추이와 캐시 적중률, 위험 명령과 시크릿 유출
  플래깅, 예약 실행되는 리텐션/GC
- **방해하지 않는 데스크톱 클라이언트** —— 로컬 우선, 계정 없음, 텔레메트리 없음,
  직접 관리하는 AES 암호화 백업

## 스크린샷

아래 스크린샷은 실제 코퍼스(**요청 48,282건 · 6.48B 토큰 · $4,553 지출 · 에이전트 5종 ·
대화 1,184건**)에 대해 **v1.3.0**을 실행한 결과입니다. 라이트/다크 테마를 모두 지원하며
다크 테마를 표시합니다.

<table>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/chat.png" alt="대화 — 목록, 도구 이벤트가 인라인으로 표시되는 상세, 명령 타임라인" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/overview.png" alt="개요 — KPI 카드, 에이전트 사용량 도넛, 일별 토큰 추이, 모델 분해, 도구 Top 10" /></td>
    <td><img src="docs/images/screenshots/cost.png" alt="비용 — 월별 예산, 전망, 프로젝트 비용 Top 10" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/activity.png" alt="활동 — 히트맵, 시간대 분포, 대화 간트 차트" /></td>
    <td><img src="docs/images/screenshots/security.png" alt="보안 — 이상 감지, 감사, 위험 명령 규칙" /></td>
  </tr>
  <tr>
    <td><img src="docs/images/screenshots/knowledge.png" alt="지식 — 세션 간 결정 사항, TODO, 오류" /></td>
    <td><img src="docs/images/screenshots/projects.png" alt="프로젝트 — 워크스페이스별 비용 귀속" /></td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/images/screenshots/assets.png" alt="자산 — 에이전트 전반의 skills / plugins / 내장 기능" /></td>
  </tr>
</table>

> 현재 레이아웃까지 이른 13라운드의 UI/UX 개선 과정은
> [docs/optimization-rounds/REPORT.md](docs/optimization-rounds/REPORT.md)를 참고하세요.

## 아키텍처

<p align="center">
  <img src="docs/images/architecture.svg" alt="아키텍처 다이어그램" />
</p>

3개 계층, 하나의 불변 규칙: **서드파티 데이터는 읽기 전용**.

- **Adapters** —— 8개 어댑터(markdown, jsonl, claude-code, zcode, cursor, minimax, codex,
  deepseek-harness)가 stdio JSON-RPC로 격리된 자식 프로세스에서 실행됩니다. 크래시가 호스트를
  죽이지 않고, 타임아웃이 UI를 멈추지 않습니다
- **Core(`DaemonState`)** —— 유일한 작성자. BLAKE3 콘텐츠 어드레싱, zstd 압축, 멱등성,
  완전도 스코어링, 감사 로그. 25개 이상의 SQLite 테이블(V16) + N-gram CJK 토큰화(또는
  옵트인 jieba)의 Tantivy 인덱스
- **Frontends** —— Tauri 2 데스크톱 앱(React + TS, 101개 명령, 7개 뷰), `ch` CLI,
  상주 JSON-RPC 데몬(14개 메서드), 외부 클라이언트용 공개 API

## 주요 기능

### 세션 허브

| | |
|---|---|
| 🔌 **8개 읽기 전용 어댑터** | ZCode · Claude Code · Cursor · MiniMax Code · Codex · DeepSeek Harness · Markdown · JSONL —— 프로세스 격리, 크래시 세이프 |
| 🗄️ **SQLite V16 + WAL** | 25+ 테이블, FTS5 폴백, 시작 시 자동 스키마 마이그레이션 |
| 🔍 **이중 검색 엔진** | Tantivy(N-gram + jieba 선택) 기본, FTS5 폴백, 쿼리 문법, 시간순 히트 |
| 🧬 **표준 정규화** | 6 소스 × 19 이벤트 타입 통합 도메인 모델, BLAKE3 해시, 멱등, 완전도 스코어링 |
| 📦 **콘텐츠 어드레싱 Raw Store** | 모든 원본 페이로드를 그대로 보존해 재추출과 감사에 대비 |
| 🏷️ **워크스페이스 자동 병합** | 7단계 우선순위 + 신뢰도, 수동 오버라이드 UI 제공 |
| ⭐ **즐겨찾기 · 태그 · 아카이브 · 소프트/하드 삭제** | 검색 인덱스와 Raw Store에 걸친 캐스케이드 정리 포함 |
| 🔁 **증분 동기화** | `import_state` 신선도 추적, 10분 자동 동기화, 리셋 후 재동기화, 소스 측 제목 변경은 다음 동기화에 반영(수동 이름 변경이 항상 우선) |
| 💡 **지식 추출** | 기본은 규칙 엔진, 선택 시 LLM 엔진(OpenAI / DeepSeek / GLM / Ollama / LM Studio / llama.cpp), 스키마 버전 관리와 실행 이력 포함 |
| 🔐 **암호화 백업 / 복원** | XChaCha20-Poly1305 + Argon2id, 키는 디스크 밖으로 나가지 않음 |
| 🧪 **유사 세션 추천** | 한정된 후보 집합, 지난 컨텍스트 재발견에 유용 |
| 🧠 **저장된 검색** | 세션 간 지속화, 원샷 재실행 |

### CodeAgentOps 거버넌스

팀과 파워 유저를 위한 두 번째 화면: 사용량, 비용, 보안, 자동화.

- **사용량·비용 수집** —— 4개 에이전트에 걸친 요청 단위 수집
- **대시보드** —— 비용, 캐시 적중률, 지연 시간 P50/P95 추이
- **보안 감사** —— 시크릿 패턴, 위험 명령, HTML 리포트
- **정책·예산·가격** —— 초과 경고와 월말 전망
- **자산 인벤토리** —— 에이전트 전반의 skills / plugins
- **프로젝트 비용 귀속** + 캐시 적중 분석
- **이상 감지** —— 오류 급증, 재시도 폭주, 컨텍스트 오버플로 이벤트
- **에이전트 건강 점수**(0–100, 안정성 가중)
- **토큰 낭비 감지** + 에이전트 간 비교
- **주간 리포트**(HTML, 자동 생성)
- **데이터 라이프사이클** —— 스토리지 대시보드, 고아 blob GC, 리텐션 정책, 인덱스 재구축
- **거버넌스 감사 로그** —— 모든 민감 작업 기록

### 데스크톱 GUI(7개 뷰)

`개요` · `대화`(타임라인 + 대량 작업) · `지식`(세션 간 + 이중 엔진) ·
`활동`(GitHub 스타일 히트맵 + 대화 간트 차트) · `프로젝트` · `Ops`(비용 · 자산 ·
보안 · 자동화) · `설정`.

- Apple 스타일 라이트/다크 테마, 4단계 글꼴 크기(macOS *Larger Text*)
- 영어 / 간체 중국어 UI(930+ 문자열, 설정 → 외관 → 언어)
- 커맨드 팔레트, 온보딩 투어, 첫 실행 시 변경 이력
- 대화별 개인 메모, ⌘1–⌘8 사이드바 고정, 앱 내 하단 터미널 독(⌘J)
- 검색 기록 드롭다운(개별 삭제), 대화 간트 차트, 목록 가로 스크롤, 메시지 내 이미지 네이티브 미리보기
- 같은 제목 대화 접기(배치 실행을 `×N` 한 줄로), 검색 좌측 패널 컨텍스트 메뉴(즐겨찾기 / 아카이브 / 고정 / 태그 / 삭제)

## 빠른 시작

### 데스크톱(개발 모드)

```bash
git clone https://github.com/SunArthurX/threadock.git
cd threadock
cd apps/desktop
npm install
npx tauri dev          # 개발 모드에서도 Dock에 실제 앱 아이콘이 표시됩니다
```

### 전체 빌드

```bash
# CLI + Daemon + Markdown 어댑터
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Tauri 데스크톱
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### 설치 프로그램

`v*` 태그를 push → [릴리스 파이프라인](.github/workflows/release.yml)이
**macOS**(dmg arm64 + x64), **Windows**(nsis + msi), **Linux**(appimage + deb)용
Tauri 설치 프로그램과 4개 플랫폼 CLI 바이너리, SHA256SUMS를 빌드해 게시합니다.

[![GitHub release](https://img.shields.io/github/v/release/SunArthurX/threadock?include_prereleases&style=flat-square)](https://github.com/SunArthurX/threadock/releases)

> 설치 프로그램은 현재**서명되지 않았습니다**(인증서 준비 중). macOS: 우클릭 → 열기
> (또는 `xattr -dr com.apple.quarantine <app>`). Windows: *자세한 정보* → *실행*.
> 반드시 SHA256SUMS를 검증하세요.

### CLI 투어

```bash
CH=./target/release/ch

# 가져오기(형식 자동 감지)
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# 실제 IDE 데이터에서(읽기 전용)
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from zcode <session-id>

# 쿼리 문법 검색
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex 오류 처리'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# 지식 추출
$CH --db ./hub.db knowledge <id>            # 추출 후 표시
$CH --db ./hub.db knowledge <id> --save     # 추출 후 저장(버전 관리)
$CH --db ./hub.db knowledge <id> --show     # 저장된 것 표시
$CH --db ./hub.db similar <id>              # 유사 세션

# 거버넌스 / 백업
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
CH_BACKUP_PASSWORD="..." $CH --db ./hub.db backup hub.chbak

# 데몬(stdio JSON-RPC, 14개 메서드)
$CH --db ./data daemon
```

### Daemon JSON-RPC

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

메서드: `system.getInfo` · `workspace.list` · `conversation.list` · `conversation.get` ·
`conversation.delete` · `conversation.restore` · `conversation.similar` · `message.list` ·
`event.list` · `search.query` · `knowledge.extract` · `knowledge.save` · `knowledge.get` ·
`provider.sync`. 전체 스키마는 [docs/api.md](docs/api.md)를 참고하세요.

## 저장소 구조

```
threadock/
├── crates/
│   ├── domain/              통합 도메인 모델(6 소스 × 19 이벤트 타입)
│   ├── storage/             SQLite V16 + Migration + FTS5 + 거버넌스 테이블
│   ├── raw-store/           콘텐츠 어드레싱 페이로드(BLAKE3 + zstd)
│   ├── search/              Tantivy(N-gram + jieba 선택)
│   ├── identity-resolver/   워크스페이스 자동 병합(7단계 우선순위)
│   ├── normalization/       BLAKE3 해시 + 멱등성 + 완전도
│   ├── export/              Markdown / JSON 내보내기 + 마스킹
│   ├── backup/              XChaCha20-Poly1305 + Argon2id
│   ├── knowledge/           규칙 + LLM 이중 엔진(버전 관리)
│   ├── llm/                 LLM 클라이언트 + AEAD 시크릿 볼트
│   ├── adapter-sdk/         Adapter trait + stdio JSON-RPC 프로토콜
│   ├── adapter-host/        프로세스 격리(spawn / 타임아웃 / 크래시)
│   ├── adapter-markdown/    Markdown 어댑터(독립 바이너리)
│   ├── adapter-jsonl/       JSONL 어댑터
│   ├── adapter-claude-code/ Claude Code 어댑터
│   ├── adapter-zcode/       ZCode 어댑터(SQLite 읽기)
│   ├── adapter-cursor/      Cursor 어댑터(state.vscdb)
│   ├── adapter-minimax/     MiniMax Code 어댑터
│   ├── adapter-codex/       Codex 어댑터
│   ├── adapter-deepseek-harness/ DeepSeek Harness (dsh) 어댑터
│   ├── ops-metrics/         사용량 / 비용 / 건강 지표
│   ├── audit/               보안 감사(시크릿 + 위험 명령)
│   ├── benchmarks/          성능, 검색 지연, 콜드 스타트
│   ├── daemon/              상주 서비스(stdio JSON-RPC)
│   └── cli/                 `ch` 커맨드라인
├── apps/desktop/            Tauri 2 데스크톱(React + TS)
│   ├── src/                 7개 뷰 · 101개 명령 · 90+ SVG 아이콘
│   └── src-tauri/           Rust 백엔드
├── docs/                    기획서, 사용 가이드, 프라이버시, API, 벤치마크
├── fixtures/                익명화 골든 픽스처 + 어댑터 테스트
└── .github/workflows/       CI(3 OS × lint × test)· CodeQL · 릴리스
```

## 개발

### 커밋 전 체크

```bash
scripts/precheck.sh --install-hooks    # 최초 1회: pre-commit=lint, pre-push=test
scripts/precheck.sh                    # 수동: fmt×2 + clippy×2 + tsc + eslint
scripts/precheck.sh test               # lint + 전체 테스트 + 빌드(CI와 동일)
scripts/precheck.sh all                # + cargo audit + MSRV 1.88 검사
```

이 스크립트는 `.github/workflows/ci.yml`을 명령어 단위로 그대로 미러링합니다. `clippy`는
**CI와 동일한 플래그**(`-W clippy::pedantic -D warnings`)로 실행됩니다 —— 로컬에서 플래그 없이
`cargo clippy`를 실행하면 pedantic 위반을 놓칩니다. 긴급 시 회피: `git commit --no-verify`.

### 테스트

```bash
cargo test --workspace                                          # Rust 코어
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # Tauri 명령
cd apps/desktop && npm test                                     # React + TS
cargo clippy --workspace --all-targets                          # pedantic 수준 클린
cargo test -p ch-search --features jieba                        # jieba 토크나이저
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture
```

| | |
|---|---|
| **Gate 1** | 세션 10만 / 메시지 50만 건 FTS5 검색 P95 = **50.9 ms**(목표 300 ms) —— [docs/benchmark-report-v1.0.0.md](docs/benchmark-report-v1.0.0.md) 참고 |
| **워크스페이스 병합 정확도** | 수동 라벨링 11 샘플 = 100%(잘못된 자동 병합 0건) |
| **ESLint** | 0 오류 / 0 경고 |
| **공급망 어드바이저리** | 2건 수정, 1건은 업스트림 블록(RUSTSEC-2026-0253, tantivy 업데이트 대기 중, CI는 허용) |

## 핵심 설계 결정

| 결정 | 상태 | 이유 |
|---|---|---|
| 로컬 우선 | ✅ | 대화 기록은 내 컴퓨터에 남습니다. 계정 없음, 텔레메트리 없음, 업로드 없음 |
| SQLite WAL | ✅(V16) | 단일 파일 내구성, 쉬운 백업, 부하 하에서 예측 가능 |
| Tauri 2 데스크톱 | ✅ | 네이티브 윈도우 + WebView; 데몬을 내장한 단일 Rust 바이너리 |
| Rust 코어 | ✅(MSRV 1.88) | 메모리 안전성 + 제로 코스트 FFI; 품질 기준으로서 pedantic clippy |
| Tantivy 검색 | ✅(N-gram + jieba) | N-gram이 기본(외부 의존성 없음); 중국어 재현율 향상용 jieba 옵트인 |
| 어댑터 프로세스 격리 | ✅ | IDE 어댑터는 읽기 전용이며 별도 프로세스로 실행 —— 버그 있는 어댑터가 호스트를 죽일 수 없음 |
| Raw + 정규화 이중 저장 | ✅ | 원본 페이로드를(BLAKE3 키로) 보존해 재추출 가능; 검색에는 정규화 형식 사용 |
| 서드파티 데이터 읽기 전용 | ✅ | 철칙. `~/.claude`, `state.vscdb` 등에 절대 쓰지 않음 |

전체 배경: [docs/conversation-hub-execution-plan.md §1.3](docs/conversation-hub-execution-plan.md).

## 문서

- [사용 가이드](docs/user-guide.md) —— 설치, 7개 뷰, 동기화, 검색, 지식, 백업
- [프라이버시 성명](docs/privacy.md) —— 무엇을 읽고, 무엇을 저장하고, 하지 **않는** 것
- [API 레퍼런스](docs/api.md) —— 자동 생성된 Tauri 명령 목록 + Daemon JSON-RPC 메서드
- [성능 벤치마크](docs/benchmark-report-v1.0.0.md) —— Gate 1 대규모 결과
- [전체 계획](docs/ai-ide-conversation-hub-enterprise-plan.md) · [실행 계획](docs/conversation-hub-execution-plan.md) · [거버넌스 계획](docs/codeagent-ops-plan.md)
- [UI 개선 리포트](docs/optimization-rounds/REPORT.md) —— 13라운드 × 라운드당 30+ 변경
- [변경 이력](CHANGELOG.md)

## 로드맵

- [ ] OpenCode 어댑터(7번째 IDE 소스)
- [ ] Daemon UDS / Named Pipe IPC + 로컬 인증 토큰(현재: stdio만)
- [ ] 어댑터 호스트 리소스 쿼터(메모리 / CPU / 파일 화이트리스트 / 네트워크 차단)
- [ ] Tauri Updater 자동 업데이트 + 설치 프로그램 코드 서명 / 공증
- [ ] Android 열람 전용 클라이언트(Phase 4 PoC)
- [ ] 엔터프라이즈: SSO / RBAC / KMS / 암호화 동기화(Phase 5)

## 기여

Issue와 PR을 환영합니다. 열기 전에:

1. [docs/conversation-hub-execution-plan.md](docs/conversation-hub-execution-plan.md)을 읽어
   주세요 —— "타협 불가 결정" 섹션이 왜 이런 구조인지 설명합니다.
2. `scripts/precheck.sh test`를 실행하세요 —— CI와 같은 명령입니다.
3. 공개 API(Tauri 명령, CLI 서브커맨드, JSON-RPC 메서드)를 건드리는 변경이라면
   `docs/`의 관련 문서를 업데이트하세요.

큰 변경은 먼저 Issue를 열어 방향을 논의해 주세요. 메인테이너는 프로젝트의 유일한 풀타임
기여자이므로 리뷰에 시간이 걸릴 수 있습니다.

## License

다음 중 하나를 선택하는 듀얼 라이선스:

- [MIT](LICENSE-MIT)
- [Apache 2.0](LICENSE-APACHE)

## 감사의 말

- [Tantivy](https://github.com/quickwit-oss/tantivy) —— 검색 엔진
- [Tauri](https://tauri.app) —— 데스크톱 셸
- [SQLite](https://sqlite.org) —— 스토리지 계층
- [React](https://react.dev), [Vite](https://vitejs.dev), [Vitest](https://vitest.dev) —— 프론트엔드
- [xterm.js](https://xtermjs.org) —— 앱 내 터미널
- ZCode, Claude Code, Cursor, MiniMax Code, Codex, DeepSeek Harness 팀

---

<sub align="center">Built and maintained by <a href="https://github.com/SunArthurX">SunArthurX</a> · 2026</sub>
