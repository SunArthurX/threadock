//! 知识提取，对应 plan §13.5「AI 知识提取」。
//!
//! ## 设计
//!
//! plan §13.5 要求：自动摘要、技术决策、TODO、错误与解决方案、关键命令、涉及文件。
//! 并强调：默认关闭/显式启用、可选本地或云模型、生成结果有来源引用、不覆盖原始对话。
//!
//! MVP 实现一套**规则 + 模板**的确定性提取引擎（不依赖外部 LLM，可充分测试）。
//! 未来可替换为 LLM 调用——只需让 [`Extractor`] trait 的另一个实现走模型 API，
//! [`ExtractionResult`] 契约保持不变（plan §13.5 的输出结构）。
//!
//! ## 提取规则
//!
//! - **summary**：取首条 user 消息 + assistant 最长回复拼接（启发式）。
//! - **todos**：匹配 `TODO`/`FIXME`/`待办`/`需要` 等关键词的句子。
//! - **commands**：来自 Command 事件 + 消息中的 `` `code` `` 反引号块。
//! - **errors**：匹配 `error`/`错误`/`failed`/`panic` 的句子 + Error 事件。
//! - **decisions**：匹配 `决定`/`选用`/`结论`/`应该` 的句子（决策性表述）。
//! - **files**：从 Diff/File 事件 + 消息中的路径模式提取。

pub mod extract;
pub mod llm;
pub mod model;
pub mod similar;

pub use extract::{RuleExtractor, TODOS};
pub use llm::{LlmExtractor, PROMPT_VERSION};
pub use model::{Decision, ErrorItem, ExtractionInput, ExtractionResult, FileRef, TodoItem};
pub use similar::{conversation_text, find_similar, ConversationText, SimilarHit};
