use super::*;
use crate::{
    lsp::{test_support::FakeServer, Position},
    structure_review::typescript_analyzer::lsp::{DefinitionEvidence, TypeScriptLsp},
};
use std::sync::Arc;

/// 通过真实 stdio 记录请求；已知项目含两个源码文件，第三个文件不属于该项目。
fn session(
    replies: impl FnOnce(&Path) -> Vec<(&'static str, Value)>,
) -> (FakeServer, TypeScriptLsp) {
    let server = FakeServer::with_workspace(|root| {
        let root = root.canonicalize().unwrap();
        for (file, source) in [
            ("main.ts", "export function main() {}"),
            ("worker.ts", "export function work() {}"),
            ("unused.ts", "export function unused() {}"),
            ("tsconfig.json", r#"{"files":["main.ts","worker.ts"]}"#),
        ] {
            std::fs::write(root.join(file), source).unwrap();
        }
        let mut steps = vec![
            vec![json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}})],
            vec![],
            vec![response(2, "openExternalProject", Value::Null)],
            vec![],
        ];
        let mut responses = vec![
            (
                "projectInfo",
                json!({"configFileName":root.join("tsconfig.json"),"languageServiceDisabled":false,
                    "fileNames":[root.join("main.ts"),root.join("worker.ts")]}),
            ),
            ("semanticDiagnosticsSync", json!([])),
        ];
        responses.extend(replies(&root));
        let shutdown_id = responses.len() + 3;
        for (index, (command, body)) in responses.into_iter().enumerate() {
            steps.push(vec![response(index + 3, command, body)]);
        }
        steps.push(vec![
            json!({"jsonrpc":"2.0","id":shutdown_id,"result":null}),
        ]);
        steps
    });
    let root = server.workspace().canonicalize().unwrap();
    let support = tempfile::tempdir().unwrap();
    let name = root.join("tsconfig.json");
    let project = PreparedProject {
        support_root: support.path().to_path_buf(),
        _directory: support,
        workspace_root: root.clone(),
        toolchain: TypeScriptToolchain {
            node: server.workspace().join("server"),
            runtime_root: root.join("runtime"),
        },
        config_files: vec![name.clone()],
        registrations: vec![ProjectRegistration {
            name: name.clone(),
            roots: vec![root.join("main.ts"), root.join("worker.ts")],
            options: json!({}),
        }],
        files: vec![FileProjects {
            file: root.join("main.ts"),
            projects: vec![name.clone()],
        }],
        allowed_roots: vec![root.clone()],
        source_paths: ["main.ts", "worker.ts", "unused.ts"]
            .into_iter()
            .map(|file| root.join(file))
            .collect(),
        metadata: BTreeSet::from([name]),
        loaded_sources: RefCell::new(BTreeMap::new()),
    };
    let session =
        TypeScriptLsp::start(project, Duration::from_secs(5), Arc::new(|| false)).unwrap();
    (server, session)
}

/// 返回 tsserver 原生正文，外层仍通过通用 LSP 请求匹配。
fn response(id: usize, command: &str, body: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{
        "type":"response","command":command,"success":true,"body":body
    }})
}

/// 相同项目的重复和跨文件查询只加载一次成员，空诊断也缓存，未确认定义仍保留。
#[test]
fn queries_reuse_project_members_and_empty_diagnostics() {
    let (server, mut session) = session(|root| {
        let definition = json!({"file":root.join("worker.ts"),
            "start":{"line":1,"offset":17},"end":{"line":1,"offset":21}});
        vec![
            (
                "definition",
                json!([definition.clone(), {"file":root.join("missing.ts"),"unverified":true}]),
            ),
            ("definition", json!([definition])),
            ("semanticDiagnosticsSync", json!([])),
        ]
    });
    let root = session.project().workspace_root.clone();
    let project = root.join("tsconfig.json");
    for file in ["main.ts", "worker.ts"] {
        let definitions = session
            .definitions(&root.join(file), Position::default(), &project)
            .unwrap();
        let DefinitionEvidence::Checked(location) = &definitions[0] else {
            panic!("定义应保留已校验的位置");
        };
        assert_eq!(location.path, root.join("worker.ts"));
        assert_eq!((location.span.start_byte, location.span.end_byte), (16, 20));
        if file == "main.ts" {
            assert!(matches!(definitions[1], DefinitionEvidence::Unverified));
        }
        for _ in 0..2 {
            session
                .check_runtime_type_usage(&root.join(file), &project)
                .unwrap();
            assert!(!session.runtime_type_error(&root.join(file), 0, &project));
        }
    }
    session.shutdown().unwrap();
    let requests = server.requests();
    let commands = requests
        .iter()
        .filter(|request| request["method"] == "workspace/executeCommand")
        .map(|request| request["params"]["arguments"][0].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        [
            "openExternalProject",
            "projectInfo",
            "semanticDiagnosticsSync",
            "definition",
            "definition",
            "semanticDiagnosticsSync",
        ]
    );
}

/// 已有成员缓存仍拒绝项目外文件和注册别名，不向服务发送可能回退的定义请求。
#[test]
fn cached_members_reject_nonmembers_and_unknown_projects() {
    for (file, project) in [
        ("unused.ts", "tsconfig.json"),
        ("main.ts", ".narrant-registration-0"),
    ] {
        let (server, mut session) = session(|_| vec![]);
        let root = session.project().workspace_root.clone();
        let error = session
            .definitions(&root.join(file), Position::default(), &root.join(project))
            .unwrap_err();
        assert!(error.to_string().contains("项目"));
        assert!(!server
            .requests()
            .iter()
            .any(|request| { request["params"]["arguments"][0] == "definition" }));
    }
}
