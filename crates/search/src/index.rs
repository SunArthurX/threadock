//! Tantivy 索引实现：schema、分词、增删查、重建。

use crate::error::{SearchError, SearchResult as LibResult};
use ch_domain::{Provider, Role, Timestamp};
use std::path::PathBuf;
// Provider::from_str 需要trait 在作用域内（语法 provider: 前缀解析）
use std::str::FromStr as _;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::query::{BooleanQuery, Occur, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, NumericOptions, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, RawTokenizer, TextAnalyzer};
use tantivy::{doc, Index as TantivyIndex, IndexReader, IndexWriter, Order, ReloadPolicy};

/// IndexWriter 默认堆大小（15 MiB）：单 writer 场景的内存/性能平衡点。
pub const DEFAULT_WRITER_HEAP: usize = 15_000_000;

/// 单条命中。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SearchHit {
    pub message_id: String,
    pub conversation_id: String,
    pub provider: Provider,
    pub workspace_id: Option<String>,
    pub role: Role,
    pub title: Option<String>,
    /// 命中片段（已高亮）。
    pub snippet: String,
    /// 相关性得分（时间序模式下为 0，排序依据见 `created_at_ms`）。
    pub score: f32,
    /// 消息时间（unix 毫秒）。时间序模式下的排序键；旧索引（schema 无此
    /// 字段）或未写入时间的消息为 None。
    pub created_at_ms: Option<i64>,
}

/// 查询条件（与 `storage::search::SearchQuery` 对齐）。
///
/// `query` 支持查询语法（plan §13.2）：`provider:` `workspace:` `type:` `role:`
/// 前缀在索引内生效；`status:` `file:` `model:` `after:` `before:` 由调用方
/// 拿 `ParsedQuery` 走 SQLite 后过滤（见 `Repository::search_filter_conversation_ids`）。
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub query: String,
    pub provider: Option<Provider>,
    pub workspace_id: Option<String>,
    /// workspace 名字解析出的多个候选 id（语法 `workspace:` 由调用方解析）。
    pub workspace_ids: Vec<String>,
    /// 角色过滤（如 "user" = 仅用户提问）。
    pub role: Option<String>,
    pub limit: usize,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            provider: None,
            workspace_id: None,
            workspace_ids: Vec::new(),
            role: None,
            limit: 50,
        }
    }
    #[must_use]
    pub fn with_provider(mut self, p: Provider) -> Self {
        self.provider = Some(p);
        self
    }
    #[must_use]
    pub fn with_workspace(mut self, id: impl Into<String>) -> Self {
        self.workspace_id = Some(id.into());
        self
    }
    /// 多候选 workspace（名字解析出多个 id 时 OR 匹配）。
    #[must_use]
    pub fn with_workspace_ids(mut self, ids: Vec<String>) -> Self {
        self.workspace_ids = ids;
        self
    }
    /// 角色过滤（"user" = 仅我的提问）。
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
    #[must_use]
    pub fn with_limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }
}

/// 字段元信息（避免到处传 Field）。
///
/// `created_at_ms` 为时间序排序键（fast 字段）：既有索引 schema 里没有时
/// 为 None——此时搜索退回相关性排序，直到调用方 `recreate` 后重建。
struct SchemaFields {
    message_id: Field,
    conversation_id: Field,
    provider: Field,
    workspace_id: Field,
    role: Field,
    title: Field,
    body: Field,
    created_at_ms: Option<Field>,
}

/// 按名字从「索引实际 schema」解析字段。
///
/// 打开既有索引时 schema 来自 meta.json（代码 schema 仅在建索引时生效），
/// 必须按名解析而非复用代码 Field id——否则 schema 演进（如追加
/// created_at_ms）后 id 与旧索引错位。`created_at_ms` 允许缺席（旧 schema）。
fn resolve_fields(schema: &Schema) -> LibResult<SchemaFields> {
    let need = |name: &str| {
        schema
            .get_field(name)
            .map_err(|_| SearchError::Tantivy(format!("index schema lacks field {name:?}")))
    };
    Ok(SchemaFields {
        message_id: need("message_id")?,
        conversation_id: need("conversation_id")?,
        provider: need("provider")?,
        workspace_id: need("workspace_id")?,
        role: need("role")?,
        title: need("title")?,
        body: need("body")?,
        created_at_ms: schema.get_field("created_at_ms").ok(),
    })
}

/// 中文分词器选择（plan §13.1「分词器可插拔 + N-gram 兜底」）。
///
/// - `NGram`：默认。双字 N-gram，零词典依赖，中文召回稳定。
/// - `Jieba`：需 `jieba` feature。词典分词，准确率高、索引更小；
///   **切换分词器必须重建索引**（旧文档按旧分词写入，混用会导致查询不命中；
///   GUI「设置 → 存储 → 重建索引」或 `rebuild_search_index` 命令）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChineseTokenizer {
    #[default]
    NGram,
    #[cfg(feature = "jieba")]
    Jieba,
}

impl ChineseTokenizer {
    /// 写进 schema 的分词器注册名（决定索引期与查询期用同一套分词）。
    #[must_use]
    pub fn registry_name(self) -> &'static str {
        match self {
            Self::NGram => "ngram",
            #[cfg(feature = "jieba")]
            Self::Jieba => "jieba",
        }
    }
}

fn build_schema(tokenizer: ChineseTokenizer) -> (Schema, SchemaFields) {
    let mut schema_builder = Schema::builder();

    // 文本字段：按选择的中文分词器（默认 ngram），title/body 启用索引与存储
    let text_opts = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(tokenizer.registry_name())
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();

    let id_opts = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        )
        .set_stored();

    // 消息时间（unix 毫秒）：fast 字段，时间倒序排序键。字段句柄一律按名
    // 从索引实际 schema 解析（见 resolve_fields），旧 schema 缺此字段时为
    // None：写入跳过、排序回退相关性，直到 recreate + 重建
    schema_builder.add_i64_field("created_at_ms", NumericOptions::default().set_fast());

    schema_builder.add_text_field("message_id", id_opts.clone());
    schema_builder.add_text_field("conversation_id", id_opts.clone());
    schema_builder.add_text_field("provider", id_opts.clone());
    schema_builder.add_text_field("workspace_id", id_opts.clone());
    schema_builder.add_text_field("role", id_opts);
    schema_builder.add_text_field("title", text_opts.clone());
    schema_builder.add_text_field("body", text_opts);
    let schema = schema_builder.build();
    let fields = resolve_fields(&schema).expect("freshly built schema resolves");
    (schema, fields)
}

/// 注册分词器：ngram（默认）+ raw（ID 精确匹配）+ jieba（可选 feature）。
fn register_tokenizers(index: &TantivyIndex) {
    let ngram = TextAnalyzer::builder(NgramTokenizer::new(2, 2, false).expect("unexpected None"))
        .filter(LowerCaser)
        .build();
    let tokenizers = index.tokenizers();
    tokenizers.register("ngram", ngram);
    // raw 用于精确匹配 ID 类字段
    // raw 必须是整串精确匹配（RawTokenizer 不分词）：id 字段的 delete_term
    // 依赖整串 term 命中。此前误用 SimpleTokenizer，含 '_' 的真实 ID
    //（msg_xxx）被分词 → 删除路径失效（2026-08-15 治理闭环引入时发现）。
    tokenizers.register(
        "raw",
        TextAnalyzer::builder(RawTokenizer::default()).build(),
    );
    #[cfg(feature = "jieba")]
    tokenizers.register(
        "jieba",
        TextAnalyzer::builder(jieba_tokenizer::JiebaTokenizer).build(),
    );
}

/// jieba 分词的 tantivy TokenStream 适配（feature = "jieba" 时编译）。
///
/// 直接适配而不依赖 tantivy-jieba 三方 crate：避免其与 tantivy 0.26 的
/// 版本耦合。词典（~5MB）通过全局 OnceLock 单例共享，Tokenizer 本体是
/// 零大小 unit struct（满足 tantivy 的 Clone bound 且不复制词典）。
#[cfg(feature = "jieba")]
mod jieba_tokenizer {
    use jieba_rs::Jieba;
    use std::sync::OnceLock;
    use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

    static JIEBA: OnceLock<Jieba> = OnceLock::new();

    fn global_jieba() -> &'static Jieba {
        JIEBA.get_or_init(Jieba::new)
    }

    #[derive(Debug, Default, Clone, Copy)]
    pub struct JiebaTokenizer;

    impl Tokenizer for JiebaTokenizer {
        type TokenStream<'a> = JiebaTokenStream<'a>;
        fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
            // Search 模式：对长词再切子词，提升召回；HMM 开启新词发现。
            // 注意 jieba 的 Token.start/end 是 Unicode 字符位置，
            // tantivy 要字节偏移 —— 用 byte_start/byte_end。
            let intervals = global_jieba()
                .tokenize(text, jieba_rs::TokenizeMode::Search, true)
                .into_iter()
                .map(|t| (t.byte_start, t.byte_end))
                .collect::<Vec<_>>();
            JiebaTokenStream {
                text,
                intervals,
                index: 0,
                token: Token::default(),
            }
        }
    }

    pub struct JiebaTokenStream<'a> {
        text: &'a str,
        intervals: Vec<(usize, usize)>,
        index: usize,
        token: Token,
    }

    impl TokenStream for JiebaTokenStream<'_> {
        fn advance(&mut self) -> bool {
            if self.index >= self.intervals.len() {
                return false;
            }
            let (start, end) = self.intervals[self.index];
            self.index += 1;
            self.token = Token {
                offset_from: start,
                offset_to: end,
                position: self.index,
                text: self.text[start..end].to_lowercase(),
                position_length: 1,
            };
            true
        }
        fn token(&self) -> &Token {
            &self.token
        }
        fn token_mut(&mut self) -> &mut Token {
            &mut self.token
        }
    }
}

/// Tantivy 搜索索引。
///
/// 生命周期：open → 多次 `index_message/delete_by_message` → commit → search。
/// 与 `SQLite` 主数据并存；索引可随时从主数据 rebuild（plan §3 Rebuildable index）。
pub struct SearchIndex {
    index: TantivyIndex,
    reader: IndexReader,
    fields: SchemaFields,
    /// 索引目录（`recreate` 时原地重建用）。
    path: Option<PathBuf>,
    /// 创建索引时用的分词器（`recreate` 保持一致）。
    tokenizer: ChineseTokenizer,
}

/// 待索引的一条消息（由调用方从主数据组装）。
#[derive(Debug, Clone)]
pub struct IndexableMessage {
    pub message_id: String,
    pub conversation_id: String,
    pub provider: Provider,
    pub workspace_id: Option<String>,
    pub role: Role,
    pub title: Option<String>,
    pub body: Option<String>,
    /// 消息时间：写入 `created_at_ms` fast 字段，时间倒序排序键。
    /// None（未知/旧索引无此字段）排序时视为最旧。
    pub created_at: Option<Timestamp>,
}

/// `OffsetDateTime` → unix 毫秒（fast 字段精度，足够排序区分消息）。
fn ts_unix_ms(ts: &Timestamp) -> i64 {
    ts.unix_timestamp() * 1000 + i64::from(ts.millisecond())
}

impl SearchIndex {
    /// 打开（或创建）位于 `path` 的持久化索引（默认 N-gram 分词）。
    pub fn open(path: impl AsRef<std::path::Path>) -> LibResult<Self> {
        Self::open_with_tokenizer(path, ChineseTokenizer::NGram)
    }

    /// 打开（或创建）索引并指定中文分词器（plan §13.1 可插拔）。
    ///
    /// 注意：既有索引的 schema 固定了创建时的分词器；切换分词器后必须
    /// 重建索引（delete_all + 重灌），否则新旧文档分词不一致导致漏召回。
    pub fn open_with_tokenizer(
        path: impl AsRef<std::path::Path>,
        tokenizer: ChineseTokenizer,
    ) -> LibResult<Self> {
        let (schema, _) = build_schema(tokenizer);
        let path_buf = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path_buf)?;
        // 判断目录是否已有索引（含 meta.json）
        let meta_exists = path_buf.join("meta.json").exists();
        let index = if meta_exists {
            TantivyIndex::open_in_dir(&path_buf).map_err(|e| SearchError::Tantivy(e.to_string()))?
        } else {
            TantivyIndex::builder()
                .schema(schema)
                .create_in_dir(&path_buf)
                .map_err(|e| SearchError::Tantivy(e.to_string()))?
        };
        register_tokenizers(&index);
        // 字段按名解析自索引实际 schema（旧 schema 缺 created_at_ms → None）
        let fields = resolve_fields(&index.schema())?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        Ok(Self {
            index,
            reader,
            fields,
            path: Some(path_buf),
            tokenizer,
        })
    }

    /// 创建内存索引（主要用于测试，默认 N-gram）。
    pub fn open_in_memory() -> LibResult<Self> {
        Self::open_in_memory_with(ChineseTokenizer::NGram)
    }

    /// 创建指定分词器的内存索引（测试 / jieba 验证用）。
    pub fn open_in_memory_with(tokenizer: ChineseTokenizer) -> LibResult<Self> {
        let (schema, _) = build_schema(tokenizer);
        let index = TantivyIndex::create_in_ram(schema);
        register_tokenizers(&index);
        let fields = resolve_fields(&index.schema())?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        Ok(Self {
            index,
            reader,
            fields,
            path: None,
            tokenizer,
        })
    }

    /// 索引 schema 是否含 `created_at_ms`（时间倒序排序就绪）。
    ///
    /// false = 旧 schema 索引：搜索回退相关性排序；调用方应 `recreate`
    /// 后从主数据全量重灌以启用时间序（GUI 启动时自动做）。
    #[must_use]
    pub fn has_time_field(&self) -> bool {
        self.fields.created_at_ms.is_some()
    }

    /// 原地重建索引目录：删除后按当前代码 schema 新建（空索引）。
    ///
    /// schema 演进（如新增 created_at_ms）后由调用方触发，随后从主数据
    /// 全量重灌。仅支持持久化索引（内存索引没有目录）。unix 语义下删除
    /// 仍被旧 reader 映射的文件是安全的；此方法后旧 writer/reader 全部失效，
    /// 调用方不得再持有。
    pub fn recreate(&mut self) -> LibResult<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| SearchError::Tantivy("in-memory index cannot recreate".into()))?;
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let (schema, _) = build_schema(self.tokenizer);
        std::fs::create_dir_all(&path)?;
        let index = TantivyIndex::builder()
            .schema(schema)
            .create_in_dir(&path)
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        register_tokenizers(&index);
        let fields = resolve_fields(&index.schema())?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        *self = Self {
            index,
            reader,
            fields,
            path: Some(path),
            tokenizer: self.tokenizer,
        };
        Ok(())
    }

    /// 创建一个 writer（调用方负责 commit）。
    pub fn writer(&self, heap_size_bytes: usize) -> LibResult<IndexWriter> {
        self.index
            .writer(heap_size_bytes)
            .map_err(|e| SearchError::Tantivy(e.to_string()))
    }

    /// 索引一条消息（自动按 `message_id` 删除旧版本后插入，保证幂等）。
    pub fn index_message(&self, writer: &mut IndexWriter, m: &IndexableMessage) -> LibResult<()> {
        // 先删旧
        let _ = writer.delete_term(tantivy::Term::from_field_text(
            self.fields.message_id,
            &m.message_id,
        ));
        let f = &self.fields;
        let mut doc = doc!(
            f.message_id => m.message_id.as_str(),
            f.conversation_id => m.conversation_id.as_str(),
            f.provider => m.provider.as_str(),
            f.role => m.role.as_str(),
        );
        if let Some(ws) = &m.workspace_id {
            doc.add_text(f.workspace_id, ws);
        }
        if let Some(t) = &m.title {
            doc.add_text(f.title, t);
        }
        if let Some(b) = &m.body {
            doc.add_text(f.body, b);
        }
        if let (Some(ts_field), Some(ts)) = (f.created_at_ms, m.created_at.as_ref()) {
            doc.add_i64(ts_field, ts_unix_ms(ts));
        }
        writer
            .add_document(doc)
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        Ok(())
    }

    /// 按 `message_id` 删除。
    pub fn delete_message(&self, writer: &mut IndexWriter, message_id: &str) -> LibResult<()> {
        writer.delete_term(tantivy::Term::from_field_text(
            self.fields.message_id,
            message_id,
        ));
        Ok(())
    }

    /// 执行查询。
    pub fn search(&self, q: &SearchQuery) -> LibResult<Vec<SearchHit>> {
        let parsed = ch_domain::query_syntax::parse(&q.query);
        let searcher = self.reader.searcher();
        let f = &self.fields;

        // 构造查询：全文只在 body 上匹配（方案 A）——title 仍入索引供返回展示，
        // 但不参与命中：否则标题含关键词的会话每条消息都算命中，结果全是无高亮噪音
        let query_parser = QueryParser::for_index(&self.index, vec![f.body]);
        // 让用户的裸关键词被当成词组查（更符合直觉）
        let escaped = escape_query(&parsed.text);

        let provider = q.provider.or_else(|| {
            parsed
                .provider
                .as_deref()
                .and_then(|s| Provider::from_str(s).ok())
        });
        let role = q.role.clone().or_else(|| parsed.role.clone());
        let has_filter = provider.is_some()
            || q.workspace_id.is_some()
            || !q.workspace_ids.is_empty()
            || role.is_some();

        let text_query: Box<dyn tantivy::query::Query> = if escaped.is_empty() {
            if has_filter {
                // 纯过滤查询：全量扫描后按过滤条件收窄
                Box::new(tantivy::query::AllQuery)
            } else {
                return Ok(Vec::new());
            }
        } else {
            Box::new(
                query_parser
                    .parse_query(&escaped)
                    .map_err(|e| SearchError::InvalidQuery(e.to_string()))?,
            )
        };

        // 用 BooleanQuery 叠加过滤条件
        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> =
            vec![(Occur::Must, text_query)];

        if let Some(p) = provider {
            let term = tantivy::Term::from_field_text(f.provider, p.as_str());
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if let Some(ws) = &q.workspace_id {
            let term = tantivy::Term::from_field_text(f.workspace_id, ws);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if !q.workspace_ids.is_empty() {
            // 多候选 id：OR 组合后再 Must
            let should: Vec<(Occur, Box<dyn tantivy::query::Query>)> = q
                .workspace_ids
                .iter()
                .map(|ws| {
                    let term = tantivy::Term::from_field_text(f.workspace_id, ws);
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic))
                            as Box<dyn tantivy::query::Query>,
                    )
                })
                .collect();
            clauses.push((
                Occur::Must,
                Box::new(BooleanQuery::new(should)) as Box<dyn tantivy::query::Query>,
            ));
        }
        if let Some(role) = &role {
            let term = tantivy::Term::from_field_text(f.role, role);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        let bool_query = BooleanQuery::new(clauses);
        // 收集器：schema 含 created_at_ms 时按消息时间倒序（全局取 top-N，
        // 不受相关性截断）；旧索引无此字段时回退 BM25 相关性
        //（tantivy 0.26: TopDocs 是 builder，with_limit 返 TopDocs 不 impl
        //  Collector，必须链 order_by_* 才拿到 Collector）
        let hits: Vec<(Option<i64>, tantivy::DocAddress)> = if f.created_at_ms.is_some() {
            let top = TopDocs::with_limit(q.limit)
                .order_by_fast_field::<i64>("created_at_ms", Order::Desc);
            searcher
                .search(&bool_query, &top)
                .map_err(|e| SearchError::Tantivy(e.to_string()))?
        } else {
            let top = TopDocs::with_limit(q.limit).order_by_score();
            searcher
                .search(&bool_query, &top)
                .map_err(|e| SearchError::Tantivy(e.to_string()))?
                .into_iter()
                .map(|(_score, addr)| (None, addr))
                .collect()
        };

        let mut results = Vec::with_capacity(hits.len());
        for (created_at_ms, doc_addr) in hits {
            results.push(hit_from_doc(
                &searcher,
                doc_addr,
                f,
                &parsed.text,
                created_at_ms,
            )?);
        }
        Ok(results)
    }

    /// 重建：清空索引后由调用方重新灌入所有消息（plan §3 可重建）。
    pub fn rebuild<F>(&self, writer: &mut IndexWriter, reindex: F) -> LibResult<usize>
    where
        F: FnOnce(&mut IndexWriter) -> LibResult<usize>,
    {
        writer
            .delete_all_documents()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        reindex(writer)
    }

    /// 清空所有索引文档（用于「重置数据」）。
    pub fn clear_all(&self) -> LibResult<()> {
        let writer = self.writer(DEFAULT_WRITER_HEAP)?;
        writer
            .delete_all_documents()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        self.commit(writer)?;
        Ok(())
    }

    /// 提交并刷新 reader。
    pub fn commit(&self, mut writer: IndexWriter) -> LibResult<()> {
        writer
            .commit()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        self.reader
            .reload()
            .map_err(|e| SearchError::Tantivy(e.to_string()))?;
        Ok(())
    }
}

/// 取回命中文档并组装 SearchHit（时间序模式下无相关性得分，score 恒 0）。
fn hit_from_doc(
    searcher: &tantivy::Searcher,
    doc_addr: tantivy::DocAddress,
    f: &SchemaFields,
    query_text: &str,
    created_at_ms: Option<i64>,
) -> LibResult<SearchHit> {
    let doc: tantivy::TantivyDocument = searcher
        .doc(doc_addr)
        .map_err(|e| SearchError::Tantivy(e.to_string()))?;
    let get = |field: Field| -> Option<String> {
        doc.get_first(field)
            .and_then(|v| v.as_str().map(std::string::ToString::to_string))
    };
    let provider_str = get(f.provider).unwrap_or_default();
    let role_str = get(f.role).unwrap_or_default();
    // 高亮命中片段：取 body 的前若干字符，标记查询词（仅自由文本部分）
    let body = get(f.body).unwrap_or_default();
    let snippet = make_snippet(&body, query_text);
    Ok(SearchHit {
        message_id: get(f.message_id).unwrap_or_default(),
        conversation_id: get(f.conversation_id).unwrap_or_default(),
        provider: provider_str.parse().unwrap_or(Provider::Unknown),
        workspace_id: get(f.workspace_id),
        role: parse_role(&role_str),
        title: get(f.title),
        snippet,
        score: 0.0,
        created_at_ms,
    })
}

fn parse_role(s: &str) -> Role {
    match s {
        "assistant" => Role::Assistant,
        "system" => Role::System,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

/// 转义用户查询，让裸关键词按词组匹配（避免 AND/OR/NOT 等被当语法）。
fn escape_query(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // 用双引号包裹整个输入作为词组查询
    let cleaned: String = trimmed.chars().filter(|c| *c != '"').collect();
    format!("\"{cleaned}\"")
}

/// 生成命中片段：截取 body 中包含查询词的部分，用 » « 标记。
///
/// 按字符（非字节）切分，避免中文边界 panic。
/// 输出经 HTML 转义（前端用 innerHTML 渲染 snippet，防存储型 XSS）。
fn make_snippet(body: &str, query: &str) -> String {
    if body.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = body.chars().collect();
    let window_chars = 20;
    let lower_body: String = chars.iter().collect::<String>().to_lowercase();
    let lower_query = query.to_lowercase();
    if let Some(byte_pos) = lower_body.find(&lower_query) {
        // 把 byte_pos 转成 char 索引
        let char_pos = lower_body[..byte_pos].chars().count();
        let start_char = char_pos.saturating_sub(window_chars / 2);
        let end_char = (char_pos + query.chars().count() + window_chars / 2).min(chars.len());
        let segment: String = chars[start_char..end_char].iter().collect();
        let prefix = if start_char > 0 { "…" } else { "" };
        let suffix = if end_char < chars.len() { "…" } else { "" };
        // 高亮：把 segment 里的 query 部分用 » « 包裹（大小写不敏感替换）
        let highlighted = highlight_ci(&segment, query);
        format!("{prefix}{highlighted}{suffix}")
    } else {
        let end_char = chars.len().min(window_chars);
        let s: String = chars[..end_char].iter().collect();
        format!("{}…", ch_domain::html::escape_html(&s))
    }
}

/// 大小写不敏感地高亮 segment 中的 query（输出 HTML 转义）。
fn highlight_ci(segment: &str, query: &str) -> String {
    use ch_domain::html::escape_html;
    let lower_seg = segment.to_lowercase();
    let lower_q = query.to_lowercase();
    if let Some(pos) = lower_seg.find(&lower_q) {
        let mut result = String::new();
        result.push_str(&escape_html(&segment[..pos]));
        result.push('»');
        result.push_str(&escape_html(&segment[pos..pos + query.len()]));
        result.push('«');
        result.push_str(&escape_html(&segment[pos + query.len()..]));
        result
    } else {
        escape_html(segment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::Provider;

    fn msg(id: &str, conv: &str, title: &str, body: &str) -> IndexableMessage {
        IndexableMessage {
            message_id: id.into(),
            conversation_id: conv.into(),
            provider: Provider::Generic,
            workspace_id: None,
            role: Role::User,
            title: Some(title.into()),
            body: Some(body.into()),
            created_at: None,
        }
    }

    fn index_samples(idx: &SearchIndex, msgs: &[IndexableMessage]) {
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        for m in msgs {
            idx.index_message(&mut writer, m).expect("file I/O failed");
        }
        idx.commit(writer).expect("file I/O failed");
    }

    #[test]
    fn index_and_search_basic() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[
                msg("m1", "c1", "Tauri 讨论", "如何用 Tauri 做 Android 后台任务"),
                msg("m2", "c2", "Rust 错误", "thiserror 和 anyhow 的选择"),
            ],
        );

        let hits = idx
            .search(&SearchQuery::new("tauri"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].conversation_id, "c1");
        assert!(hits[0].snippet.contains("»"));
    }

    #[test]
    fn search_chinese_keyword() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[msg(
                "m1",
                "c1",
                "后台任务",
                "讨论 Android 后台任务的实现方案",
            )],
        );
        // 中文双字查询
        let hits = idx
            .search(&SearchQuery::new("后台任务"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty());
    }

    #[test]
    fn search_body_only_title_match_not_hit() {
        // 方案 A：消息全文搜索只匹配正文 —— 标题含关键词、正文不含的消息不算命中
        //（此前 title 参与 OR 匹配：标题含「白板」的会话每条消息都命中，snippet 无高亮）
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[
                msg("m1", "c1", "白板手绘风格", "文件结构清晰，先看看图的风格"),
                msg("m2", "c1", "无关标题", "我们用白板梳理一下流程"),
            ],
        );
        let hits = idx
            .search(&SearchQuery::new("白板"))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 1, "只有正文含「白板」的消息应命中");
        assert_eq!(hits[0].message_id, "m2");
    }

    #[test]
    fn search_no_match_returns_empty() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(&idx, &[msg("m1", "c1", "x", "hello world")]);
        let hits = idx
            .search(&SearchQuery::new("zzznotexist"))
            .expect("SQL execution failed");
        assert!(hits.is_empty());
    }

    #[test]
    fn filter_by_provider() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        idx.index_message(
            &mut writer,
            &IndexableMessage {
                message_id: "m1".into(),
                conversation_id: "c1".into(),
                provider: Provider::Codex,
                workspace_id: None,
                role: Role::User,
                title: Some("t".into()),
                body: Some("search keyword here".into()),
                created_at: None,
            },
        )
        .expect("unexpected None");
        idx.index_message(
            &mut writer,
            &IndexableMessage {
                message_id: "m2".into(),
                conversation_id: "c2".into(),
                provider: Provider::Cursor,
                workspace_id: None,
                role: Role::User,
                title: Some("t".into()),
                body: Some("search keyword here".into()),
                created_at: None,
            },
        )
        .expect("unexpected None");
        idx.commit(writer).expect("file I/O failed");

        let only_codex = idx
            .search(&SearchQuery::new("keyword").with_provider(Provider::Codex))
            .expect("unexpected None");
        assert_eq!(only_codex.len(), 1);
        assert_eq!(only_codex[0].provider, Provider::Codex);
    }

    #[test]
    fn filter_by_workspace() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        let mut m1 = msg("m1", "c1", "t", "findme text");
        m1.workspace_id = Some("ws1".into());
        let mut m2 = msg("m2", "c2", "t", "findme text");
        m2.workspace_id = Some("ws2".into());
        idx.index_message(&mut writer, &m1)
            .expect("file I/O failed");
        idx.index_message(&mut writer, &m2)
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let in_ws1 = idx
            .search(&SearchQuery::new("findme").with_workspace("ws1"))
            .expect("unexpected None");
        assert_eq!(in_ws1.len(), 1);
        assert_eq!(in_ws1[0].workspace_id.as_deref(), Some("ws1"));
    }

    #[test]
    fn reindex_same_message_is_idempotent() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let m = msg("m1", "c1", "t", "unique content");
        index_samples(&idx, std::slice::from_ref(&m));
        // 再次索引同 message_id（应替换而非重复）
        index_samples(&idx, std::slice::from_ref(&m));
        let hits = idx
            .search(&SearchQuery::new("unique"))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 1, "reindex should replace not duplicate");
    }

    #[test]
    fn delete_removes_from_index_realistic_id() {
        // 回归：含 '_' 的真实 ID（msg_abc-123）必须能被 delete_term 删除
        //（旧实现误用 SimpleTokenizer 分词导致删除失效）
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[msg("msg_abc-123_def", "c1", "t", "deletable realistic")],
        );

        let mut writer = idx.writer(DEFAULT_WRITER_HEAP).expect("file I/O failed");
        idx.delete_message(&mut writer, "msg_abc-123_def")
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("realistic"))
            .expect("SQL execution failed");
        assert!(hits.is_empty(), "realistic id delete must work");
    }

    #[test]
    fn delete_removes_from_index() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(&idx, &[msg("m1", "c1", "t", "deletable content")]);

        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        idx.delete_message(&mut writer, "m1")
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("deletable"))
            .expect("SQL execution failed");
        assert!(hits.is_empty());
    }

    #[test]
    fn rebuild_clears_and_refills() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(&idx, &[msg("old1", "c1", "t", "old content")]);

        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        idx.rebuild(&mut writer, |w| {
            idx.index_message(w, &msg("new1", "c2", "t", "new content"))?;
            Ok(1)
        })
        .expect("unexpected None");
        idx.commit(writer).expect("file I/O failed");

        let old_hits = idx
            .search(&SearchQuery::new("old"))
            .expect("SQL execution failed");
        assert!(old_hits.is_empty(), "old docs should be cleared");
        let new_hits = idx
            .search(&SearchQuery::new("new"))
            .expect("SQL execution failed");
        assert_eq!(new_hits.len(), 1);
    }

    #[test]
    fn snippet_highlights_match() {
        let s = make_snippet("前面一些文字 后台任务 后面一些文字", "后台任务");
        assert!(s.contains("»后台任务«"));
    }

    #[test]
    fn snippet_escapes_html() {
        // 防存储型 XSS：正文里的 HTML 不能原样进入 snippet
        let s = make_snippet("keyword <img src=x onerror=alert(1)> tail", "keyword");
        assert!(!s.contains("<img"), "raw HTML must not survive: {s}");
        assert!(s.contains("&lt;img"), "HTML must be escaped: {s}");
    }

    #[test]
    fn snippet_escapes_html_on_truncation_path() {
        // 截断路径（查询词不命中）也要转义
        let s = make_snippet("<script>alert(1)</script> no match", "zzz");
        assert!(!s.contains("<script"), "raw HTML must not survive: {s}");
    }

    #[test]
    fn empty_query_returns_empty() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(&idx, &[msg("m1", "c1", "t", "anything")]);
        let hits = idx
            .search(&SearchQuery::new(""))
            .expect("SQL execution failed");
        assert!(hits.is_empty());
    }

    #[test]
    fn persistence_across_open() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let path = dir.path().join("idx");

        // 第一次：建索引
        {
            let idx = SearchIndex::open(&path).expect("unexpected None");
            index_samples(&idx, &[msg("m1", "c1", "persist", "persistent content")]);
        }
        // 第二次：重新打开应能查到
        {
            let idx = SearchIndex::open(&path).expect("unexpected None");
            let hits = idx
                .search(&SearchQuery::new("persistent"))
                .expect("SQL execution failed");
            assert_eq!(hits.len(), 1);
        }
    }

    #[test]
    fn limit_respected() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let msgs: Vec<_> = (0..10)
            .map(|i| msg(&format!("m{i}"), "c1", "t", "shared keyword"))
            .collect();
        index_samples(&idx, &msgs);
        let hits = idx
            .search(&SearchQuery::new("keyword").with_limit(3))
            .expect("unexpected None");
        assert!(hits.len() <= 3);
    }

    #[test]
    fn tokenizer_choice_names() {
        assert_eq!(ChineseTokenizer::NGram.registry_name(), "ngram");
        assert_eq!(ChineseTokenizer::default(), ChineseTokenizer::NGram);
    }

    // jieba 分词器端到端（cargo test -p ch-search --features jieba）
    #[cfg(feature = "jieba")]
    #[test]
    fn jieba_tokenizer_chinese_search() {
        let idx =
            SearchIndex::open_in_memory_with(ChineseTokenizer::Jieba).expect("unexpected None");
        index_samples(
            &idx,
            &[msg(
                "m1",
                "c1",
                "数据库连接池",
                "讨论 PostgreSQL 连接池泄漏的排查方案",
            )],
        );
        // 词典分词：整词查询应命中
        let hits = idx
            .search(&SearchQuery::new("连接池"))
            .expect("SQL execution failed");
        assert!(!hits.is_empty(), "jieba should match whole word 连接池");
        // 二级子词也应命中（Search 模式切子词）
        let sub = idx
            .search(&SearchQuery::new("泄漏"))
            .expect("SQL execution failed");
        assert!(!sub.is_empty(), "jieba Search mode should match sub-words");
    }

    #[test]
    fn filter_by_role_user_only() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[
                msg("m1", "c1", "t", "user asks about feature"),
                IndexableMessage {
                    message_id: "m2".into(),
                    conversation_id: "c1".into(),
                    provider: Provider::Generic,
                    workspace_id: None,
                    role: Role::Assistant,
                    title: Some("t".into()),
                    body: Some("assistant answers feature".into()),
                    created_at: None,
                },
            ],
        );
        let hits = idx
            .search(&SearchQuery::new("feature").with_role("user"))
            .expect("unexpected None");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.role == Role::User));
    }

    // ── 查询语法测试（plan §13.2，索引内生效的 provider/type）─────────

    #[test]
    fn syntax_provider_prefix_in_index() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        let mut m1 = msg("m1", "c1", "t", "keyword here");
        m1.provider = Provider::Codex;
        let mut m2 = msg("m2", "c2", "t", "keyword here");
        m2.provider = Provider::Cursor;
        idx.index_message(&mut writer, &m1)
            .expect("file I/O failed");
        idx.index_message(&mut writer, &m2)
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("provider:codex keyword"))
            .expect("unexpected None");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].provider, Provider::Codex);
    }

    #[test]
    fn syntax_type_role_prefix_in_index() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        index_samples(
            &idx,
            &[
                msg("m1", "c1", "t", "user asks feature"),
                IndexableMessage {
                    message_id: "m2".into(),
                    conversation_id: "c1".into(),
                    provider: Provider::Generic,
                    workspace_id: None,
                    role: Role::Assistant,
                    title: Some("t".into()),
                    body: Some("assistant answers feature".into()),
                    created_at: None,
                },
            ],
        );
        let hits = idx
            .search(&SearchQuery::new("type:assistant feature"))
            .expect("unexpected None");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.role == Role::Assistant));
    }

    #[test]
    fn syntax_pure_filter_matches_all_scope() {
        // 纯过滤（无关键词）：provider 过滤下全量扫描该 provider 的消息
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        let mut m1 = msg("m1", "c1", "t", "anything one");
        m1.provider = Provider::Codex;
        let mut m2 = msg("m2", "c2", "t", "anything two");
        m2.provider = Provider::Cursor;
        idx.index_message(&mut writer, &m1)
            .expect("file I/O failed");
        idx.index_message(&mut writer, &m2)
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("provider:codex"))
            .expect("unexpected None");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "m1");
    }

    #[test]
    fn workspace_ids_multi_or() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        let mut m1 = msg("m1", "c1", "t", "findme");
        m1.workspace_id = Some("ws1".into());
        let mut m2 = msg("m2", "c2", "t", "findme");
        m2.workspace_id = Some("ws2".into());
        let mut m3 = msg("m3", "c3", "t", "findme");
        m3.workspace_id = Some("ws3".into());
        for m in [&m1, &m2, &m3] {
            idx.index_message(&mut writer, m).expect("file I/O failed");
        }
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(
                &SearchQuery::new("findme").with_workspace_ids(vec!["ws1".into(), "ws2".into()]),
            )
            .expect("unexpected None");
        assert_eq!(hits.len(), 2);
    }

    // ── 时间倒序排序（created_at_ms fast 字段）────────────────────────

    fn ts(secs: i64) -> Timestamp {
        Timestamp::from_unix_timestamp(secs).expect("timestamp out of range")
    }

    #[test]
    fn search_orders_by_time_desc() {
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        // 故意让最旧的消息词频最高（相关性最强）：时间序必须压过相关性
        let old = IndexableMessage {
            created_at: Some(ts(1_700_000_000)),
            ..msg("m_old", "c_old", "旧", "配置 配置 配置 配置 最早的消息")
        };
        let mid = IndexableMessage {
            created_at: Some(ts(1_750_000_000)),
            ..msg("m_mid", "c_mid", "中", "配置 中间的消息")
        };
        let new = IndexableMessage {
            created_at: Some(ts(1_800_000_000)),
            ..msg("m_new", "c_new", "新", "配置 最新的消息")
        };
        for m in [&old, &mid, &new] {
            idx.index_message(&mut writer, m).expect("file I/O failed");
        }
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("配置"))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 3, "三条都应命中");
        let ids: Vec<&str> = hits.iter().map(|h| h.message_id.as_str()).collect();
        assert_eq!(ids, vec!["m_new", "m_mid", "m_old"], "按消息时间倒序");
        assert!(hits[0].created_at_ms > hits[1].created_at_ms);
    }

    #[test]
    fn time_order_not_truncated_by_relevance() {
        // 时间序是「全局 top-N by 时间」：低相关但最新的消息必须排第一，
        // 而不是先被相关性截断再排序
        let idx = SearchIndex::open_in_memory().expect("unexpected None");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        for i in 0..30 {
            let m = IndexableMessage {
                created_at: Some(ts(1_700_000_000 + i)),
                ..msg(&format!("m{i}"), "c1", "噪音", "配置 配置 配置 配置")
            };
            idx.index_message(&mut writer, &m).expect("file I/O failed");
        }
        let target = IndexableMessage {
            created_at: Some(ts(1_900_000_000)),
            ..msg("newest", "c2", "目标", "正文很长只出现一次配置")
        };
        idx.index_message(&mut writer, &target)
            .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");

        let hits = idx
            .search(&SearchQuery::new("配置").with_limit(5))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 5);
        assert_eq!(hits[0].message_id, "newest", "最新消息必须在时间序首位");
    }

    /// 旧 schema（无 created_at_ms）索引：打开不炸、搜索走相关性回退；
    /// `recreate` 后 schema 更新、可重灌启用时间序——GUI 启动迁移依赖此链路。
    #[test]
    fn legacy_index_falls_back_then_recreate_upgrades() {
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        let path = dir.path().join("idx");

        // 1. 用「旧 schema」（7 字段，无 created_at_ms）手工建索引并灌一条
        {
            let mut b = Schema::builder();
            let text_opts = TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("ngram")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored();
            let id_opts = TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("raw")
                        .set_index_option(IndexRecordOption::Basic),
                )
                .set_stored();
            let message_id = b.add_text_field("message_id", id_opts.clone());
            b.add_text_field("conversation_id", id_opts.clone());
            b.add_text_field("provider", id_opts.clone());
            b.add_text_field("workspace_id", id_opts.clone());
            b.add_text_field("role", id_opts);
            b.add_text_field("title", text_opts.clone());
            let body = b.add_text_field("body", text_opts);
            std::fs::create_dir_all(&path).expect("mkdir");
            let index = TantivyIndex::builder()
                .schema(b.build())
                .create_in_dir(&path)
                .expect("create");
            register_tokenizers(&index);
            let mut w = index.writer(15_000_000).expect("writer");
            w.add_document(doc!(message_id => "legacy1", body => "历史遗留消息"))
                .expect("add doc");
            w.commit().expect("commit");
        }

        // 2. 打开：无时间字段 → 搜索仍工作（相关性回退），命中无时间
        let mut idx = SearchIndex::open(&path).expect("unexpected None");
        assert!(!idx.has_time_field(), "旧 schema 不含时间字段");
        let hits = idx
            .search(&SearchQuery::new("遗留"))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].created_at_ms, None, "回退模式命中无时间");

        // 3. recreate：schema 升级、文档清空 → 重灌后时间序生效
        idx.recreate().expect("recreate");
        assert!(idx.has_time_field());
        let empty = idx
            .search(&SearchQuery::new("遗留"))
            .expect("SQL execution failed");
        assert!(empty.is_empty(), "recreate 后索引为空");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        idx.index_message(
            &mut writer,
            &IndexableMessage {
                created_at: Some(ts(1_800_000_000)),
                ..msg("fresh1", "c1", "新", "重灌后的遗留消息")
            },
        )
        .expect("file I/O failed");
        idx.commit(writer).expect("file I/O failed");
        let hits = idx
            .search(&SearchQuery::new("遗留"))
            .expect("SQL execution failed");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "fresh1");
        assert!(hits[0].created_at_ms.is_some(), "时间随文档写入");
    }
}
