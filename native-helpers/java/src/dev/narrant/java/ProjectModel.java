package dev.narrant.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import javax.xml.XMLConstants;
import javax.xml.parsers.DocumentBuilderFactory;
import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.Node;
import org.w3c.dom.NodeList;
import static dev.narrant.java.Main.Failure;

/** 仓库内有限静态配置及显式目标 JDK 模型。 */
final class ProjectModel {
    final Path repository;
    final Path targetJdk;
    final String targetPlatformId;
    final List<Configuration> configurations;
    final Map<String, Configuration> selected;
    final List<String> files;
    final boolean includeTests;

    /** 一份静态配置所控制的源码根及 release。 */
    static final class Configuration {
        final String id;
        final String release;
        final List<String> sourcePaths;
        final List<SourceRoot> roots;

        /** 保存已验证的单一配置。 */
        Configuration(String id, String release, List<String> sourcePaths, List<SourceRoot> roots) {
            this.id = id;
            this.release = release;
            this.sourcePaths = sourcePaths;
            this.roots = roots;
        }
    }

    /** 有明确生产/测试归属的源码根。 */
    static final class SourceRoot {
        final String path;
        final boolean test;

        /** 记录一个相对仓库的源码根。 */
        SourceRoot(String path, boolean test) { this.path = path; this.test = test; }
    }

    /** 完成路径及静态来源检查后保存请求模型。 */
    private ProjectModel(Path repository, Path targetJdk, String targetPlatformId, List<Configuration> configurations,
                         Map<String, Configuration> selected, List<String> files, boolean includeTests) {
        this.repository = repository;
        this.targetJdk = targetJdk;
        this.targetPlatformId = targetPlatformId;
        this.configurations = configurations;
        this.selected = selected;
        this.files = files;
        this.includeTests = includeTests;
    }

    /** 从固定输入发现 standalone 声明或有限 Maven POM。 */
    static ProjectModel discover(JsonObject request) throws Failure {
        JsonObject input = Main.object(request, "input");
        Main.fields(input, Set.of("repository_root", "files", "include_tests", "profile"));
        Main.requireString(input, "profile", Main.PROFILE);
        Path root = Path.of(Main.string(input, "repository_root"));
        if (!root.isAbsolute() || !Files.isDirectory(root)) throw new Failure("invalid_input", "invalid repository_root");
        try { root = root.toRealPath(); } catch (Exception e) { throw new Failure("invalid_input", "unreadable repository_root"); }
        if (!input.get("include_tests").isJsonPrimitive() || !input.get("include_tests").getAsJsonPrimitive().isBoolean())
            throw new Failure("invalid_input", "include_tests must be boolean");
        boolean includeTests = input.get("include_tests").getAsBoolean();
        List<String> files = new ArrayList<>();
        Set<String> unique = new HashSet<>();
        for (JsonElement element : Main.array(input, "files")) {
            if (!element.isJsonPrimitive() || !element.getAsJsonPrimitive().isString())
                throw new Failure("invalid_input", "files must be paths");
            String file = element.getAsString();
            validateRelative(file);
            if (!file.endsWith(".java") || !unique.add(file))
                throw new Failure("invalid_input", "duplicate or non-Java file: " + file);
            checkedFile(root, file);
            files.add(file);
        }
        JsonObject environment = Main.object(request, "environment");
        Main.fields(environment, Set.of("scratch_root", "platforms"));
        Path scratch = Path.of(Main.string(environment, "scratch_root"));
        if (!scratch.isAbsolute() || scratch.normalize().startsWith(root))
            throw new Failure("invalid_input", "scratch_root must be outside repository");
        List<Configuration> configs = new ArrayList<>();
        Path metadata = root.resolve("narrant-java.json");
        Path pom = root.resolve("pom.xml");
        if (Files.exists(metadata) && Files.exists(pom))
            throw new Failure("unsupported_configuration", "ambiguous Java metadata and POM");
        if (Files.exists(metadata)) configs.add(readMetadata(root, metadata));
        else if (Files.exists(pom)) readPomTree(root, root, configs, new HashSet<>(), null);
        else if (Files.exists(root.resolve("build.gradle")) || Files.exists(root.resolve("build.gradle.kts")) ||
                 Files.exists(root.resolve("settings.gradle")) || Files.exists(root.resolve("settings.gradle.kts")))
            throw new Failure("unsupported_configuration", "Gradle needs narrant-java.json static metadata");
        else throw new Failure("unsupported_configuration", "no explicit Java source roots or target release");
        if (configs.isEmpty()) throw new Failure("unsupported_configuration", "no Java configurations");
        Set<String> releases = new HashSet<>();
        for (Configuration config : configs) releases.add(config.release);
        if (releases.size() != 1) throw new Failure("unsupported_configuration", "mixed target releases are unsupported");
        String release = releases.iterator().next();
        Target target = chooseTarget(environment, release);
        Map<String, Configuration> selected = new HashMap<>();
        for (String file : files) {
            Configuration match = null;
            for (Configuration config : configs) for (SourceRoot sourceRoot : config.roots) {
                if (file.startsWith(sourceRoot.path + "/")) {
                    if (match != null && match != config)
                        throw new Failure("unsupported_configuration", "overlapping source roots", List.of(file));
                    match = config;
                }
            }
            if (match == null) throw new Failure("unsupported_configuration", "selected file outside declared source roots", List.of(file));
            selected.put(file, match);
        }
        return new ProjectModel(root, target.path, target.id, configs, selected, files, includeTests);
    }

    /** 仅接受规范仓内相对路径。 */
    static void validateRelative(String path) throws Failure {
        if (path.isEmpty() || path.startsWith("/") || path.indexOf('\\') >= 0 || path.indexOf('\0') >= 0 ||
            path.matches("^[A-Za-z]:.*")) throw new Failure("invalid_input", "invalid relative path");
        for (String part : path.split("/", -1)) if (part.isEmpty() || part.equals(".") || part.equals(".."))
            throw new Failure("invalid_input", "invalid relative path");
    }

    /** 对每次读取执行真实路径越界检查。 */
    static Path checkedFile(Path root, String relative) throws Failure {
        try {
            Path path = root.resolve(relative).toRealPath();
            if (!path.startsWith(root) || !Files.isRegularFile(path) || Files.isSymbolicLink(root.resolve(relative)))
                throw new Failure("invalid_input", "file outside repository", List.of(relative));
            return path;
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("object_extraction_failed", "cannot read selected source", List.of(relative)); }
    }

    /** 验证目标声明与 JDK release 文件严格一致。 */
    private static Target chooseTarget(JsonObject environment, String release) throws Failure {
        Target chosen = null;
        Set<String> ids = new HashSet<>();
        for (JsonElement element : Main.array(environment, "platforms")) {
            if (!element.isJsonObject()) throw new Failure("invalid_input", "platform must be object");
            JsonObject platform = element.getAsJsonObject();
            Main.fields(platform, Set.of("id", "kind", "version", "root", "fingerprint", "metadata"));
            String id = Main.string(platform, "id");
            if (id.isEmpty() || !ids.add(id)) throw new Failure("invalid_input", "duplicate platform id");
            if (Main.string(platform, "version").isEmpty() || Main.string(platform, "fingerprint").isEmpty())
                throw new Failure("invalid_input", "empty platform version or fingerprint");
            if (!"java-jdk".equals(Main.string(platform, "kind"))) continue;
            JsonObject metadata = Main.object(platform, "metadata");
            Main.fields(metadata, Set.of("release"));
            if (!release.equals(Main.string(metadata, "release"))) continue;
            Path path = Path.of(Main.string(platform, "root"));
            if (!path.isAbsolute()) throw new Failure("invalid_input", "target JDK root must be absolute");
            try {
                path = path.toRealPath();
                String data = Files.readString(path.resolve("release"), StandardCharsets.UTF_8);
                Matcher match = Pattern.compile("(?m)^JAVA_VERSION=\"([0-9]+)(?:[.\"])").matcher(data);
                if (!match.find() || !release.equals(match.group(1)) || !Files.isRegularFile(path.resolve("lib/jrt-fs.jar")))
                    throw new Failure("target_unavailable", "catalog JDK does not match target release");
            } catch (Failure e) { throw e; }
            catch (Exception e) { throw new Failure("target_unavailable", "target JDK unavailable"); }
            if (chosen != null) throw new Failure("invalid_input", "ambiguous target JDK platforms");
            chosen = new Target(id, path);
        }
        if (chosen == null) throw new Failure("target_unavailable", "no exact target JDK for Java " + release);
        return chosen;
    }

    /** 匹配后的显式平台标识及真实 JDK 路径。 */
    private record Target(String id, Path path) {}

    /** 读取版本化的声明式源码根。 */
    private static Configuration readMetadata(Path repository, Path file) throws Failure {
        try {
            JsonObject metadata = Main.parseObject(Files.readString(file, StandardCharsets.UTF_8));
            Main.fields(metadata, Set.of("schema", "release", "source_roots"));
            Main.requireString(metadata, "schema", Main.PROFILE);
            String release = Main.string(metadata, "release");
            validateRelease(release);
            List<SourceRoot> roots = new ArrayList<>();
            for (JsonElement item : Main.array(metadata, "source_roots")) {
                if (!item.isJsonObject()) throw new Failure("unsupported_configuration", "invalid source root");
                JsonObject entry = item.getAsJsonObject();
                Main.fields(entry, Set.of("path", "role"));
                String path = Main.string(entry, "path");
                validateRelative(path);
                String role = Main.string(entry, "role");
                if (!role.equals("main") && !role.equals("test"))
                    throw new Failure("unsupported_configuration", "unknown source root role");
                if (!Files.isDirectory(repository.resolve(path)))
                    throw new Failure("unsupported_configuration", "source root missing: " + path);
                roots.add(new SourceRoot(path, role.equals("test")));
            }
            if (roots.isEmpty()) throw new Failure("unsupported_configuration", "source_roots is empty");
            return new Configuration("java-config:metadata", release, List.of("narrant-java.json"), roots);
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("unsupported_configuration", "invalid narrant-java.json"); }
    }

    /** 只读递归解释仓内 Maven 模块的受限字段。 */
    private static void readPomTree(Path repository, Path module, List<Configuration> output,
                                    Set<Path> visited, String inheritedRelease) throws Failure {
        Path pom = module.resolve("pom.xml");
        try {
            pom = pom.toRealPath();
            if (!pom.startsWith(repository) || !visited.add(pom))
                throw new Failure("unsupported_configuration", "POM cycle or external path");
            DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
            factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
            factory.setFeature("http://xml.org/sax/features/external-general-entities", false);
            factory.setFeature("http://xml.org/sax/features/external-parameter-entities", false);
            factory.setFeature(XMLConstants.FEATURE_SECURE_PROCESSING, true);
            factory.setXIncludeAware(false);
            factory.setExpandEntityReferences(false);
            Document doc;
            try (InputStream stream = Files.newInputStream(pom)) { doc = factory.newDocumentBuilder().parse(stream); }
            Element project = doc.getDocumentElement();
            if (!"project".equals(project.getTagName())) throw new Failure("unsupported_configuration", "invalid POM root");
            if (child(project, "profiles") != null || child(project, "repositories") != null ||
                child(project, "pluginRepositories") != null)
                throw new Failure("unsupported_configuration", "dynamic Maven configuration unsupported");
            Element build = child(project, "build");
            if (build != null && (child(build, "plugins") != null || child(build, "pluginManagement") != null ||
                                  child(build, "extensions") != null))
                throw new Failure("unsupported_configuration", "Maven plugins unsupported");
            Element parent = child(project, "parent");
            if (parent != null) {
                String relative = text(parent, "relativePath");
                if (relative == null) relative = "../pom.xml";
                Path parentPom = module.resolve(relative).normalize();
                if (Files.isDirectory(parentPom)) parentPom = parentPom.resolve("pom.xml");
                if (!parentPom.toRealPath().startsWith(repository))
                    throw new Failure("unsupported_configuration", "external Maven parent unsupported");
            }
            Element props = child(project, "properties");
            String release = props == null ? null : text(props, "maven.compiler.release");
            if (release == null && build != null) release = text(build, "release");
            if (release == null) release = inheritedRelease;
            Element modules = child(project, "modules");
            List<String> names = modules == null ? List.of() : texts(modules, "module");
            if (!names.isEmpty() && release == null && parent == null) {
                // 聚合 POM 可把目标 release 留给每个子模块。
            } else if (release != null) validateRelease(release);
            String modulePath = repository.relativize(module).toString().replace('\\', '/');
            String main = build == null ? null : text(build, "sourceDirectory");
            String test = build == null ? null : text(build, "testSourceDirectory");
            List<SourceRoot> roots = new ArrayList<>();
            addRoot(repository, modulePath, main == null ? "src/main/java" : main, false, roots);
            addRoot(repository, modulePath, test == null ? "src/test/java" : test, true, roots);
            if (!roots.isEmpty()) {
                if (release == null) throw new Failure("unsupported_configuration", "Maven module lacks explicit maven.compiler.release");
                output.add(new Configuration("java-config:" + (modulePath.isEmpty() ? "." : modulePath), release,
                    List.of(repository.relativize(pom).toString().replace('\\', '/')), roots));
            }
            for (String name : names) {
                validateRelative(name);
                readPomTree(repository, module.resolve(name), output, visited, release);
            }
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("unsupported_configuration", "cannot statically read POM"); }
    }

    /** 仅纳入实际存在的源码根。 */
    private static void addRoot(Path repo, String module, String local, boolean test,
                                List<SourceRoot> roots) throws Failure {
        validateRelative(local);
        String relative = module.isEmpty() ? local : module + "/" + local;
        if (Files.isDirectory(repo.resolve(relative))) roots.add(new SourceRoot(relative, test));
    }

    /** 限定首版经过验证的 release。 */
    private static void validateRelease(String release) throws Failure {
        if (!"17".equals(release))
            throw new Failure("unsupported_configuration", "unsupported Java release: " + release);
    }

    /** 查找直接 XML 子元素，避免误读嵌套插件字段。 */
    private static Element child(Element parent, String name) {
        for (Node node = parent.getFirstChild(); node != null; node = node.getNextSibling())
            if (node instanceof Element element && name.equals(element.getTagName())) return element;
        return null;
    }

    /** 读取直接 XML 子元素的字面文本。 */
    private static String text(Element parent, String name) {
        Element child = child(parent, name);
        return child == null ? null : child.getTextContent().trim();
    }

    /** 读取重复的直接 XML 子元素。 */
    private static List<String> texts(Element parent, String name) {
        List<String> result = new ArrayList<>();
        for (Node node = parent.getFirstChild(); node != null; node = node.getNextSibling())
            if (node instanceof Element element && name.equals(element.getTagName())) result.add(element.getTextContent().trim());
        return result;
    }

    /** 判断所选文件是否来自确证的测试源码根。 */
    boolean isTest(String file) {
        Configuration config = selected.get(file);
        for (SourceRoot root : config.roots) if (root.test && file.startsWith(root.path + "/")) return true;
        return false;
    }

    /** 枚举配置允许的上下文源码，拒绝外向链接。 */
    List<String> context(Configuration config) throws Failure {
        List<String> result = new ArrayList<>();
        for (SourceRoot sourceRoot : config.roots) {
            if (sourceRoot.test && !includeTests) continue;
            try (var stream = Files.walk(repository.resolve(sourceRoot.path))) {
                for (Path path : stream.filter(p -> p.toString().endsWith(".java")).toList()) {
                    String relative = repository.relativize(path).toString().replace('\\', '/');
                    checkedFile(repository, relative);
                    result.add(relative);
                }
            } catch (Failure e) { throw e; }
            catch (Exception e) { throw new Failure("object_extraction_failed", "cannot enumerate source root"); }
        }
        result.sort(Comparator.naturalOrder());
        return result;
    }
}
