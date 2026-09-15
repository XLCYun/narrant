//! 固定 TypeScript 会话：显式项目查询、来源校验和 UTF-16 证据标准化。
use super::{
    failure,
    project::{AnalysisNote, PreparedProject},
    text, Result,
};
use crate::lsp::{CancellationCheck, LspClient, LspOptions, Position, Range};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

/// 已同时校验原文 UTF-16 边界和字节边界的闭开范围。
#[derive(Clone, Debug)]
pub(crate) struct CheckedRange {
    pub range: Range,
    pub start_byte: u32,
    pub end_byte: u32,
}

/// 第一方 URI 使用原始侧身份；path 仅用于会话续查。
#[derive(Clone, Debug)]
pub(crate) struct CheckedLocation {
    pub path: PathBuf,
    pub uri: String,
    pub unverified: bool,
    pub span: CheckedRange,
}

/// tsserver 的未验证定义可能指向不存在的文件，不能作为已读取的源码位置。
#[derive(Clone, Debug)]
pub(crate) enum DefinitionEvidence {
    Checked(CheckedLocation),
    Unverified,
}

/// 调用层级对象保留名称与完整范围，语义层自行匹配语法声明。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct HierarchyItem {
    pub name: String,
    pub location: CheckedLocation,
    pub full_span: CheckedRange,
}

/// 调用位置始终位于调用方文件；incoming 的调用方为 target，outgoing 为查询文件。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct CallEvidence {
    pub target: HierarchyItem,
    pub from_spans: Vec<CheckedRange>,
}

/// 会话借用 Git workspace；字段顺序保证先回收子进程，再清理自身工具文件。
pub(crate) struct TypeScriptLsp {
    client: Option<LspClient>,
    project: PreparedProject,
    diagnosed: BTreeSet<(PathBuf, PathBuf)>,
    diagnosed_options: BTreeSet<PathBuf>,
    /// 原生诊断指出类型专用导入/导出被用作值；不能因 definition 越过别名而伪造调用。
    runtime_type_errors: BTreeMap<(PathBuf, PathBuf), Vec<CheckedRange>>,
}
impl TypeScriptLsp {
    /// 配置注册完成后才打开文档，逐一验证真实项目与成员并收集显式诊断。
    pub fn start(
        project: PreparedProject,
        timeout: Duration,
        cancelled: CancellationCheck,
    ) -> Result<Self> {
        let mut command = project.toolchain.command();
        command
            .arg(
                project
                    .toolchain
                    .runtime_root
                    .join("typescript-language-server/lib/cli.mjs"),
            )
            .arg("--stdio");
        let options = LspOptions {
            initialization_options: json!({"disableAutomaticTypingAcquisition":true, "plugins":[], "tsserver":{"path":project.server_entry(), "useSyntaxServer":"never"}}),
            request_timeout: timeout,
            cancelled,
            ..LspOptions::default()
        };
        let client =
            LspClient::start(command, &project.workspace_root, options).map_err(lsp_failure)?;
        let mut session = Self {
            client: Some(client),
            project,
            diagnosed: BTreeSet::new(),
            diagnosed_options: BTreeSet::new(),
            runtime_type_errors: BTreeMap::new(),
        };
        for (index, registration) in session
            .project
            .registrations
            .clone()
            .into_iter()
            .enumerate()
        {
            let alias = if registration
                .name
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                session
                    .project
                    .workspace_root
                    .join(format!(".narrant-registration-{index}"))
            } else {
                registration.name
            };
            session.raw("openExternalProject", json!({"projectFileName":alias, "rootFiles":registration.roots.iter().map(|file| json!({"fileName":file})).collect::<Vec<_>>(), "options":registration.options, "typeAcquisition":{"enable":false}}), false)?;
        }
        for (file_index, file) in session.project.files.clone().into_iter().enumerate() {
            let snapshot = session
                .project
                .sources
                .get(&file.file)
                .ok_or_else(|| failure("缺少选中源码快照"))?;
            let extension = file
                .file
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let language = match extension {
                "tsx" => "typescriptreact",
                "jsx" => "javascriptreact",
                "js" | "cjs" | "mjs" => "javascript",
                _ => "typescript",
            };
            session
                .client
                .as_mut()
                .expect("已启动会话")
                .open_document(
                    &crate::lsp::file_uri(&file.file).map_err(failure)?,
                    language,
                    1,
                    &snapshot.source,
                )
                .map_err(lsp_failure)?;
            let configured = session
                .project
                .registrations
                .iter()
                .filter(|registration| session.project.config_files.contains(&registration.name))
                .map(|registration| registration.name.clone())
                .collect::<Vec<_>>();
            let mut actual = Vec::new();
            for name in configured {
                if session.inspect_project(&file.file, &name)?.is_some() {
                    actual.push(name);
                }
            }
            if actual.is_empty() {
                actual = file.projects;
                if let Some(registration) =
                    session.project.registrations.iter().find(|registration| {
                        actual.contains(&registration.name)
                            && !session.project.config_files.contains(&registration.name)
                    })
                {
                    if session.project.notes.len() < 200 {
                        session.project.notes.push(AnalysisNote {
                            code: Some("typescript.inferred_project".into()),
                            project: Some(registration.name.clone()),
                            file: file.file.clone(),
                            message: format!(
                                "未配置文件使用 {}/{} ES2022 推断配置",
                                registration.options["module"].as_str().unwrap_or("未知"),
                                registration.options["moduleResolution"]
                                    .as_str()
                                    .unwrap_or("未知")
                            ),
                        });
                    }
                }
            }
            session.project.files[file_index].projects = actual.clone();
            for name in &actual {
                session.validate_project(&file.file, name)?;
                session.collect_notes(&file.file, name)?;
            }
        }
        Ok(session)
    }

    /// 语义层借用同一个 Git workspace 的路径与内存快照。
    pub fn project(&self) -> &PreparedProject {
        &self.project
    }

    /// 统一错误出口：RPC、取消、格式或范围失败立即回收会话，而非等待上层析构。
    fn guarded<T>(&mut self, action: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let result = action(self);
        if result.is_err() {
            self.client.take();
        }
        result
    }

    /// raw 通道固定 semantic 路由；服务错误和缺失 body 不能降级成空列表。
    fn raw(&mut self, command: &str, arguments: Value, body_required: bool) -> Result<Value> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| failure("TypeScript 会话已经关闭"))?;
        let response = client.request("workspace/executeCommand", json!({"command":"typescript.tsserverRequest", "arguments":[command, arguments, {"executionTarget":0, "expectsResult":true, "isAsync":false}]}));
        self.project.check_access()?;
        let response = response.map_err(|mut error| {
            error.message = format!("tsserver {command}：{}", error.message);
            lsp_failure(error)
        })?;
        response_body(command, response, body_required)
    }

    /// 每次语义请求都验证实际项目名，避免 tsserver 对错误 projectFileName 静默回退。
    pub fn validate_project(&mut self, file: &Path, name: &Path) -> Result<Vec<PathBuf>> {
        self.guarded(|session| session.validate_project_inner(file, name))
    }

    /// 校验 projectInfo 的真实身份、服务状态及所有源码成员；配置路径单独校验。
    fn validate_project_inner(&mut self, file: &Path, name: &Path) -> Result<Vec<PathBuf>> {
        let file = self.project.checked_path(file)?;
        if !self
            .project
            .registrations
            .iter()
            .any(|registration| registration.name == name)
        {
            return Err(failure("查询使用未知项目名或注册别名"));
        }
        self.inspect_project(&file, name)?
            .ok_or_else(|| failure("TypeScript 实际项目与请求不符或请求文件不属于项目"))
    }

    /// 配置解析只能提供根文件；显式查询补全经 imports/references 加入的选中成员。
    /// 探测得到的回退项目不产生证据，正式查询仍要求请求名与实际名完全相同。
    fn inspect_project(&mut self, file: &Path, name: &Path) -> Result<Option<Vec<PathBuf>>> {
        let file = self.project.checked_path(file)?;
        let body = self.raw(
            "projectInfo",
            json!({"file":file, "projectFileName":name, "needFileNameList":true}),
            true,
        )?;
        let actual = body["configFileName"]
            .as_str()
            .ok_or_else(|| failure("projectInfo 缺少真实项目名"))?;
        if body["languageServiceDisabled"] != false {
            return Err(failure(format!("TypeScript 服务禁用或状态无效：{body}")));
        }
        let matches = Path::new(actual) == name;
        let members = body["fileNames"]
            .as_array()
            .ok_or_else(|| failure("projectInfo 缺少 fileNames"))?;
        let mut checked = Vec::new();
        for member in members {
            let path = Path::new(
                member
                    .as_str()
                    .ok_or_else(|| failure("projectInfo 成员不是路径"))?,
            );
            if self
                .project
                .config_files
                .iter()
                .any(|config| config == path)
            {
                if !path
                    .canonicalize()
                    .map_err(failure)?
                    .starts_with(&self.project.workspace_root)
                {
                    return Err(failure("项目配置路径越界"));
                }
                continue;
            }
            checked.push(self.project.checked_path(path)?);
        }
        if !matches || !checked.contains(&file) {
            return Ok(None);
        }
        Ok(Some(checked))
    }

    /// 转换 checked 零基坐标为协议的一基坐标，并显式绑定项目。
    fn query_arguments(&mut self, file: &Path, position: Position, name: &Path) -> Result<Value> {
        self.validate_project_inner(file, name)?;
        text::byte_offset(&self.project.source(file)?, &position)?;
        Ok(
            json!({"file":file, "line":position.line.checked_add(1).ok_or_else(|| failure("行号溢出"))?, "offset":position.character.checked_add(1).ok_or_else(|| failure("列号溢出"))?, "projectFileName":name}),
        )
    }

    /// 定义证据始终带原始 URI；空数组是唯一的成功空定义响应。
    pub fn definitions(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<DefinitionEvidence>> {
        self.locations("definition", file, position, name)
    }

    /// 联合成员调用的 definition 可能只给出选中签名，类型定义补充同名成员的完整候选。
    pub fn type_definitions(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<DefinitionEvidence>> {
        self.locations("typeDefinition", file, position, name)
    }

    /// 未验证定义只保留不确定性；已验证定义共用严格项目与源码位置检查。
    fn locations(
        &mut self,
        command: &str,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<DefinitionEvidence>> {
        self.guarded(|session| {
            let arguments = session.query_arguments(file, position, name)?;
            let response = session.raw(command, arguments, true)?;
            let result = array(&response, command)?
                .iter()
                .map(|value| {
                    // 相对资源和缺失模块可返回 unverified；协议不保证这些文件存在。
                    if definition_unverified(value)? {
                        return Ok(DefinitionEvidence::Unverified);
                    }
                    let file = Path::new(
                        value["file"]
                            .as_str()
                            .ok_or_else(|| failure("定义缺少 file"))?,
                    );
                    session
                        .location(file, value)
                        .map(DefinitionEvidence::Checked)
                })
                .collect::<Result<Vec<_>>>()?;
            session.validate_project_inner(file, name)?;
            Ok(result)
        })
    }

    /// 返回指定项目的层级入口，供需要从重载或复合声明继续查询的语义层使用。
    #[cfg(test)]
    pub fn prepare_hierarchy(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<HierarchyItem>> {
        self.guarded(|session| {
            let arguments = session.query_arguments(file, position, name)?;
            let response = session.raw("prepareCallHierarchy", arguments, true)?;
            let values = if response.is_object() {
                vec![response]
            } else {
                array(&response, "prepareCallHierarchy")?.to_vec()
            };
            let items = values
                .iter()
                .map(|value| session.hierarchy_item(value))
                .collect::<Result<Vec<_>>>()?;
            session.validate_project_inner(file, name)?;
            Ok(items)
        })
    }

    /// 查询出边，每个 from_span 都在当前查询文件中校验。
    #[cfg(test)]
    pub fn outgoing_calls(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<CallEvidence>> {
        self.calls(file, position, name, false)
    }

    /// 查询入边，每个 from_span 都在返回的调用方文件中校验。
    #[cfg(test)]
    pub fn incoming_calls(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
    ) -> Result<Vec<CallEvidence>> {
        self.calls(file, position, name, true)
    }

    /// 出入边共用明确项目选择和严格 span 校验，不使用标准 LSP 的隐式默认项目。
    #[cfg(test)]
    fn calls(
        &mut self,
        file: &Path,
        position: Position,
        name: &Path,
        incoming: bool,
    ) -> Result<Vec<CallEvidence>> {
        self.guarded(|session| {
            let arguments = session.query_arguments(file, position, name)?;
            let command = if incoming {
                "provideCallHierarchyIncomingCalls"
            } else {
                "provideCallHierarchyOutgoingCalls"
            };
            let response = session.raw(command, arguments, true)?;
            let result = array(&response, command)?
                .iter()
                .map(|value| {
                    let target =
                        session.hierarchy_item(&value[if incoming { "from" } else { "to" }])?;
                    let caller = if incoming {
                        target.location.path.as_path()
                    } else {
                        file
                    };
                    let source = session.project.source(caller)?;
                    let from_spans = array(&value["fromSpans"], "fromSpans")?
                        .iter()
                        .map(|span| checked_range(&source, span))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(CallEvidence { target, from_spans })
                })
                .collect::<Result<Vec<_>>>()?;
            session.validate_project_inner(file, name)?;
            Ok(result)
        })
    }

    /// 层级选择范围及完整范围都必须落在同一份受控源码中。
    #[cfg(test)]
    fn hierarchy_item(&self, value: &Value) -> Result<HierarchyItem> {
        let file = Path::new(
            value["file"]
                .as_str()
                .ok_or_else(|| failure("调用对象缺少 file"))?,
        );
        let location = self.location(file, &value["selectionSpan"])?;
        let full_span = checked_range(&self.project.source(file)?, &value["span"])?;
        if location.span.start_byte < full_span.start_byte
            || location.span.end_byte > full_span.end_byte
        {
            return Err(failure("调用对象选择范围超出完整范围"));
        }
        Ok(HierarchyItem {
            name: value["name"]
                .as_str()
                .ok_or_else(|| failure("调用对象缺少名称"))?
                .to_owned(),
            location,
            full_span,
        })
    }

    /// 所有位置先验证范围和来源，再生成原始身份 URI。
    fn location(&self, file: &Path, span: &Value) -> Result<CheckedLocation> {
        let path = self.project.checked_path(file)?;
        let unverified = definition_unverified(span)?;
        let span = checked_range(&self.project.source(&path)?, span)?;
        let uri = self.project.source_uri(&path)?;
        Ok(CheckedLocation {
            path,
            uri,
            unverified,
            span,
        })
    }

    /// 每个文件/项目最多收集一次诊断，明确查询防止最近通知覆盖其他文件信息。
    pub fn collect_notes(&mut self, file: &Path, name: &Path) -> Result<()> {
        self.guarded(|session| {
            session.validate_project_inner(file, name)?;
            if !session
                .diagnosed
                .insert((file.to_path_buf(), name.to_path_buf()))
            {
                return Ok(());
            }
            for command in [
                "compilerOptionsDiagnostics-full",
                "syntacticDiagnosticsSync",
                "semanticDiagnosticsSync",
            ] {
                if command == "compilerOptionsDiagnostics-full"
                    && !session.diagnosed_options.insert(name.to_path_buf())
                {
                    continue;
                }
                let diagnostics = session.raw(
                    command,
                    json!({"file":file, "projectFileName":name, "includeLinePosition":true}),
                    true,
                )?;
                let mut batches = vec![(file.to_path_buf(), diagnostics)];
                // TS 的 compilerOptionsDiagnostics-full 丢弃附带配置文件的诊断，必须显式查询配置。
                if command == "compilerOptionsDiagnostics-full"
                    && session.project.config_files.contains(&name.to_path_buf())
                {
                    session.project.checked_path(name)?;
                    let diagnostics = session.raw(
                        "semanticDiagnosticsSync",
                        json!({"file":name,"projectFileName":name,"includeLinePosition":true}),
                        true,
                    )?;
                    batches.push((name.to_path_buf(), diagnostics));
                }
                for (diagnostic_file, diagnostics) in batches {
                    for (index, diagnostic) in array(&diagnostics, command)?.iter().enumerate() {
                        let code = diagnostic["code"]
                            .as_u64()
                            .ok_or_else(|| failure("诊断缺少 code"))?;
                        if matches!(code, 1361 | 1362 | 2693) {
                            let start = diagnostic
                                .get("startLocation")
                                .or_else(|| diagnostic.get("start"));
                            let end = diagnostic
                                .get("endLocation")
                                .or_else(|| diagnostic.get("end"));
                            let start = start.ok_or_else(|| failure("运行时类型错误缺少起点"))?;
                            let end = end.ok_or_else(|| failure("运行时类型错误缺少终点"))?;
                            let span = checked_range(
                                &session.project.source(&diagnostic_file)?,
                                &json!({"start":start,"end":end}),
                            )?;
                            session
                                .runtime_type_errors
                                .entry((diagnostic_file.clone(), name.to_path_buf()))
                                .or_default()
                                .push(span);
                        }
                        if index >= 40 {
                            continue;
                        }
                        let message = diagnostic
                            .get("text")
                            .or_else(|| diagnostic.get("message"))
                            .or_else(|| diagnostic.get("messageText"))
                            .ok_or_else(|| failure("诊断缺少说明"))?;
                        let message = diagnostic_message(message)?;
                        if session.project.notes.len() < 200 {
                            let original = session
                                .project
                                .sources
                                .get(&diagnostic_file)
                                .map(|source| source.path.clone())
                                .or_else(|| session.project.metadata.get(&diagnostic_file).cloned())
                                .unwrap_or_else(|| diagnostic_file.clone());
                            session.project.notes.push(AnalysisNote {
                                code: Some(format!("typescript.TS{code}")),
                                project: Some(name.to_path_buf()),
                                file: original,
                                message: format!(
                                    "{} TS{code}: {message}",
                                    name.strip_prefix(&session.project.workspace_root)
                                        .map_err(failure)?
                                        .display()
                                )
                                .chars()
                                .take(4096)
                                .collect(),
                            });
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// 按真实项目和原始位置检查原生诊断，不用导入名称猜测词法绑定。
    pub fn runtime_type_error(&self, file: &Path, offset: u32, name: &Path) -> bool {
        self.runtime_type_errors
            .get(&(file.to_path_buf(), name.to_path_buf()))
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|range| range.start_byte <= offset && offset < range.end_byte)
            })
    }

    /// 完成 shutdown/exit 后销毁客户端；即使关闭失败也先回收进程再清理工具文件。
    pub fn shutdown(mut self) -> Result<()> {
        self.client
            .take()
            .ok_or_else(|| failure("TypeScript 会话已经关闭"))?
            .shutdown()
            .map_err(lsp_failure)
    }
}

/// 只接受协议明确的布尔标记；缺省表示服务提供了已验证的定义。
fn definition_unverified(value: &Value) -> Result<bool> {
    match value.get("unverified") {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| failure("定义 unverified 标记无效")),
    }
}

/// raw 响应是 tsserver Response 而非 body；拒绝错误 command 和不成功的响应。
fn response_body(command: &str, response: Value, body_required: bool) -> Result<Value> {
    if response["type"] != "response"
        || response["command"] != command
        || response["success"] != true
    {
        return Err(failure(format!(
            "tsserver {command} 响应失败或格式无效：{response}"
        )));
    }
    match response.get("body") {
        Some(body) => Ok(body.clone()),
        None if !body_required => Ok(Value::Null),
        None => Err(failure(format!("tsserver {command} 缺少 body"))),
    }
}

/// 严格列表协议；null 和对象不能被当成可靠的空目标。
fn array<'a>(value: &'a Value, command: &str) -> Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("tsserver {command} 缺少数组")))
}

/// 递归诊断链仍是纯文字元数据；不将畸形对象转换成空说明。
fn diagnostic_message(value: &Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_owned());
    }
    let mut message = value["messageText"]
        .as_str()
        .ok_or_else(|| failure("无效的诊断说明"))?
        .to_owned();
    if let Some(next) = value.get("next") {
        for item in array(next, "diagnostic.next")? {
            message.push('\n');
            message.push_str(&diagnostic_message(item)?);
        }
    }
    Ok(message)
}

/// TypeScript 的一基 line/offset 必须为正整数且对应有效 UTF-16 边界。
fn raw_position(value: &Value) -> Result<Position> {
    let convert = |key: &str| {
        value[key]
            .as_u64()
            .and_then(|value| value.checked_sub(1))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| failure("tsserver 位置不是有效的一基坐标"))
    };
    Ok(Position {
        line: convert("line")?,
        character: convert("offset")?,
    })
}

/// 校验闭开区间的方向、行列与字节偏移，拒绝截断或反向范围。
fn checked_range(source: &str, value: &Value) -> Result<CheckedRange> {
    let range = Range {
        start: raw_position(&value["start"])?,
        end: raw_position(&value["end"])?,
    };
    let start_byte = text::byte_offset(source, &range.start)?;
    let end_byte = text::byte_offset(source, &range.end)?;
    if start_byte > end_byte {
        return Err(failure("tsserver 范围起点晚于终点"));
    }
    Ok(CheckedRange {
        range,
        start_byte,
        end_byte,
    })
}

/// 适配共享传输错误时保留取消标记，禁止变成成功的空图。
fn lsp_failure(error: crate::lsp::LspError) -> super::super::code_graph::AnalysisError {
    if error.is_cancelled() {
        super::super::code_graph::AnalysisError::cancelled(error.message)
    } else {
        failure(error.message)
    }
}

#[cfg(test)]
#[path = "tests/lsp.rs"]
mod tests;
