import java.nio.file.*;
import java.util.*;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.dom.*;

/** 隔离检查目标 JDK API 与 recovered binding 的一次性探针。 */
public final class Probe {
    /** 只输出可核实的类型与方法绑定状态。 */
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        Path jdk = Path.of(args[1]);
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        Map<String, String> options = new HashMap<>();
        JavaCore.setComplianceOptions("17", options);
        options.put(JavaCore.COMPILER_RELEASE, JavaCore.ENABLED);
        options.put(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, JavaCore.DISABLED);
        parser.setCompilerOptions(options);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setEnvironment(new String[]{jdk.resolve("lib/jrt-fs.jar").toString()},
            new String[]{root.resolve("src").toString()}, new String[]{"UTF-8"}, false);
        String[] paths = {root.resolve("src/p/Main.java").toString(), root.resolve("src/p/Helper.java").toString()};
        parser.createASTs(paths, new String[]{"UTF-8", "UTF-8"}, new String[0], new FileASTRequestor() {
            /** 诊断与调用绑定同时输出，防止仅凭 binding 非空误判。 */
            @Override public void acceptAST(String path, CompilationUnit unit) {
                System.out.println("FILE " + Path.of(path).getFileName());
                for (var p : unit.getProblems()) System.out.println("PROBLEM " + p.getMessage());
                unit.accept(new ASTVisitor() {
                    /** 观察正常与恢复绑定。 */
                    @Override public boolean visit(MethodInvocation node) {
                        var binding = node.resolveMethodBinding();
                        System.out.println("CALL " + node + " BINDING " + (binding == null ? "null" :
                            binding.getMethodDeclaration().getKey() + " recovered=" + binding.isRecovered() +
                            " declarationRecovered=" + binding.getMethodDeclaration().isRecovered()));
                        return true;
                    }
                });
            }
        }, null);
    }
}
