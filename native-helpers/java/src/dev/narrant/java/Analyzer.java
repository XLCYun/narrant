package dev.narrant.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicBoolean;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.ToolFactory;
import org.eclipse.jdt.core.compiler.IScanner;
import org.eclipse.jdt.core.compiler.ITerminalSymbols;
import org.eclipse.jdt.core.compiler.IProblem;
import org.eclipse.jdt.core.dom.*;
import org.eclipse.core.runtime.NullProgressMonitor;
import static dev.narrant.java.Main.Failure;

/** 使用固定 JDT Core 解析一侧源码并生成完整 Code Graph。 */
final class Analyzer {
    private final JsonObject request;
    private final AtomicBoolean cancelled;
    private final long deadline;
    private final long resultLimit;
    private final ProjectModel model;
    private final TreeMap<String, GraphNode> nodes = new TreeMap<>();
    private final Map<String, External> externals = new TreeMap<>();
    private final Map<String, FilePart> parts = new HashMap<>();
    private final List<String> roots = new ArrayList<>();

    /** 同一源码身份跨配置出现不兼容内容时的受控失败。 */
    private static final class UnionConflict extends RuntimeException {
        /** 保存冲突身份。 */
        UnionConflict(String id) { super(id); }
    }

    /** 同一配置的源码声明重复，不能把两处当成一个对象。 */
    private static final class DuplicateDeclaration extends RuntimeException {
        /** 保留重复的源码身份。 */
        DuplicateDeclaration(String id) { super(id); }
    }

    /** JDT binding key 只能在创建它的配置语义环境内使用。 */
    private static final class ConfigState {
        final ProjectModel.Configuration config;
        final Map<String, String> bindingSources = new HashMap<>();
        final Map<String, ASTNode> seen = new HashMap<>();
        final Map<String, String> topLevelTypes = new HashMap<>();

        /** 为一次配置解析建立隔离的声明索引。 */
        ConfigState(ProjectModel.Configuration config) { this.config = config; }
    }

    /** 已归属到一个源码声明的图节点。 */
    private static final class GraphNode {
        final String category;
        final String id;
        final String nativeKind;
        final String name;
        final String signature;
        final String path;
        final ASTNode ast;
        final String source;
        String parent;
        String code;
        final List<String> contains = new ArrayList<>();
        final List<Pending> pending = new ArrayList<>();

        /** 保存节点自己的源码位置，关系待所有文件完成后归类。 */
        GraphNode(String category, String id, String nativeKind, String name, String signature,
                  String path, ASTNode ast, String source) {
            this.category = category;
            this.id = id;
            this.nativeKind = nativeKind;
            this.name = name;
            this.signature = signature;
            this.path = path;
            this.ast = ast;
            this.source = source;
        }
    }

    /** 延迟分类为节点、外部符号或未知的关系证据。 */
    private static final class Pending {
        final String semantic;
        final String observed;
        final IBinding binding;
        final ConfigState state;

        /** 保留原始绑定和恢复状态直到图成员固定。 */
        Pending(String semantic, String observed, IBinding binding, ConfigState state) {
            this.semantic = semantic;
            this.observed = observed;
            this.binding = binding;
            this.state = state;
        }
    }

    /** 外部目标的显示名和可证明来源。 */
    private static final class External {
        final String symbol;
        final String name;
        final String scope;

        /** 记录最终图外目标。 */
        External(String symbol, String name, String scope) {
            this.symbol = symbol;
            this.name = name;
            this.scope = scope;
        }
    }

    /** 每份源码的原始字符和编译单元。 */
    private static final class FilePart {
        final String path;
        final String source;
        final CompilationUnit unit;

        /** 保存只读获取的本侧源码。 */
        FilePart(String path, String source, CompilationUnit unit) {
            this.path = path;
            this.source = source;
            this.unit = unit;
        }
    }

    /** 验证期限和目标模型，进程不运行项目工具。 */
    Analyzer(JsonObject request, AtomicBoolean cancelled) throws Failure {
        this.request = request;
        this.cancelled = cancelled;
        JsonObject limits = Main.object(request, "limits");
        Main.fields(limits, Set.of("analysis_timeout_ms", "max_result_bytes", "max_diagnostic_bytes"));
        long timeout = positive(limits, "analysis_timeout_ms");
        this.resultLimit = positive(limits, "max_result_bytes");
        positive(limits, "max_diagnostic_bytes");
        this.deadline = System.nanoTime() + timeout * 1_000_000L;
        this.model = ProjectModel.discover(request);
    }

    /** 解析正整数限制。 */
    private static long positive(JsonObject object, String key) throws Failure {
        try {
            long value = object.get(key).getAsLong();
            if (value > 0 && object.get(key).getAsString().matches("[0-9]+")) return value;
        } catch (Exception ignored) { /* 下面统一返回输入错误。 */ }
        throw new Failure("invalid_input", "invalid limit: " + key);
    }

    /** 返回 Host 也独立执行的结果字节上限。 */
    long maxResultBytes() { return resultLimit; }

    /** 执行配置隔离解析、来源归并与关系终判。 */
    JsonObject run() throws Failure {
        for (ProjectModel.Configuration config : model.configurations) {
            checkpoint();
            boolean needed = model.files.stream().anyMatch(file -> model.activeConfigurations(file).contains(config));
            if (!needed) continue;
            List<String> context = model.context(config);
            for (String file : context) if (file.endsWith("/module-info.java") || file.equals("module-info.java"))
                throw new Failure("unsupported_configuration", "JPMS module descriptors require a separate profile", List.of(file));
            if (context.isEmpty()) continue;
            parseConfiguration(config, context);
        }
        checkpoint();
        for (GraphNode node : nodes.values()) node.code = projection(node);
        JsonObject result = new JsonObject();
        result.add("graph", graph());
        result.add("evidence", evidence());
        return result;
    }

    /** 检查取消与分析总时限，JDT 回调间也执行。 */
    private void checkpoint() throws Failure {
        if (cancelled.get()) throw Failure.cancelled();
        if (System.nanoTime() > deadline) throw new Failure("resource_limit", "analysis timeout");
    }

    /** 对一个配置创建独立 JDT 语义环境。 */
    private void parseConfiguration(ProjectModel.Configuration config, List<String> context) throws Failure {
        try {
            ConfigState state = new ConfigState(config);
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            Map<String, String> options = new HashMap<>();
            JavaCore.setComplianceOptions(config.release, options);
            options.put(JavaCore.COMPILER_RELEASE, JavaCore.ENABLED);
            options.put(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, JavaCore.DISABLED);
            parser.setCompilerOptions(options);
            parser.setResolveBindings(true);
            parser.setBindingsRecovery(true);
            parser.setStatementsRecovery(false);
            List<String> roots = new ArrayList<>();
            for (ProjectModel.SourceRoot root : config.roots)
                if (!root.test || model.includeTests) roots.add(model.repository.resolve(root.path).toString());
            String[] encodings = new String[roots.size()];
            java.util.Arrays.fill(encodings, "UTF-8");
            parser.setEnvironment(new String[]{model.targetJdk.resolve("lib/jrt-fs.jar").toString()},
                                  roots.toArray(String[]::new), encodings, false);
            String[] sourcePaths = context.stream().map(file -> model.repository.resolve(file).toString()).toArray(String[]::new);
            String[] sourceEncodings = new String[sourcePaths.length];
            java.util.Arrays.fill(sourceEncodings, "UTF-8");
            final Failure[] callbackFailure = new Failure[1];
            parser.createASTs(sourcePaths, sourceEncodings, new String[0], new FileASTRequestor() {
                /** 不让 JDT 回调异常消失在解析器内部。 */
                @Override public void acceptAST(String path, CompilationUnit unit) {
                    if (callbackFailure[0] != null) return;
                    try {
                        checkpoint();
                        String relative = model.repository.relativize(Path.of(path)).toString().replace('\\', '/');
                        String source = Files.readString(ProjectModel.checkedFile(model.repository, relative), StandardCharsets.UTF_8);
                        if (source.indexOf('\uFFFD') >= 0) throw new Failure("object_extraction_failed", "invalid UTF-8 source", List.of(relative));
                        FilePart part = new FilePart(relative, source, unit);
                        parts.put(relative, part);
                        rejectMalformed(part);
                        verifyPackage(config, part);
                        boolean selected = model.selected.containsKey(relative) &&
                            model.activeConfigurations(relative).contains(config);
                        new Visitor(part, selected, state).extract();
                    } catch (Failure problem) { callbackFailure[0] = problem; }
                    catch (DuplicateDeclaration problem) {
                        callbackFailure[0] = new Failure("object_extraction_failed", "duplicate Java declaration: " + problem.getMessage());
                    }
                    catch (UnionConflict problem) {
                        callbackFailure[0] = new Failure("union_conflict", "incompatible source identity: " + problem.getMessage());
                    }
                    catch (Exception problem) {
                        callbackFailure[0] = new Failure("object_extraction_failed", "cannot extract Java source");
                    }
                }
            }, new NullProgressMonitor() {
                /** JDT 查询时立即感知取消和期限。 */
                @Override public boolean isCanceled() {
                    return cancelled.get() || System.nanoTime() > deadline;
                }
            });
            if (callbackFailure[0] != null) throw callbackFailure[0];
            for (String file : context) if (!parts.containsKey(file))
                throw new Failure("object_extraction_failed", "JDT omitted source", List.of(file));
        } catch (Failure e) { throw e; }
        catch (DuplicateDeclaration e) { throw new Failure("object_extraction_failed", "duplicate Java declaration: " + e.getMessage()); }
        catch (UnionConflict e) { throw new Failure("union_conflict", "incompatible source identity: " + e.getMessage()); }
        catch (Exception e) {
            if (System.nanoTime() > deadline) throw new Failure("resource_limit", "analysis timeout");
            if (cancelled.get()) throw Failure.cancelled();
            throw new Failure("object_extraction_failed", "JDT parser failed: " + e.getMessage());
        }
    }

    /** 语法恢复可能遗漏声明，因此拒绝畸形选中树。 */
    private static void rejectMalformed(FilePart part) throws Failure {
        final boolean[] malformed = {false};
        part.unit.accept(new ASTVisitor() {
            /** 仅标记结构损坏；缺依赖诊断仍可保留对象。 */
            @Override public void preVisit(ASTNode node) {
                if ((node.getFlags() & ASTNode.MALFORMED) != 0) malformed[0] = true;
            }
        });
        if (malformed[0]) throw new Failure("object_extraction_failed", "malformed Java syntax", List.of(part.path));
        Set<Integer> duplicates = Set.of(IProblem.DuplicateMethod, IProblem.DuplicateField,
            IProblem.DuplicateTypes, IProblem.DuplicateNestedType, IProblem.DuplicateMethodErasure,
            IProblem.DuplicateParameterizedMethods, IProblem.DuplicateAnnotationMember,
            IProblem.RecordDuplicateComponent);
        for (IProblem problem : part.unit.getProblems()) if (problem.isError() && duplicates.contains(problem.getID()))
            throw new Failure("object_extraction_failed", "duplicate Java declaration", List.of(part.path));
    }

    /** 源码包名须与明确源码根下的真实目录一致。 */
    private static void verifyPackage(ProjectModel.Configuration config, FilePart part) throws Failure {
        String expected = null;
        for (ProjectModel.SourceRoot root : config.roots) {
            if (!part.path.startsWith(root.path + "/")) continue;
            String local = part.path.substring(root.path.length() + 1);
            int slash = local.lastIndexOf('/');
            expected = slash < 0 ? "" : local.substring(0, slash).replace('/', '.');
            break;
        }
        if (expected == null) throw new Failure("object_extraction_failed", "source outside configuration roots", List.of(part.path));
        String actual = part.unit.getPackage() == null ? "" : part.unit.getPackage().getName().getFullyQualifiedName();
        if (!expected.equals(actual))
            throw new Failure("object_extraction_failed", "package does not match source root", List.of(part.path));
    }

    /** 实际选中源码的声明树与关系访问器。 */
    private final class Visitor extends ASTVisitor {
        private final FilePart part;
        private final boolean selected;
        private final ConfigState state;
        private final Deque<GraphNode> owners = new ArrayDeque<>();
        private final Map<ASTNode, GraphNode> opened = new HashMap<>();
        private final Map<String, Integer> slots = new HashMap<>();

        /** 建立文件局部访问状态。 */
        Visitor(FilePart part, boolean selected, ConfigState state) {
            this.part = part;
            this.selected = selected;
            this.state = state;
        }

        /** 添加物理文件与本文件的 package 声明。 */
        void extract() throws Failure {
            String id = "java:" + part.path + "#file";
            GraphNode file = add("container", id, "file", part.path, null, part.unit, null);
            if (selected && !roots.contains(id)) roots.add(id);
            owners.push(file);
            part.unit.accept(this);
            owners.pop();
        }

        /** 按词法 owner 建立稳定声明身份。 */
        private String id(String kind, String name, String overload) {
            String owner = owners.isEmpty() ? "java:" + part.path + "#file" : owners.peek().id;
            String slot = overload == null ? "" : "(" + overload + ")";
            return owner + "/" + kind + ":" + escape(name) + slot;
        }

        /** 对重复初始化块、匿名类使用同类兄弟槽位。 */
        private String numbered(String kind) {
            String owner = owners.isEmpty() ? part.path : owners.peek().id;
            String key = owner + "/" + kind;
            int next = slots.merge(key, 1, Integer::sum);
            return "$" + next;
        }

        /** 创建选中节点，并保证同一源码身份兼容。 */
        private GraphNode add(String category, String id, String kind, String name,
                              String signature, ASTNode ast, IBinding binding) {
            ASTNode seen = state.seen.putIfAbsent(id, ast);
            if (seen != null && seen != ast) throw new DuplicateDeclaration(id);
            String startSource = ast == part.unit ? part.source : excerpt(part.source, ast);
            GraphNode node = new GraphNode(category, id, kind, name, signature, part.path, ast, startSource);
            if (selected) {
                GraphNode existing = nodes.putIfAbsent(id, node);
                if (existing != null) {
                    if (!existing.category.equals(category) || !existing.nativeKind.equals(kind) ||
                        !existing.path.equals(part.path) || !existing.source.equals(startSource) ||
                        !java.util.Objects.equals(existing.name, name) ||
                        !java.util.Objects.equals(existing.signature, signature))
                        throw new UnionConflict(id);
                    node = existing;
                }
            }
            if (!owners.isEmpty() && !owners.peek().contains.contains(id)) owners.peek().contains.add(id);
            if (!owners.isEmpty()) {
                String parent = owners.peek().id;
                if (node.parent != null && !node.parent.equals(parent))
                    throw new UnionConflict(id);
                node.parent = parent;
            }
            if (usable(binding)) {
                String key = declaration(binding).getKey();
                String previous = state.bindingSources.putIfAbsent(key, id);
                if (previous != null && !previous.equals(id)) throw new DuplicateDeclaration(id);
            }
            return node;
        }

        /** 为独立源码声明打开引用所属区域。 */
        private void open(ASTNode ast, GraphNode node) {
            if (node != null) { owners.push(node); opened.put(ast, node); }
        }

        /** 声明节点结束后恢复父级。 */
        @Override public void postVisit(ASTNode node) {
            if (opened.remove(node) != null) owners.pop();
        }

        /** 按 AST 具体形态创建源码声明。 */
        @Override public void preVisit(ASTNode ast) {
            if (ast instanceof TypeDeclaration node) {
                checkTopLevel(node.getName().getIdentifier(), ast);
                String kind = node.isInterface() ? "interface" : "class";
                open(ast, add("type", id(kind, node.getName().getIdentifier(), null), kind,
                    node.getName().getIdentifier(), header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof EnumDeclaration node) {
                checkTopLevel(node.getName().getIdentifier(), ast);
                open(ast, add("type", id("enum", node.getName().getIdentifier(), null), "enum",
                    node.getName().getIdentifier(), header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof AnnotationTypeDeclaration node) {
                checkTopLevel(node.getName().getIdentifier(), ast);
                open(ast, add("type", id("annotation", node.getName().getIdentifier(), null), "annotation",
                    node.getName().getIdentifier(), header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof RecordDeclaration node) {
                checkTopLevel(node.getName().getIdentifier(), ast);
                open(ast, add("type", id("record", node.getName().getIdentifier(), null), "record",
                    node.getName().getIdentifier(), header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof AnonymousClassDeclaration node) {
                String name = numbered("anonymous");
                open(ast, add("type", id("anonymous", name, null), "anonymous", name,
                    header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof MethodDeclaration node) {
                String name = node.isConstructor() ? "<init>" : node.getName().getIdentifier();
                String kind = node.isConstructor() ? "constructor" : "method";
                String parameters = parameters(node, part.source);
                open(ast, add("callable", id(kind, name, parameters), kind, name,
                    header(part.source, ast), ast, node.resolveBinding()));
            } else if (ast instanceof AnnotationTypeMemberDeclaration node) {
                String name = node.getName().getIdentifier();
                open(ast, add("callable", id("annotation-member", name, ""), "annotation-member", name,
                    canonical(part.source, ast.getStartPosition(), ast.getStartPosition() + ast.getLength()),
                    ast, node.resolveBinding()));
            } else if (ast instanceof Initializer node) {
                boolean isStatic = java.lang.reflect.Modifier.isStatic(node.getModifiers());
                String kind = isStatic ? "static-initializer" : "initializer";
                String name = numbered(kind);
                open(ast, add("execution_region", id(kind, name, null), kind, name, null, ast, null));
            } else if (ast instanceof EnumConstantDeclaration node) {
                String name = node.getName().getIdentifier();
                open(ast, add("value", id("enum-constant", name, null), "enum-constant", name,
                    header(part.source, ast), ast, node.resolveVariable()));
            } else if (ast instanceof VariableDeclarationFragment node && node.getParent() instanceof FieldDeclaration field) {
                String name = node.getName().getIdentifier();
                VariableDeclarationFragment first = (VariableDeclarationFragment) field.fragments().get(0);
                open(ast, add("value", id("field", name, null), "field", name,
                    canonical(part.source, field.getStartPosition(), first.getStartPosition()) + " " +
                        canonical(part.source, node.getStartPosition(), node.getStartPosition() + node.getLength()),
                    ast, node.resolveBinding()));
                refer("code.type_use", field.getType().toString(), field.getType().resolveBinding());
            } else if (ast instanceof SingleVariableDeclaration node && node.getParent() instanceof RecordDeclaration) {
                String name = node.getName().getIdentifier();
                open(ast, add("value", id("record-component", name, null), "record-component", name,
                    canonical(part.source, ast.getStartPosition(), ast.getStartPosition() + ast.getLength()),
                    ast, node.resolveBinding()));
                refer("code.type_use", node.getType().toString(), node.getType().resolveBinding());
            } else if (ast instanceof PackageDeclaration node) {
                String name = node.getName().getFullyQualifiedName();
                open(ast, add("container", id("package", name, null), "package", name,
                    canonical(part.source, ast.getStartPosition(), ast.getStartPosition() + ast.getLength()), ast, null));
            } else if (ast instanceof ImportDeclaration node) {
                String name = node.getName().getFullyQualifiedName();
                open(ast, add("extension", id("import", name + (node.isOnDemand() ? ".*" : ""), null),
                    "import", name, canonical(part.source, ast.getStartPosition(), ast.getStartPosition() + ast.getLength()),
                    ast, null));
                refer("code.imports", name, node.resolveBinding());
            }
        }

        /** 记录一般方法调用。 */
        @Override public boolean visit(MethodInvocation node) {
            refer("code.calls", node.getName().getIdentifier(), node.resolveMethodBinding()); return true;
        }

        /** 记录 super 方法调用。 */
        @Override public boolean visit(SuperMethodInvocation node) {
            refer("code.calls", node.getName().getIdentifier(), node.resolveMethodBinding()); return true;
        }

        /** 记录 new 表达式的构造器。 */
        @Override public boolean visit(ClassInstanceCreation node) {
            refer("code.calls", node.getType().toString(), node.resolveConstructorBinding()); return true;
        }

        /** 记录 this 构造器调用。 */
        @Override public boolean visit(ConstructorInvocation node) {
            refer("code.calls", "this", node.resolveConstructorBinding()); return true;
        }

        /** 记录 super 构造器调用。 */
        @Override public boolean visit(SuperConstructorInvocation node) {
            refer("code.calls", "super", node.resolveConstructorBinding()); return true;
        }

        /** 方法引用不等同调用。 */
        @Override public boolean visit(ExpressionMethodReference node) {
            refer("code.references", node.getName().getIdentifier(), node.resolveMethodBinding()); return true;
        }

        /** 类型方法引用不等同调用。 */
        @Override public boolean visit(TypeMethodReference node) {
            refer("code.references", node.getName().getIdentifier(), node.resolveMethodBinding()); return true;
        }

        /** super 方法引用不等同调用。 */
        @Override public boolean visit(SuperMethodReference node) {
            refer("code.references", node.getName().getIdentifier(), node.resolveMethodBinding()); return true;
        }

        /** 构造器引用不等同立即实例化。 */
        @Override public boolean visit(CreationReference node) {
            refer("code.references", node.getType().toString(), node.resolveMethodBinding()); return true;
        }

        /** 显式继承与实现关系由绑定验证。 */
        @Override public boolean visit(TypeDeclaration node) {
            if (node.getSuperclassType() != null)
                refer("code.extends", node.getSuperclassType().toString(), node.getSuperclassType().resolveBinding());
            for (Object item : node.superInterfaceTypes()) {
                Type type = (Type) item;
                refer(node.isInterface() ? "code.extends" : "code.implements", type.toString(), type.resolveBinding());
            }
            return true;
        }

        /** enum 的显式接口关系由绑定验证。 */
        @Override public boolean visit(EnumDeclaration node) {
            for (Object item : node.superInterfaceTypes()) {
                Type type = (Type) item;
                refer("code.implements", type.toString(), type.resolveBinding());
            }
            return true;
        }

        /** record 的显式接口关系由绑定验证。 */
        @Override public boolean visit(RecordDeclaration node) {
            for (Object item : node.superInterfaceTypes()) {
                Type type = (Type) item;
                refer("code.implements", type.toString(), type.resolveBinding());
            }
            return true;
        }

        /** 将关系归属最近的独立源码对象。 */
        private void refer(String semantic, String observed, IBinding binding) {
            if (selected && !owners.isEmpty()) owners.peek().pending.add(new Pending(semantic, observed, binding, state));
        }

        /** 同一配置下两份顶层同名类型不能形成两个确定对象。 */
        private void checkTopLevel(String name, ASTNode ast) {
            if (!(ast.getParent() instanceof CompilationUnit)) return;
            String packageName = part.unit.getPackage() == null ? "" : part.unit.getPackage().getName().getFullyQualifiedName();
            String qualified = packageName.isEmpty() ? name : packageName + "." + name;
            String previous = state.topLevelTypes.putIfAbsent(qualified, part.path);
            if (previous != null) throw new DuplicateDeclaration(qualified);
        }
    }

    /** 构建语义图并在最终成员集合上判定关系目标。 */
    private JsonObject graph() throws Failure {
        JsonObject graph = new JsonObject();
        roots.sort(Comparator.naturalOrder());
        graph.add("root_contains", Main.strings(roots));
        JsonArray entries = new JsonArray();
        for (GraphNode node : nodes.values()) {
            checkpoint();
            JsonObject entry = new JsonObject();
            entry.addProperty("category", node.category);
            entry.addProperty("id", node.id);
            entry.addProperty("language", "java");
            entry.addProperty("native_kind", node.nativeKind);
            nullable(entry, "name", node.name);
            nullable(entry, "signature", node.signature);
            nullable(entry, "code", node.code);
            nullable(entry, "source", node.source);
            node.contains.sort(Comparator.naturalOrder());
            entry.add("contains", Main.strings(node.contains));
            JsonArray refs = new JsonArray();
            Set<String> seen = new HashSet<>();
            List<JsonObject> ordered = new ArrayList<>();
            for (Pending pending : node.pending) {
                JsonObject target = classify(pending);
                String fact = pending.semantic + "\u0000" + target;
                if (!seen.add(fact)) continue;
                JsonObject reference = new JsonObject();
                reference.addProperty("semantic", pending.semantic);
                reference.add("target", target);
                ordered.add(reference);
            }
            if (node.parent != null && nodes.get(node.parent).category.equals("type") &&
                Set.of("type", "callable", "value", "execution_region").contains(node.category)) {
                JsonObject target = new JsonObject();
                target.addProperty("kind", "node");
                target.addProperty("id", node.parent);
                JsonObject reference = new JsonObject();
                reference.addProperty("semantic", "code.member_of");
                reference.add("target", target);
                ordered.add(reference);
            }
            ordered.sort(Comparator.comparing(JsonObject::toString));
            for (JsonObject reference : ordered) refs.add(reference);
            entry.add("references", refs);
            JsonObject extensions = new JsonObject();
            extensions.addProperty("java.declaration_kind", node.nativeKind);
            entry.add("extensions", extensions);
            entries.add(entry);
        }
        graph.add("nodes", entries);
        JsonArray externalSymbols = new JsonArray();
        for (External external : externals.values()) {
            JsonObject item = new JsonObject();
            item.addProperty("symbol", external.symbol);
            item.addProperty("name", external.name);
            item.addProperty("scope", external.scope);
            externalSymbols.add(item);
        }
        graph.add("external_symbols", externalSymbols);
        return graph;
    }

    /** 以 binding 的真实状态分类，绝不提升 recovered 关系。 */
    private JsonObject classify(Pending pending) {
        JsonObject target = new JsonObject();
        if (!usable(pending.binding)) {
            target.addProperty("kind", "unresolved");
            target.addProperty("name", pending.observed);
            return target;
        }
        IBinding declaration = declaration(pending.binding);
        String key = declaration.getKey();
        String sourceId = pending.state.bindingSources.get(key);
        if (sourceId != null && nodes.containsKey(sourceId)) {
            target.addProperty("kind", "node");
            target.addProperty("id", sourceId);
            return target;
        }
        String ownerId = repositoryOwner(pending.state, declaration);
        boolean repository = sourceId != null || ownerId != null;
        String symbol = sourceId != null ? sourceId : ownerId != null ?
            ownerId + "/implicit:" + escape(symbol(declaration)) : symbol(declaration);
        String scope = repository ? "repository" : "dependency";
        target.addProperty("kind", "external");
        target.addProperty("symbol", symbol);
        externals.putIfAbsent(symbol, new External(symbol, display(declaration), scope));
        return target;
    }

    /** 默认构造器等隐式成员沿声明类型继承仓库来源。 */
    private static String repositoryOwner(ConfigState state, IBinding binding) {
        ITypeBinding owner = null;
        if (binding instanceof IMethodBinding method) owner = method.getDeclaringClass();
        if (binding instanceof IVariableBinding variable) owner = variable.getDeclaringClass();
        return owner == null ? null : state.bindingSources.get(owner.getTypeDeclaration().getKey());
    }

    /** 建立与请求文件一一对应的来源证据。 */
    private JsonObject evidence() {
        JsonObject evidence = new JsonObject();
        JsonArray configs = new JsonArray();
        for (ProjectModel.Configuration config : model.configurations) {
            JsonObject item = new JsonObject();
            item.addProperty("id", config.id);
            item.add("source_paths", Main.strings(config.sourcePaths));
            item.add("platform_ids", Main.strings(List.of(model.targetPlatformId)));
            configs.add(item);
        }
        evidence.add("configurations", configs);
        JsonArray files = new JsonArray();
        for (String path : model.files) {
            JsonObject item = new JsonObject();
            item.addProperty("path", path);
            item.addProperty("disposition", !model.includeTests && model.isTest(path) ? "excluded_test" : "analyzed");
            List<ProjectModel.Configuration> relevant = model.activeConfigurations(path);
            if (relevant.isEmpty()) relevant = model.selected.get(path);
            item.add("configuration_ids", Main.strings(relevant.stream().map(config -> config.id).toList()));
            files.add(item);
        }
        evidence.add("files", files);
        JsonArray origins = new JsonArray();
        for (GraphNode node : nodes.values()) {
            JsonObject item = new JsonObject();
            item.addProperty("node_id", node.id);
            item.add("selected_paths", Main.strings(List.of(node.path)));
            origins.add(item);
        }
        evidence.add("origins", origins);
        return evidence;
    }

    /** 序列化可空源码展示字段。 */
    private static void nullable(JsonObject object, String key, String value) {
        if (value == null) object.add(key, com.google.gson.JsonNull.INSTANCE);
        else object.addProperty(key, value);
    }

    /** 仅将完整且非恢复的绑定视为确定事实。 */
    private static boolean usable(IBinding binding) {
        if (binding == null || binding.isRecovered() || binding.getKey() == null) return false;
        IBinding declaration = declaration(binding);
        return declaration != null && !declaration.isRecovered() && declaration.getKey() != null;
    }

    /** 将泛型实例绑定收敛到真实源码声明。 */
    private static IBinding declaration(IBinding binding) {
        if (binding instanceof IMethodBinding method) return method.getMethodDeclaration();
        if (binding instanceof ITypeBinding type) return type.getTypeDeclaration();
        if (binding instanceof IVariableBinding variable) return variable.getVariableDeclaration();
        return binding;
    }

    /** 构建不含 checkout 绝对路径的语义符号。 */
    private static String symbol(IBinding binding) {
        IBinding declaration = declaration(binding);
        if (declaration instanceof ITypeBinding type) {
            String name = type.getQualifiedName();
            return "java:" + (name.isEmpty() ? type.getKey() : name);
        }
        if (declaration instanceof IMethodBinding method) {
            String owner = method.getDeclaringClass() == null ? "?" : method.getDeclaringClass().getTypeDeclaration().getQualifiedName();
            List<String> parameters = new ArrayList<>();
            for (ITypeBinding parameter : method.getParameterTypes()) parameters.add(parameter.getErasure().getQualifiedName());
            return "java:" + owner + "#" + (method.isConstructor() ? "<init>" : method.getName()) +
                "(" + String.join(",", parameters) + ")";
        }
        if (declaration instanceof IVariableBinding variable) {
            String owner = variable.getDeclaringClass() == null ? "?" : variable.getDeclaringClass().getTypeDeclaration().getQualifiedName();
            return "java:" + owner + "#" + variable.getName();
        }
        return "java:" + declaration.getKey();
    }

    /** 提供已确认外部目标的人类可读名称。 */
    private static String display(IBinding binding) {
        if (binding instanceof ITypeBinding type) return type.getQualifiedName();
        if (binding instanceof IMethodBinding method) return method.getName();
        return binding.getName();
    }

    /** 只把字母数字及安全分隔符保留在源码身份中。 */
    private static String escape(String value) {
        return value.replace("%", "%25").replace("/", "%2F").replace(":", "%3A")
            .replace("#", "%23").replace("(", "%28").replace(")", "%29");
    }

    /** 按词法作用域及序号归一泛型变量；内层同名声明遮蔽外层，不依赖绑定。 */
    private static String parameters(MethodDeclaration method, String source) {
        Map<String, String> variables = new HashMap<>();
        int typeDepth = 0, methodDepth = 0;
        for (ASTNode scope = method; scope != null; scope = scope.getParent()) {
            List<?> parameters = List.of();
            String prefix = "";
            if (scope instanceof MethodDeclaration declaration) {
                parameters = declaration.typeParameters();
                prefix = "@method" + methodDepth++ + ":";
            } else if (scope instanceof TypeDeclaration declaration) {
                parameters = declaration.typeParameters();
                prefix = "@type" + typeDepth++ + ":";
            } else if (scope instanceof RecordDeclaration declaration) {
                parameters = declaration.typeParameters();
                prefix = "@type" + typeDepth++ + ":";
            }
            for (int index = 0; index < parameters.size(); index++) {
                TypeParameter parameter = (TypeParameter) parameters.get(index);
                variables.putIfAbsent(parameter.getName().getIdentifier(), prefix + index);
            }
        }
        List<String> parts = new ArrayList<>();
        for (Object value : method.parameters()) {
            SingleVariableDeclaration parameter = (SingleVariableDeclaration) value;
            parts.add(parameterType(parameter.getType(), source, variables) +
                "[]".repeat(parameter.getExtraDimensions()) + (parameter.isVarargs() ? "..." : ""));
        }
        return String.join(",", parts);
    }

    /** 只替换类型位置的简单名称，保留限定类型及注解中的同名标识符。 */
    private static String parameterType(Type type, String source, Map<String, String> variables) {
        List<SimpleName> names = new ArrayList<>();
        type.accept(new ASTVisitor() {
            /** 收集词法类型变量的源码区间。 */
            @Override public boolean visit(SimpleType simple) {
                if (simple.getName() instanceof SimpleName name && variables.containsKey(name.getIdentifier()))
                    names.add(name);
                return true;
            }
        });
        names.sort(Comparator.comparingInt(ASTNode::getStartPosition));
        StringBuilder result = new StringBuilder();
        int cursor = type.getStartPosition();
        for (SimpleName name : names) {
            result.append(canonical(source, cursor, name.getStartPosition()));
            result.append(variables.get(name.getIdentifier()));
            cursor = name.getStartPosition() + name.getLength();
        }
        result.append(canonical(source, cursor, type.getStartPosition() + type.getLength()));
        return result.toString().replaceAll("\\s+", "");
    }

    /** 取得原始源码展示片段。 */
    private static String excerpt(String source, ASTNode node) {
        int start = Math.max(0, node.getStartPosition());
        int end = Math.min(source.length(), start + node.getLength());
        return source.substring(start, end);
    }

    /** 提取声明头部，方法体内容归 code 而非 signature。 */
    private static String header(String source, ASTNode node) {
        int start = Math.max(0, node.getStartPosition());
        int end = Math.min(source.length(), start + node.getLength());
        if (node instanceof MethodDeclaration method && method.getBody() != null)
            end = method.getBody().getStartPosition();
        else if (node instanceof EnumConstantDeclaration constant && constant.getAnonymousClassDeclaration() != null)
            end = constant.getAnonymousClassDeclaration().getStartPosition();
        else if (node instanceof AnonymousClassDeclaration) end = start;
        else {
            int nameEnd = typeNameEnd(node);
            if (nameEnd >= 0) end = typeBodyOpen(source, nameEnd, end);
        }
        if (node instanceof RecordDeclaration record) {
            StringBuilder result = new StringBuilder();
            int cursor = start;
            for (Object value : record.recordComponents()) {
                SingleVariableDeclaration component = (SingleVariableDeclaration) value;
                result.append(canonical(source, cursor, component.getStartPosition()))
                    .append(" @child(record-component:").append(component.getName().getIdentifier()).append(") ");
                cursor = component.getStartPosition() + component.getLength();
            }
            return result.append(canonical(source, cursor, end)).toString().trim();
        }
        return canonical(source, start, end);
    }

    /** 返回具名类型标识符之后的位置。 */
    private static int typeNameEnd(ASTNode node) {
        SimpleName name = null;
        if (node instanceof TypeDeclaration type) name = type.getName();
        else if (node instanceof EnumDeclaration type) name = type.getName();
        else if (node instanceof AnnotationTypeDeclaration type) name = type.getName();
        else if (node instanceof RecordDeclaration type) name = type.getName();
        return name == null ? -1 : name.getStartPosition() + name.getLength();
    }

    /** 词法寻找类型体开括号，跳过注解字符串和括号内数组初始化。 */
    private static int typeBodyOpen(String source, int from, int limit) {
        if (from >= limit) return limit;
        IScanner scanner = ToolFactory.createScanner(true, false, false, false);
        scanner.setSource(source.substring(from, limit).toCharArray());
        int parens = 0;
        try {
            while (true) {
                int token = scanner.getNextToken();
                if (token == ITerminalSymbols.TokenNameEOF) break;
                if (token == ITerminalSymbols.TokenNameLPAREN) parens++;
                else if (token == ITerminalSymbols.TokenNameRPAREN) parens = Math.max(0, parens - 1);
                else if (token == ITerminalSymbols.TokenNameLBRACE && parens == 0)
                    return from + scanner.getCurrentTokenStartPosition();
            }
        } catch (Exception ignored) { /* 词法错误已在提取检查中处理。 */ }
        return limit;
    }

    /** 自身投影保留有序子对象锚点而剥离其内容。 */
    private String projection(GraphNode node) {
        int start = node.ast == null ? 0 : Math.max(0, node.ast.getStartPosition());
        int end = node.ast == null ? node.source.length() : Math.min(parts.get(node.path).source.length(),
            start + node.ast.getLength());
        String source = parts.get(node.path).source;
        List<GraphNode> children = new ArrayList<>();
        for (String id : node.contains) children.add(nodes.get(id));
        children.sort(Comparator.comparingInt(child -> child.ast.getStartPosition()));
        StringBuilder result = new StringBuilder();
        int cursor = start;
        for (GraphNode child : children) {
            int childStart = child.ast.getStartPosition();
            int childEnd = childStart + child.ast.getLength();
            if (childStart < cursor || childEnd > end) continue;
            result.append(canonical(source, cursor, childStart)).append(" @child(")
                .append(child.nativeKind).append(':').append(child.name).append(") ");
            cursor = childEnd;
        }
        result.append(canonical(source, cursor, end));
        return result.toString().trim();
    }

    /** JDT 词法器去除排版及普通注释，同时保留字面量。 */
    private static String canonical(String source, int start, int end) {
        if (start >= end || start < 0 || end > source.length()) return "";
        IScanner scanner = ToolFactory.createScanner(true, false, false, false);
        scanner.setSource(source.substring(start, end).toCharArray());
        StringBuilder normalized = new StringBuilder();
        try {
            while (true) {
                int token = scanner.getNextToken();
                if (token == ITerminalSymbols.TokenNameEOF) break;
                if (token == ITerminalSymbols.TokenNameCOMMENT_BLOCK ||
                    token == ITerminalSymbols.TokenNameCOMMENT_JAVADOC ||
                    token == ITerminalSymbols.TokenNameCOMMENT_LINE ||
                    token == ITerminalSymbols.TokenNameWHITESPACE) continue;
                if (!normalized.isEmpty()) normalized.append(' ');
                normalized.append(scanner.getCurrentTokenSource());
            }
        } catch (Exception problem) {
            return source.substring(start, end).trim();
        }
        return normalized.toString();
    }
}
