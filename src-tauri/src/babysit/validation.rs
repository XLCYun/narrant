use super::{BabysitError, BabysitProviderRequest, FileSide, LineTarget};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// Provider 与前端共用的紧凑 before/after 行解释结构。
///
/// Provider 协议使用字符串行号，因为 JSON 对象的 key 必须是字符串；内部校验
/// 仍然会把它们严格解析为一基行号，保证不会把错误 key 静默投影到 UI。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitFileExplanations {
    pub before: BTreeMap<String, String>,
    pub after: BTreeMap<String, String>,
}

/// 严格解析 provider JSON，拒绝未知顶层字段和重复行 key。
///
/// 生产 Codex adapter 会使用这个入口解析结构化输出；不能直接依赖 serde 默认的
/// Map 行为，因为默认行为会把重复 key 的前一个值静默覆盖掉。
pub fn parse_provider_response(json: &str) -> Result<BabysitFileExplanations, BabysitError> {
    serde_json::from_str(json).map_err(|error| BabysitError::InvalidProviderOutput {
        detail: format!("provider JSON 无效：{error}"),
    })
}

impl<'de> Deserialize<'de> for BabysitFileExplanations {
    /// 从 provider JSON 反序列化完整 before/after map，并交给严格 visitor 校验。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(ExplanationsVisitor)
    }
}

/// `BabysitFileExplanations` 的严格顶层 map visitor。
struct ExplanationsVisitor;

impl<'de> Visitor<'de> for ExplanationsVisitor {
    type Value = BabysitFileExplanations;

    /// 声明顶层响应必须是包含 before 和 after 的解释对象。
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("包含 before 和 after 的解释对象")
    }

    /// 读取两个文件侧，同时拒绝未知字段、重复字段和缺失字段。
    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut before = None;
        let mut after = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "before" if before.is_none() => before = Some(map.next_value::<UniqueLineMap>()?.0),
                "after" if after.is_none() => after = Some(map.next_value::<UniqueLineMap>()?.0),
                "before" | "after" => return Err(de::Error::custom(format!("重复的 {key} 字段"))),
                _ => return Err(de::Error::custom(format!("未知字段 {key}"))),
            }
        }
        let before = before.ok_or_else(|| de::Error::missing_field("before"))?;
        let after = after.ok_or_else(|| de::Error::missing_field("after"))?;
        Ok(BabysitFileExplanations { before, after })
    }
}

/// 保留 JSON map 中行号 key 的唯一性。
struct UniqueLineMap(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for UniqueLineMap {
    /// 从 JSON 行号 map 反序列化，并保留重复 key 的检测能力。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(UniqueLineMapVisitor)
    }
}

/// `UniqueLineMap` 的 map visitor；重复 key 会在反序列化阶段失败。
struct UniqueLineMapVisitor;

impl<'de> Visitor<'de> for UniqueLineMapVisitor {
    type Value = UniqueLineMap;

    /// 声明 map 的 value 必须是行号到解释文本的映射。
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("行号到解释文本的对象")
    }

    /// 读取每个行号解释，拒绝重复行号 key。
    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("重复的行号 key {key}")));
            }
            let value = map.next_value::<String>()?;
            values.insert(key, value);
        }
        Ok(UniqueLineMap(values))
    }
}

/// 后端把 provider 输出拒绝原因限制为不含源代码的稳定错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationFailure {
    DuplicateTarget { side: FileSide, line_number: usize },
    MissingLine { side: FileSide, line_number: usize },
    UnexpectedLine { side: FileSide, line_number: String },
    InvalidLineNumber { side: FileSide, line_number: String },
    EmptyExplanation { side: FileSide, line_number: usize },
}

impl fmt::Display for ValidationFailure {
    /// 将校验失败转换为不含源代码的 repair 诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTarget { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行目标重复")
            }
            Self::MissingLine { side, line_number } => {
                write!(formatter, "{side:?} 缺少第 {line_number} 行解释")
            }
            Self::UnexpectedLine { side, line_number } => {
                write!(formatter, "{side:?} 出现未请求的第 {line_number} 行解释")
            }
            Self::InvalidLineNumber { side, line_number } => {
                write!(formatter, "{side:?} 行号 {line_number} 无效")
            }
            Self::EmptyExplanation { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行解释为空")
            }
        }
    }
}

/// 对 provider 返回的 before/after map 执行严格 coverage 与文本安全校验。
pub fn validate_response(
    request: &BabysitProviderRequest,
    output: &BabysitFileExplanations,
) -> Result<(), ValidationFailure> {
    validate_target_inventory_for_response(&request.targets)?;
    validate_side(FileSide::Before, &request.targets, &output.before)?;
    validate_side(FileSide::After, &request.targets, &output.after)
}

/// 检查 provider 请求中的 target identity 是否出现重复。
fn validate_target_inventory_for_response(targets: &[LineTarget]) -> Result<(), ValidationFailure> {
    let mut seen = HashSet::new();
    for target in targets {
        if !seen.insert((target.side, target.line_number)) {
            return Err(ValidationFailure::DuplicateTarget {
                side: target.side,
                line_number: target.line_number,
            });
        }
    }
    Ok(())
}

/// 校验一侧 map 的 exact coverage、key 格式和解释安全约束。
fn validate_side(
    side: FileSide,
    targets: &[LineTarget],
    explanations: &BTreeMap<String, String>,
) -> Result<(), ValidationFailure> {
    let expected = targets
        .iter()
        .filter(|target| target.side == side)
        .map(|target| target.line_number.to_string())
        .collect::<HashSet<_>>();
    for key in explanations.keys() {
        if key.parse::<usize>().ok().filter(|line| *line > 0).is_none() {
            return Err(ValidationFailure::InvalidLineNumber {
                side,
                line_number: key.clone(),
            });
        }
        if !expected.contains(key) {
            return Err(ValidationFailure::UnexpectedLine {
                side,
                line_number: key.clone(),
            });
        }
    }
    if let Some(missing) = expected
        .iter()
        .find(|line_number| !explanations.contains_key(*line_number))
    {
        return Err(ValidationFailure::MissingLine {
            side,
            line_number: missing.parse().unwrap_or_default(),
        });
    }
    for (key, explanation) in explanations {
        let line_number = key.parse::<usize>().unwrap_or_default();
        if explanation.trim().is_empty() {
            return Err(ValidationFailure::EmptyExplanation { side, line_number });
        }
    }
    Ok(())
}

/// 丢弃 provider 自行添加的 key，并一次收集所有需要局部补写的目标行。
pub(super) fn normalize_and_collect_repairs(
    request: &BabysitProviderRequest,
    output: &mut BabysitFileExplanations,
) -> Vec<ValidationFailure> {
    let expected = request
        .targets
        .iter()
        .map(|target| (target.side, target.line_number))
        .collect::<HashSet<_>>();
    canonicalize_requested_line_keys(&mut output.before, FileSide::Before, &expected);
    canonicalize_requested_line_keys(&mut output.after, FileSide::After, &expected);
    request
        .targets
        .iter()
        .filter_map(|target| {
            let explanations = match target.side {
                FileSide::Before => &output.before,
                FileSide::After => &output.after,
            };
            match explanations.get(&target.line_number.to_string()) {
                None => Some(ValidationFailure::MissingLine {
                    side: target.side,
                    line_number: target.line_number,
                }),
                Some(value) if value.trim().is_empty() => {
                    Some(ValidationFailure::EmptyExplanation {
                        side: target.side,
                        line_number: target.line_number,
                    })
                }
                Some(_) => None,
            }
        })
        .collect()
}

/// 把 provider 从编号源码抄回的 `0001` 归一化为契约使用的 `1`。
///
/// 同一行若同时出现多个等价 key，则丢弃这组歧义结果，让既有 repair 流程重新生成，
/// 避免静默选择其中一条解释。
fn canonicalize_requested_line_keys(
    explanations: &mut BTreeMap<String, String>,
    side: FileSide,
    expected: &HashSet<(FileSide, usize)>,
) {
    let mut canonical = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut ambiguous = HashSet::new();
    for (key, value) in std::mem::take(explanations) {
        let Some(line_number) = key.parse::<usize>().ok().filter(|line| *line > 0) else {
            continue;
        };
        if !expected.contains(&(side, line_number)) || ambiguous.contains(&line_number) {
            continue;
        }
        let canonical_key = line_number.to_string();
        if seen.insert(line_number) {
            canonical.insert(canonical_key, value);
        } else {
            canonical.remove(&canonical_key);
            ambiguous.insert(line_number);
        }
    }
    *explanations = canonical;
}

/// 只接受本轮明确请求的修复行，避免 patch 改写上一轮已经有效的解释。
pub(super) fn merge_repair_patch(
    output: &mut BabysitFileExplanations,
    patch: BabysitFileExplanations,
    failures: &[ValidationFailure],
) {
    for failure in failures {
        let (side, line_number) = match failure {
            ValidationFailure::MissingLine { side, line_number }
            | ValidationFailure::EmptyExplanation { side, line_number } => (*side, *line_number),
            _ => continue,
        };
        let key = line_number.to_string();
        let value = match side {
            FileSide::Before => patch.before.get(&key),
            FileSide::After => patch.after.get(&key),
        };
        if let Some(value) = value {
            match side {
                FileSide::Before => &mut output.before,
                FileSide::After => &mut output.after,
            }
            .insert(key, value.clone());
        }
    }
}
