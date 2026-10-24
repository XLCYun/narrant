//! 单次最终 JSON 的严格 schema 与递归重复键检查。

use super::{failure, Result};
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

pub(super) const PROTOCOL: &str = "codegraph.analyzer/1";
pub(super) const REQUEST_ID: &str = "analysis-1";

/// 所有对象字段递归检查重复键，不能先交给 Value 静默覆盖。
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    /// 保留 serde_json 自带的嵌套深度约束，并拒绝每一层对象的重复键。
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        /// JSON 值访问器只接受 JSON 的六类数据。
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            /// 描述格式错误时预期的协议数据。
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("无重复键的 JSON")
            }
            /// JSON 布尔值保持原值。
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            /// JSON 有符号整数保持精度。
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            /// JSON 无符号整数保持精度。
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            /// JSON 浮点数必须有限。
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("非有限数值"))
            }
            /// JSON 字符串由解码器校验 UTF-8。
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            /// null 保留为显式空值。
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            /// 数组递归经过同一严格解码器。
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            /// 在插入前检查键是否存在，包含 extensions 内的嵌套对象。
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("重复 JSON 键：{key}")));
                    }
                    let UniqueValue(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

/// 已验证成功分支的候选数据，尚未经过公共来源及图检查。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Completed {
    pub protocol: String,
    pub engine_version: String,
    pub pack_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub outcome: String,
    pub graph: Graph,
}

/// 后端明确失败分支，不能混入成功数据。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Failed {
    pub protocol: String,
    pub engine_version: String,
    pub pack_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: Nullable,
    pub outcome: String,
    pub error: BackendError,
}

/// CodeGraph 返回的错误只包含公共错误码与有界消息。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackendError {
    pub code: String,
    pub message: String,
}

/// 图构造数据只有虚拟根子对象和节点集合。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Graph {
    pub root_contains: Vec<String>,
    pub nodes: Vec<Node>,
    pub external_symbols: Vec<ExternalSymbol>,
}

/// 每个最终 External 目标的显示名称及真实来源范围。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExternalSymbol {
    pub symbol: String,
    pub name: String,
    pub scope: ExternalScope,
}

/// 来源由 CodeGraph 的解析证据确定，消费者不通过不透明 ID 猜测。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExternalScope {
    Repository,
    Dependency,
}

/// 显式可空字段的包装，缺少字段与发送 null 不等价。
#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct Nullable(pub Option<String>);

/// 六种公共类别只由 category 字段决定。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Category {
    Container,
    Type,
    Callable,
    Value,
    ExecutionRegion,
    Extension,
}

/// 节点的每个字段必须出现，extensions 只保留语言语义。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Node {
    pub category: Category,
    pub id: String,
    pub language: String,
    pub native_kind: String,
    pub name: Nullable,
    pub signature: Nullable,
    pub code: Nullable,
    pub source: Nullable,
    pub contains: Vec<String>,
    pub references: Vec<Reference>,
    pub extensions: BTreeMap<String, Value>,
}

/// 关系事实由语义和确切目标构成。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    pub semantic: String,
    pub target: Target,
}

/// 互斥目标分支不能混用 id、symbol 和 name。
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Target {
    Node { id: String },
    External { symbol: String },
    Unresolved { name: String },
}

/// 最终消息仅有成功与明确失败两个互斥分支。
pub(super) enum Message {
    Ok(Completed),
    Error(Failed),
}

/// 严格 JSON 解码后按 type/outcome 选择互斥 schema，拒绝额外字段。
pub(super) fn decode(bytes: &[u8]) -> Result<Message> {
    let value = unique_json(bytes)?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(failure("不支持的 CodeGraph 协议"));
    }
    if value.get("type").and_then(Value::as_str) == Some("completed") && value.get("id").is_none() {
        return Err(failure("completed 缺少请求 ID 字段"));
    }
    let message = match value.get("type").and_then(Value::as_str) {
        Some("completed") => match value.get("outcome").and_then(Value::as_str) {
            Some("ok") => {
                if let Some(nodes) = value.pointer("/graph/nodes").and_then(Value::as_array) {
                    for node in nodes {
                        for key in ["name", "signature", "code", "source"] {
                            if node.get(key).is_none() {
                                return Err(failure(format!("节点缺少必需字段 {key}")));
                            }
                        }
                    }
                }
                Message::Ok(serde_json::from_value(value).map_err(failure)?)
            }
            Some("error") => Message::Error(serde_json::from_value(value).map_err(failure)?),
            _ => return Err(failure("未知 completed outcome")),
        },
        _ => return Err(failure("未知 CodeGraph 消息类型")),
    };
    Ok(message)
}

/// 控制文件与消息使用同一重复键边界，不允许 JSON 静默覆盖身份。
pub(crate) fn unique_json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice::<UniqueValue>(bytes)
        .map(|v| v.0)
        .map_err(failure)
}
