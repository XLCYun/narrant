//! 类型专用别名路线由词法绑定和原始模块语法确认，不依赖可被 nocheck 屏蔽的诊断。
use super::Resolver;
use crate::structure_review::typescript_analyzer::{
    lsp::{CheckedLocation, DefinitionEvidence, TypeScriptLsp},
    syntax::{aliases::Alias, Occurrence},
    text, Result,
};
use oxc_span::Span;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

impl Resolver {
    /// Oxc root_binding 直接区分 import 与局部 shadow；缓存按项目和完整成员路径隔离。
    pub(super) fn type_only_binding(
        &mut self,
        session: &mut TypeScriptLsp,
        file: &Path,
        occurrence: &Occurrence,
        project: &Path,
    ) -> Result<bool> {
        let Some(binding) = occurrence.root_binding else {
            return Ok(false);
        };
        let key = (
            file.to_path_buf(),
            project.to_path_buf(),
            binding.start,
            occurrence.members.clone(),
        );
        if let Some(result) = self.alias_cache.get(&key) {
            return Ok(*result);
        }
        let alias = self.documents[file]
            .aliases
            .iter()
            .find(|a| a.import && a.binding == Some(binding))
            .cloned();
        let blocked = if let Some(alias) = alias {
            self.alias_route(
                session,
                file,
                &alias,
                &occurrence.members,
                project,
                &mut BTreeSet::new(),
            )?
        } else {
            false
        };
        self.alias_cache.insert(key, blocked);
        Ok(blocked)
    }

    /// 类型专用路段不可重新变成值；循环或过深的公开路线保持保守未知。
    fn alias_route(
        &mut self,
        session: &mut TypeScriptLsp,
        file: &Path,
        alias: &Alias,
        members: &[String],
        project: &Path,
        visited: &mut BTreeSet<(PathBuf, u32, Vec<String>)>,
    ) -> Result<bool> {
        if alias.type_only {
            return Ok(true);
        }
        let marker = alias.source.or(alias.binding).unwrap_or(alias.scope).start;
        if visited.len() >= 32 || !visited.insert((file.to_path_buf(), marker, members.to_vec())) {
            return Ok(true);
        }
        if let Some(source) = alias.source {
            let names = if alias.name == "*" {
                members.to_vec()
            } else {
                std::iter::once(alias.name.clone())
                    .chain(members.iter().cloned())
                    .collect()
            };
            if names.is_empty() {
                return Ok(false);
            }
            let position = text::position(&self.documents[file].source, source.start)?;
            let modules = session.definitions(file, position, project)?;
            if modules.is_empty() {
                return Ok(true);
            }
            for definition in modules {
                let DefinitionEvidence::Checked(module) = definition else {
                    return Ok(true);
                };
                if !self.load(session, &module)? {
                    return Ok(true);
                }
                if self.export_route(session, &module, &names, project, &mut visited.clone())? {
                    return Ok(true);
                }
            }
        } else if let Some(binding) = alias.binding {
            let imported = self.documents[file]
                .aliases
                .iter()
                .find(|a| a.import && a.binding == Some(binding))
                .cloned();
            if let Some(imported) = imported {
                return self.alias_route(session, file, &imported, members, project, visited);
            }
        }
        Ok(false)
    }

    /// 公开名字逐段映射；同一函数的 type-only 和 value 重命名出口不能混为一条路线。
    fn export_route(
        &mut self,
        session: &mut TypeScriptLsp,
        module: &CheckedLocation,
        names: &[String],
        project: &Path,
        visited: &mut BTreeSet<(PathBuf, u32, Vec<String>)>,
    ) -> Result<bool> {
        let document = &self.documents[&module.view_path];
        let scope = document
            .declared_at(module.span.start_byte)
            .filter(|d| matches!(d.node.data().native_kind.as_str(), "module" | "namespace"))
            .and_then(|d| {
                d.parts
                    .iter()
                    .find(|p| {
                        p.span.start <= module.span.start_byte
                            && module.span.start_byte < p.span.end
                    })
                    .map(|p| p.span)
            })
            .unwrap_or(Span::new(0, document.source.len() as u32));
        let exports = document
            .aliases
            .iter()
            .filter(|a| !a.import && a.scope == scope)
            .collect::<Vec<_>>();
        let exact = exports
            .iter()
            .filter(|a| a.exported.as_ref() == names.first())
            .map(|a| (*a).clone())
            .collect::<Vec<_>>();
        let wildcard = exact.is_empty();
        let candidates = if wildcard {
            exports
                .into_iter()
                .filter(|a| a.exported.is_none())
                .cloned()
                .collect()
        } else {
            exact
        };
        for alias in candidates {
            let remaining = if wildcard { names } else { &names[1..] };
            if self.alias_route(
                session,
                &module.view_path,
                &alias,
                remaining,
                project,
                &mut visited.clone(),
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
