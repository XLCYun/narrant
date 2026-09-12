//! 分析器用到的标准 LSP 位置和调用数据；响应校验不依赖任何语言语法。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{failure, Result};

/// 从零开始的 LSP 行列；列的编码由会话协商，默认使用 UTF-16。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// LSP 的左闭右开源码范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// 标准 Location 或 LocationLink 统一后的定义定位信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub uri: String,
    pub position: Position,
}

/// 调用层级中用于定位被调对象的数据；prepare 的完整原始 item 由会话内部保留。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallTarget {
    pub uri: String,
    pub name: String,
    pub selection_range: Range,
}

/// 从查询对象发出的调用及其在调用方源码中的位置。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingCall {
    pub to: CallTarget,
    pub from_ranges: Vec<Range>,
}

/// 校验位置响应，不能将缺失 URI 或非法坐标当作未解析目标。
pub(super) fn locations(value: Value) -> Result<Vec<Location>> {
    let values = match value {
        Value::Null => Vec::new(),
        Value::Array(values) => values,
        Value::Object(_) => vec![value],
        _ => return Err(failure("LSP 位置响应类型无效")),
    };
    values
        .into_iter()
        .map(|value| {
            let (uri, position) = if value.get("targetUri").is_some() {
                (&value["targetUri"], &value["targetSelectionRange"]["start"])
            } else {
                (&value["uri"], &value["range"]["start"])
            };
            Ok(Location {
                uri: uri
                    .as_str()
                    .ok_or_else(|| failure("LSP 位置缺少 URI"))?
                    .to_owned(),
                position: serde_json::from_value(position.clone()).map_err(failure)?,
            })
        })
        .collect()
}

/// 可选列表只允许 null 或数组，协议错误不能变成空关联。
pub(super) fn optional_array<'a>(value: &'a Value, method: &str) -> Result<&'a [Value]> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("LSP {method} 未返回列表")))
}
