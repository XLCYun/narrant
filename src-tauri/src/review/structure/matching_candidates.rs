//! 改名与移动候选的保守证据；只辅助配对，不决定代码是否修改。

use std::collections::HashSet;

use super::super::extraction::fingerprint_tokens;
use super::super::extraction_model::ExtractedDeclaration;

/// 指纹叶子保持原始文字，相似度不删除字符串空白或改写换行。
pub(super) struct CandidateEvidence {
    leaves: usize,
    anchors: HashSet<(String, String)>,
    shingles: HashSet<Vec<(String, String)>>,
}

impl CandidateEvidence {
    /// 对未匹配声明预计算一次，避免在候选笛卡尔积内重复解码。
    pub(super) fn new(declaration: &ExtractedDeclaration) -> Self {
        let tokens = fingerprint_tokens(&declaration.body_tokens);
        let anchors = tokens
            .iter()
            .filter(|(kind, text)| {
                kind.contains("identifier")
                    || kind.contains("string")
                    || kind.contains("integer")
                    || kind.contains("number")
                    || kind.contains("float")
                    || kind.contains("char")
                    || (!text.is_empty()
                        && text.chars().all(|character| character.is_ascii_digit()))
            })
            .cloned()
            .collect();
        Self {
            leaves: tokens.len(),
            anchors,
            shingles: tokens.windows(3).map(<[_]>::to_vec).collect(),
        }
    }

    /// 独特且非空的完全相同正文可以提供配对证据，空壳声明不可以。
    pub(super) fn is_substantive(&self) -> bool {
        self.leaves >= 3 && !self.anchors.is_empty()
    }

    /// 编辑过的正文需同时有词法锚点与连续 token 片段支持，不能只凭相似语法骨架。
    pub(super) fn resembles(&self, other: &Self) -> bool {
        let anchors = self.anchors.intersection(&other.anchors).count();
        let shingles = self.shingles.intersection(&other.shingles).count();
        anchors >= 2
            && self.anchors.len().min(other.anchors.len()) >= 3
            && 2 * anchors * 100 >= 60 * (self.anchors.len() + other.anchors.len())
            && shingles >= 2
            && 2 * shingles * 100 >= 55 * (self.shingles.len() + other.shingles.len())
    }
}
