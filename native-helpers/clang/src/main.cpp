// 固定版本 LibTooling helper；所有项目输入只作为受限数据解释。
#include <clang/AST/RecursiveASTVisitor.h>
#include <clang/AST/TypeLoc.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/FrontendActions.h>
#include <clang/Index/USRGeneration.h>
#include <clang/Lex/Lexer.h>
#include <clang/Lex/PPCallbacks.h>
#include <clang/Lex/MacroInfo.h>
#include <clang/Tooling/CompilationDatabase.h>
#include <clang/Tooling/Tooling.h>
#include <llvm/Support/JSON.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <llvm/Support/raw_ostream.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <future>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <thread>
#ifdef _WIN32
#define NOMINMAX
#include <windows.h>
#else
#include <poll.h>
#include <unistd.h>
#endif

namespace fs = std::filesystem;
namespace j = llvm::json;
using namespace clang;
static const char *Protocol = "narrant.native-analyzer/1";

// 受控失败保留稳定协议错误码，禁止以空图掩盖错误。
struct Failure : std::runtime_error {
  std::string code;
  Failure(std::string c, std::string m) : runtime_error(m), code(std::move(c)) {}
};
// 检查协议条件，失败时停止整个请求。
static void require(bool ok, const std::string &message, const std::string &code = "invalid_input") {
  if (!ok) throw Failure(code, message);
}
// JSON 序列化只用于协议数据，不构造命令。
static std::string jsonText(const j::Value &v) {
  std::string out; llvm::raw_string_ostream stream(out); stream << v; return out;
}
// 提取必填字符串并校验类型。
static std::string str(const j::Object &o, llvm::StringRef key) {
  auto value = o.getString(key); require(value.has_value(), "missing/string field: " + key.str());
  return value->str();
}
// 精确校验字段集合，拒绝静默扩展协议。
static void keys(const j::Object &o, std::initializer_list<const char *> names) {
  std::set<std::string> expected; for (auto n : names) expected.insert(n);
  require(o.size() == expected.size(), "unexpected/missing object fields");
  for (auto &kv : o) require(expected.count(kv.first.str()), "unknown field: " + kv.first.str());
}
// 严格扫描 JSON 容器，额外拒绝 LLVM JSON 默认允许的重复键。
class UniqueKeys {
  const std::string &s; size_t p = 0;
  // 跳过 JSON 空白。
  void ws() { while (p < s.size() && isspace(static_cast<unsigned char>(s[p]))) ++p; }
  // 读取并解码字符串键，转义后的相同键也会被拒绝。
  std::string string() {
    size_t start = p++; bool closed = false;
    while (p < s.size()) { char c = s[p++]; if (c == '\\') { if (p < s.size()) ++p; } else if (c == '"') { closed = true; break; } }
    require(closed, "unterminated JSON string");
    auto value = j::parse(s.substr(start,p-start)); require(bool(value), "invalid JSON string");
    return value->getAsString()->str();
  }
  // 递归检查每个对象的键，容器深度有界。
  void value(unsigned depth) {
    require(depth < 128, "JSON nesting limit"); ws(); require(p < s.size(), "incomplete JSON");
    if (s[p] == '"') { string(); return; }
    if (s[p] == '{' || s[p] == '[') {
      bool object = s[p++] == '{'; char end = object ? '}' : ']'; std::set<std::string> seen; ws();
      if (p < s.size() && s[p] == end) { ++p; return; }
      while (true) {
        ws(); if (object) { require(p < s.size() && s[p] == '"', "invalid JSON key"); auto key = string(); require(seen.insert(key).second, "duplicate JSON key: " + key); ws(); require(p < s.size() && s[p++] == ':', "invalid JSON colon"); }
        value(depth+1); ws(); require(p < s.size(), "incomplete JSON container");
        if (s[p] == end) { ++p; return; } require(s[p++] == ',', "invalid JSON separator");
      }
    }
    size_t start=p; while (p<s.size() && s[p]!=',' && s[p]!=']' && s[p]!='}' && !isspace(static_cast<unsigned char>(s[p]))) ++p;
    require(p>start, "invalid JSON value");
  }
public:
  // 扫描完整消息，拒绝尾随数据。
  explicit UniqueKeys(const std::string &text) : s(text) { value(0); ws(); require(p==s.size(), "trailing JSON"); }
};
// 解析有界 JSON 文档，协议和 CDB 共用严格键规则。
static j::Value parse(const std::string &text) {
  UniqueKeys check(text); auto v=j::parse(text); require(bool(v), "invalid JSON document"); return std::move(*v);
}
// 从磁盘读取小型配置；不执行文件、不写入项目。
static std::string read(const fs::path &path) {
  require(fs::is_regular_file(path), "missing file: " + path.u8string());
  require(fs::file_size(path)<=32*1024*1024, "input file too large", "resource_limit");
  std::ifstream in(path, std::ios::binary); require(bool(in), "cannot read file");
  return std::string(std::istreambuf_iterator<char>(in), {});
}
// 对路径做真实文件系统归一化，消除符号链接越界。
static fs::path real(const fs::path &path) {
  std::error_code ec; auto p=fs::canonical(path,ec); require(!ec,"unavailable path: "+path.u8string()); return p;
}
// 按路径组件判断包含关系，避免字符串前缀误判。
static bool under(const fs::path &path, const fs::path &root) {
  auto a=path.begin(), b=root.begin(); for (; b!=root.end(); ++b,++a) if (a==path.end() || *a!=*b) return false; return true;
}
// 排除已安装的业务依赖与缓存，允许普通仓内提交源码。
static bool excluded(const fs::path &relative) {
  for (const auto &part : relative) if (part=="node_modules" || part=="vendor" || part==".git" || part==".cache" || part==".venv" || part=="target") return true;
  return false;
}
// 验证协议相对路径，不接受平台相关的替代分隔符或父级跳转。
static fs::path relativePath(const std::string &s) {
  fs::path p=fs::u8path(s); require(!s.empty() && !p.is_absolute() && s.find('\\')==std::string::npos && s.find(':')==std::string::npos && s.find('\0')==std::string::npos, "invalid relative path");
  for (const auto &part : p) require(part!=".." && part!=".","invalid path component"); return p;
}
// 保存本侧已批准的目标声明路径，不包含任意编译器参数。
struct Platform {
  std::string id, triple, resourceId; fs::path root, resource; std::vector<fs::path> includes, additionalIncludes; std::vector<std::string> additionalIds;
};
// 原生分析配置；只含白名单参数及可验证来源。
struct Config { std::string id, language; fs::path file, directory; std::vector<std::string> args; size_t platform; };
// 单请求状态维护最终并集、来源和协作取消。
struct Request {
  fs::path root; std::set<std::string> selected; std::vector<Platform> platforms; std::vector<Config> configs;
  std::string id, language; std::atomic<bool> stopped{false}; size_t maxResult=0, maxDiagnostic=0;
  std::chrono::steady_clock::time_point deadline;
  // 检查取消和总期限，不被诊断日志延长。
  void checkpoint() const { if (stopped) throw Failure("cancelled","request cancelled"); if (std::chrono::steady_clock::now()>deadline) throw Failure("resource_limit","analysis deadline exceeded"); }
};
// 文件系统只向 Clang 暴露批准的仓库源码和目标头文件。
class ReadBoundary : public llvm::vfs::ProxyFileSystem {
  Request &request; Platform &platform;
  // 对每次读取重新解析链接来源，禁止仓外头文件和业务依赖缓存。
  bool allowed(const llvm::Twine &name) {
    request.checkpoint(); std::error_code ec; fs::path input=fs::u8path(name.str()); if (input.is_relative()) input=request.root/input;
    auto path=fs::canonical(input,ec); if (ec) return false;
    if (under(path,request.root)) return !excluded(path.lexically_relative(request.root));
    if (under(path,platform.resource)) return true; for (auto &inc:platform.includes) if (under(path,inc)) return true;
    return false;
  }
public:
  // 使用独立真实文件系统，避免 Clang 修改进程全局 cwd。
  ReadBoundary(Request &r, Platform &p) : ProxyFileSystem(llvm::vfs::createPhysicalFileSystem()), request(r), platform(p) {}
  // 禁止对未授权路径探测状态。
  llvm::ErrorOr<llvm::vfs::Status> status(const llvm::Twine &p) override {
    if (!allowed(p)) return std::make_error_code(std::errc::no_such_file_or_directory); return ProxyFileSystem::status(p);
  }
  // 唯一源码打开入口始终执行来源校验。
  llvm::ErrorOr<std::unique_ptr<llvm::vfs::File>> openFileForRead(const llvm::Twine &p) override {
    if (!allowed(p)) return std::make_error_code(std::errc::no_such_file_or_directory); return ProxyFileSystem::openFileForRead(p);
  }
};
// 保存原始源码声明片段；偏移仅在当前快照内定位，不直接进入公开 ID。
struct DeclInfo {
  std::string key,path,name,category,kind,parent,usr,role,params,id,code,signature,source,language,commonPrefix;
  unsigned begin=0,end=0,body=0,nameOffset=0,typeEnd=0,declaratorBegin=0; bool selected=false;
};
// 调用关系首先保留后端绑定证据，最终集合完成后再分类。
struct Edge { std::string owner,target,usr,unknown,semantic="code.calls"; bool operator<(const Edge &o) const { return std::tie(owner,target,usr,unknown,semantic)<std::tie(o.owner,o.target,o.usr,o.unknown,o.semantic); } };
// 完整单图并集先校验同一源片段的内容和父级，再生成稳定身份。
// 宏使用点在 AST 建立后按真实源码范围归属到最近 owner。
struct SourceMacroUse { std::string path,target; unsigned offset; };
struct Graph {
  Request &request; std::map<std::string,DeclInfo> decls; std::set<Edge> edges;
  std::vector<SourceMacroUse> macroUses;
  std::map<std::string,std::set<std::string>> symbolKeys;
  std::map<std::string,std::string> files; std::map<std::string,std::set<std::string>> coverage;
  // 绑定请求对象，所有并集数据仅在单次请求中生存。
  explicit Graph(Request &r) : request(r) {}
};
// 从原始缓冲区取得全部 token；最后一个有效 token 与 EOF 必须分别处理。
static std::vector<Token> rawTokens(const std::string &source) {
  LangOptions opts; opts.CPlusPlus=true; opts.CPlusPlus11=true; opts.CPlusPlus20=true; opts.LineComment=true;
  Lexer lexer(SourceLocation(),opts,source.data(),source.data(),source.data()+source.size());
  lexer.SetCommentRetentionState(false); std::vector<Token> tokens;
  while(true) {
    Token token; bool atEnd=lexer.LexFromRawLexer(token);
    if(token.is(tok::eof)) break;
    tokens.push_back(token);
    if(atEnd) break;
  }
  return tokens;
}
// 在原始 token 上去注释和排版，保留字面量及预处理指令边界。
static std::string normalize(const std::string &source) {
  std::string out; bool directive=false;
  for(auto &t:rawTokens(source)) {
    if (t.isAtStartOfLine() && directive) { out+="\n"; directive=false; }
    if (t.isAtStartOfLine() && t.is(tok::hash)) directive=true;
    if (!out.empty() && out.back()!='\n') out+=' ';
    out.append(source.data()+t.getLocation().getRawEncoding(),t.getLength());
  }
  return out;
}
// 访问 Clang 的真实 AST，不经过 AST JSON 转储或项目编译命令。
class Collector : public RecursiveASTVisitor<Collector> {
  ASTContext &context; SourceManager &sm; Graph &graph; std::string config;
  std::map<const Decl *,std::string> known; std::vector<std::string> owners;
  // 得到物理文件位置；#line 与宏定义位置不取代真实声明来源。
  std::pair<std::string,unsigned> location(SourceLocation loc) {
    loc=sm.getExpansionLoc(loc); if (loc.isInvalid() || !loc.isFileID()) return {};
    auto file=sm.getFilename(loc); if (file.empty()) return {};
    std::error_code ec; auto p=fs::canonical(fs::u8path(file.str()),ec); if(ec) return {};
    if (!under(p,graph.request.root)) return {};
    auto rel=p.lexically_relative(graph.request.root).generic_u8string(); return {rel,sm.getFileOffset(loc)};
  }
  // 仅显式承诺的对象成为节点，其余隐式或局部对象内联于 owner。
  std::string category(const Decl *d) {
    if (d->isImplicit() || isa<ParmVarDecl>(d)) return {};
    if (auto f=dyn_cast<FunctionDecl>(d)) { if (f->getTemplateSpecializationKind()==TSK_ImplicitInstantiation) return {}; return "callable"; }
    if (auto r=dyn_cast<CXXRecordDecl>(d)) if (r->isInjectedClassName() || (isa<ClassTemplateSpecializationDecl>(r) && cast<ClassTemplateSpecializationDecl>(r)->getSpecializationKind()==TSK_ImplicitInstantiation)) return {};
    if (isa<NamespaceDecl>(d) || isa<NamespaceAliasDecl>(d)) return "container";
    if (isa<StaticAssertDecl>(d) || isa<UsingDecl>(d) || isa<UsingDirectiveDecl>(d) || isa<LinkageSpecDecl>(d)) return "extension";
    if (isa<FileScopeAsmDecl>(d)) return "execution_region";
    if (isa<ConceptDecl>(d)) return "type";
    if (isa<RecordDecl>(d) || isa<EnumDecl>(d) || isa<TypedefNameDecl>(d)) return "type";
    if (auto v=dyn_cast<VarDecl>(d)) { if(v->isLocalVarDecl()) return {}; return "value"; }
    if(isa<FieldDecl>(d) || isa<EnumConstantDecl>(d)) return "value";
    return {};
  }
  // 使用后端 USR 作为绑定证据，绝不直接当作源码节点身份。
  std::string usr(const Decl *d) {
    llvm::SmallString<128> text; if(index::generateUSRForDecl(d,text)) return {}; return text.str().str();
  }
  // 截取同一物理文件中的原始 token 范围，宏展开保持调用处文本。
  std::pair<unsigned,unsigned> range(SourceRange range, const std::string &path) {
    auto begin=location(range.getBegin()), end=location(range.getEnd());
    require(begin.first==path && end.first==path,"non-contiguous declaration source","object_extraction_failed");
    auto loc=sm.getExpansionRange(range.getEnd()).getEnd(); loc=sm.getExpansionLoc(loc);
    unsigned length=Lexer::MeasureTokenLength(loc,sm,context.getLangOpts());
    return {begin.second,end.second+length};
  }
  // 建立源位置内部键，同一位置换类别或父级会在并集阶段冲突。
  std::string keyFor(const Decl *d) {
    auto p=location(d->getLocation()); return p.first.empty()?std::string():p.first+":"+std::to_string(p.second);
  }
  // 添加一个声明，保留原始文本而不使用推导类型打印结果。
  std::string add(Decl *d) {
    auto cat=category(d); if(cat.empty()) return {};
    auto loc=location(d->getLocation()); if(loc.first.empty()) return {};
    require(!excluded(loc.first),"excluded dependency source","object_extraction_failed");
    auto *named=dyn_cast<NamedDecl>(d); std::string name=named?named->getNameAsString():""; if(name.empty()) name="<anonymous>";
    auto sourceRange=d->getSourceRange();
    if(auto f=dyn_cast<FunctionDecl>(d)) if(auto t=f->getDescribedFunctionTemplate()) sourceRange=t->getSourceRange();
    if(auto r=dyn_cast<CXXRecordDecl>(d)) if(auto t=r->getDescribedClassTemplate()) sourceRange=t->getSourceRange();
    auto span=range(sourceRange,loc.first); auto key=keyFor(d);
    auto &source=graph.files[loc.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(loc.first));
    require(span.second<=source.size() && span.first<span.second,"invalid declaration range","object_extraction_failed");
    unsigned end=span.second; if(end<source.size() && source[end]==';') ++end;
    DeclInfo n; n.key=key; n.path=loc.first; n.name=name; n.category=cat; n.kind=d->getDeclKindName(); n.begin=span.first; n.end=end; n.body=end; n.nameOffset=loc.second;
    n.language=context.getLangOpts().CPlusPlus?"cpp":"c";
    if(auto tag=dyn_cast<RecordDecl>(d)) n.kind=tag->isUnion()?"UnionDecl":tag->isStruct()?"StructDecl":"ClassDecl";
    n.selected=graph.request.selected.count(loc.first); n.usr=usr(d); if(!n.usr.empty()) graph.symbolKeys[n.usr].insert(key); n.parent=owners.empty()?"":owners.back();
    if(auto f=dyn_cast<FunctionDecl>(d)) {
      n.role=f->doesThisDeclarationHaveABody()?"definition":"declaration";
      if(f->doesThisDeclarationHaveABody()) { auto b=location(f->getBody()->getBeginLoc()); require(b.first==n.path,"unsupported function body source","object_extraction_failed"); n.body=b.second; }
      // 源参数类型组成重载槽；参数名不是类型的一部分。
      for(auto p:f->parameters()) {
        auto pr=range(p->getSourceRange(),n.path); std::string raw=source.substr(pr.first,pr.second-pr.first);
        auto pl=location(p->getLocation()); if(!p->getName().empty() && pl.first==n.path && pl.second>=pr.first && pl.second+p->getName().size()<=pr.second) raw.erase(pl.second-pr.first,p->getName().size());
        auto equal=raw.find('='); if(equal!=std::string::npos) raw.resize(equal);
        n.params+=normalize(raw)+";";
      }
      if(f->isVariadic()) n.params+="...";
      if(auto m=dyn_cast<CXXMethodDecl>(f)) { if(m->isConst()) n.params+=" const"; if(m->isVolatile()) n.params+=" volatile"; n.params+=" ref"+std::to_string(m->getRefQualifier()); }
    } else if(auto t=dyn_cast<TagDecl>(d)) { n.role=t->isThisDeclarationADefinition()?"definition":"declaration"; if(t->getBraceRange().getBegin().isValid()) n.body=location(t->getBraceRange().getBegin()).second; }
    if(auto linkage=dyn_cast<LinkageSpecDecl>(d)) {
      auto language=location(linkage->getLocation());
      n.body=language.second+Lexer::MeasureTokenLength(sm.getExpansionLoc(linkage->getLocation()),sm,context.getLangOpts());
    }
    if(isa<NamespaceDecl>(d)) {
      for(auto &token:rawTokens(source.substr(n.begin,n.end-n.begin))) if(token.is(tok::l_brace)) { n.body=n.begin+token.getLocation().getRawEncoding(); break; }
    }
    if((isa<VarDecl>(d) || isa<FieldDecl>(d))) if(auto declarator=dyn_cast<DeclaratorDecl>(d)) if(auto info=declarator->getTypeSourceInfo()) {
      TypeLoc type=info->getTypeLoc(); std::vector<unsigned> declaratorStarts;
      // 每层原生 TypeLoc 提供自身范围；MemberPointerTypeLoc 的范围包含完整 C:: 限定名。
      while(!type.getNextTypeLoc().isNull()) {
        auto local=location(type.getLocalSourceRange().getBegin());
        if(local.first==n.path) declaratorStarts.push_back(local.second);
        type=type.getNextTypeLoc();
      }
      auto typeRange=range(type.getSourceRange(),n.path); n.typeEnd=typeRange.second; n.declaratorBegin=n.nameOffset;
      for(auto start:declaratorStarts) if(start>=n.typeEnd && start<n.declaratorBegin) n.declaratorBegin=start;
    }
    n.signature=normalize(source.substr(n.begin,n.body-n.begin));
    n.source=source.substr(n.begin,n.end-n.begin);
    auto found=graph.decls.find(key);
    if(found!=graph.decls.end()) {
      auto &o=found->second;
      if(n.language=="c") o.language="c";
      require(std::tie(o.name,o.category,o.kind,o.parent,o.begin,o.end,o.signature,o.params,o.role)==std::tie(n.name,n.category,n.kind,n.parent,n.begin,n.end,n.signature,n.params,n.role),"incompatible source declaration: "+n.path+" "+n.name,"union_conflict");
    } else graph.decls.emplace(key,std::move(n));
    known[d]=key; return key;
  }
public:
  // 绑定当前 TU 的 AST 与对应配置来源。
  Collector(ASTContext &c, Graph &g, std::string id):context(c),sm(c.getSourceManager()),graph(g),config(std::move(id)) {}
  // 只遍历显式源模板，不把编译器实例化产物复制成节点。
  bool shouldVisitTemplateInstantiations() const { return false; }
  // 自定义声明栈，精确保存词法包含关系及调用 owner。
  bool TraverseDecl(Decl *d) {
    if(!d) return true; graph.request.checkpoint();
    if(d->isImplicit()) return true;
    if(auto f=dyn_cast<FunctionDecl>(d)) if(f->getTemplateSpecializationKind()==TSK_ImplicitInstantiation) return true;
    if(auto r=dyn_cast<ClassTemplateSpecializationDecl>(d)) if(r->getSpecializationKind()==TSK_ImplicitInstantiation) return true;
    auto key=add(d); if(!key.empty()) owners.push_back(key);
    bool ok=RecursiveASTVisitor<Collector>::TraverseDecl(d); if(!key.empty()) owners.pop_back(); return ok;
  }
  // 直接调用依据 Clang 绑定；函数指针和依赖调用保留未知观测。
  bool VisitCallExpr(CallExpr *call) {
    if(owners.empty()) return true; Edge e; e.owner=owners.back();
    if(auto callee=call->getDirectCallee()) { e.target=keyFor(callee); e.usr=usr(callee); if(e.target.empty()) e.unknown=callee->getQualifiedNameAsString(); }
    else { auto p=location(call->getExprLoc()); if(!p.first.empty()) { auto r=range(call->getCallee()->getSourceRange(),p.first); auto &source=graph.files[p.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(p.first)); e.unknown=normalize(source.substr(r.first,r.second-r.first)); } }
    if(e.target.empty() && e.usr.empty() && e.unknown.empty()) e.unknown="<indirect-call>";
    graph.edges.insert(std::move(e)); return true;
  }
  // 构造表达式同样按真实构造函数绑定，不按类名猜测。
  bool VisitCXXConstructExpr(CXXConstructExpr *call) {
    if(owners.empty() || call->getConstructor()->isImplicit()) return true;
    graph.edges.insert({owners.back(),keyFor(call->getConstructor()),usr(call->getConstructor()),{}}); return true;
  }
};
// 宏定义本身是显式源对象，避免仅修改宏正文却得到相同图。
class Macros : public PPCallbacks {
  SourceManager &sm; const LangOptions &options; Graph &graph;
  // 取得真实仓内位置，平台宏只作为语义上下文。
  std::pair<std::string,unsigned> where(SourceLocation loc) {
    loc=sm.getExpansionLoc(loc); if(loc.isInvalid() || !loc.isFileID()) return {};
    auto file=sm.getFilename(loc); if(file.empty()) return {};
    std::error_code ec; auto path=fs::canonical(fs::u8path(file.str()),ec); if(ec || !under(path,graph.request.root)) return {};
    return {path.lexically_relative(graph.request.root).generic_u8string(),sm.getFileOffset(loc)};
  }
public:
  // 使用当前 TU 的原生预处理器记录定义与展开。
  Macros(SourceManager &manager,const LangOptions &lang,Graph &g):sm(manager),options(lang),graph(g) {}
  // 原始定义正文参与内容比较，包括未生成独立 AST 节点的数字宏。
  void MacroDefined(const Token &name,const MacroDirective *directive) override {
    auto info=directive->getMacroInfo(); auto pos=where(name.getLocation()); if(pos.first.empty()) return;
    auto end=where(info->getDefinitionEndLoc()); require(end.first==pos.first,"nonlocal macro definition","object_extraction_failed");
    auto &source=graph.files[pos.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(pos.first));
    unsigned begin=pos.second; while(begin>0 && source[begin-1]!='\n') --begin;
    unsigned finish=end.second+Lexer::MeasureTokenLength(sm.getExpansionLoc(info->getDefinitionEndLoc()),sm,options);
    DeclInfo n; n.key=pos.first+":"+std::to_string(pos.second); n.path=pos.first; n.begin=begin; n.end=finish; n.body=finish;
    n.name=name.getIdentifierInfo()->getName().str(); n.category="extension"; n.kind="MacroDefinition"; n.role="definition";
    n.language=options.CPlusPlus?"cpp":"c"; n.selected=graph.request.selected.count(pos.first); n.source=source.substr(begin,finish-begin); n.signature="#define "+n.name;
    auto found=graph.decls.find(n.key); if(found==graph.decls.end()) graph.decls.emplace(n.key,std::move(n)); else if(n.language=="c") found->second.language="c";
  }
  // 只记录 Clang 确认展开的宏，不按同名 token 猜引用。
  void MacroExpands(const Token &name,const MacroDefinition &definition,SourceRange,const MacroArgs *) override {
    auto source=where(name.getLocation()); auto info=definition.getMacroInfo(); if(source.first.empty() || !info) return;
    auto target=where(info->getDefinitionLoc()); if(target.first.empty()) return;
    graph.macroUses.push_back({source.first,target.first+":"+std::to_string(target.second),source.second});
  }
};

// 只在 AST 完整解析后执行声明提取，任何诊断错误都使整次请求失败。
class Consumer : public ASTConsumer {
  Graph &graph; std::string config;
public:
  // 绑定本次配置。
  Consumer(Graph &g,std::string id):graph(g),config(std::move(id)) {}
  // 消费原生 AST 并确认文件覆盖。
  void HandleTranslationUnit(ASTContext &context) override {
    require(!context.getDiagnostics().hasErrorOccurred(),"Clang cannot extract complete objects","object_extraction_failed");
    Collector c(context,graph,config); c.TraverseDecl(context.getTranslationUnitDecl());
    auto &sm=context.getSourceManager();
    for(auto &file:graph.request.selected) {
      auto entry=sm.getFileManager().getOptionalFileRef((graph.request.root/fs::u8path(file)).u8string());
      if(entry && sm.translateFile(*entry).isValid()) graph.coverage[file].insert(config);
    }
  }
};
// 纯语法前端 action 不执行代码生成、链接、插件或项目逻辑。
class Action : public ASTFrontendAction {
  Graph &graph; std::string config;
public:
  // 绑定单图并集与配置。
  Action(Graph &g,std::string id):graph(g),config(std::move(id)) {}
  // 创建 AST consumer，不允许项目自定义 action。
  std::unique_ptr<ASTConsumer> CreateASTConsumer(CompilerInstance &instance,llvm::StringRef) override { instance.getPreprocessor().addPPCallbacks(std::make_unique<Macros>(instance.getSourceManager(),instance.getLangOpts(),graph)); return std::make_unique<Consumer>(graph,config); }
};
// ClangTool 为每个受控 TU 创建单一前端 action。
class Factory : public tooling::FrontendActionFactory {
  Graph &graph; std::string config;
public:
  // 初始化只读 action 工厂。
  Factory(Graph &g,std::string id):graph(g),config(std::move(id)) {}
  // 返回真实 LibTooling action。
  std::unique_ptr<FrontendAction> create() override { return std::make_unique<Action>(graph,config); }
};
// 有界收集诊断，避免 Clang 直接污染 stdout/stderr 或隐藏错误。
class Diagnostics : public DiagnosticConsumer {
  size_t limit,used=0;
public:
  std::string firstError;
  // 固定整次 TU 的最大诊断字节数。
  explicit Diagnostics(size_t max):limit(max) {}
  // 格式化诊断只用于错误信息，不改变成功标准。
  void HandleDiagnostic(DiagnosticsEngine::Level level,const Diagnostic &info) override {
    DiagnosticConsumer::HandleDiagnostic(level,info); llvm::SmallString<256> text; info.FormatDiagnostic(text); used+=text.size();
    require(used<=limit,"diagnostic byte limit exceeded","resource_limit");
    if(level>=DiagnosticsEngine::Error && firstError.empty()) firstError=text.str().str();
  }
};
// 校验平台 catalog；SDK 与工具 resource headers 分别来自批准根。
static void platforms(Request &r,const j::Array &catalog) {
  std::map<std::string,const j::Object *> entries;
  for(auto &v:catalog) {
    auto o=v.getAsObject(); require(o,"platform must be an object"); keys(*o,{"id","kind","version","root","fingerprint","metadata"});
    auto id=str(*o,"id"); require(!id.empty() && entries.emplace(id,o).second,"duplicate/empty platform id");
    require(!str(*o,"version").empty() && !str(*o,"fingerprint").empty(),"empty platform provenance");
  }
  for(auto &entry:entries) {
    auto &o=*entry.second; auto kind=str(o,"kind"); require(kind=="clang-target" || kind=="clang-resource" || kind=="clang-system-headers","unsupported platform kind");
    auto meta=o.getObject("metadata"); require(meta,"platform metadata must be an object");
    if(kind=="clang-system-headers") { keys(*meta,{}); require(fs::u8path(str(o,"root")).is_absolute() && fs::is_directory(real(fs::u8path(str(o,"root")))),"invalid system header root"); continue; }
    if(kind=="clang-resource") { keys(*meta,{}); require(str(o,"version")=="21.1.8","incompatible resource headers","target_unavailable"); continue; }
    auto required=j::Object(*meta); required.erase("additional_system_platform_ids");
    keys(required,{"target_triple","c_standard","cpp_standard","system_include_directories","resource_platform_id"});
    Platform p; p.id=entry.first; p.root=real(fs::u8path(str(o,"root"))); require(fs::u8path(str(o,"root")).is_absolute(),"platform root must be absolute");
    p.triple=str(*meta,"target_triple"); require(!p.triple.empty() && p.triple.find_first_of(" \t\r\n") == std::string::npos,"invalid target triple");
    require(str(*meta,"c_standard")=="c17" && str(*meta,"cpp_standard")=="c++20","unsupported language standard","target_unavailable");
    p.resourceId=str(*meta,"resource_platform_id"); auto resource=entries.find(p.resourceId); require(resource!=entries.end() && str(*resource->second,"kind")=="clang-resource","missing resource platform","target_unavailable");
    require(str(*resource->second,"version")=="21.1.8","incompatible resource headers","target_unavailable");
    p.resource=real(fs::u8path(str(*resource->second,"root"))); require(fs::u8path(str(*resource->second,"root")).is_absolute(),"resource root must be absolute");
    auto dirs=meta->getArray("system_include_directories"); require(dirs,"system include directories must be an array");
    for(auto &d:*dirs) { auto s=d.getAsString(); require(s.has_value(),"include directory must be string"); auto dir=real(p.root/relativePath(s->str())); require(under(dir,p.root) && fs::is_directory(dir),"include directory escapes platform"); p.includes.push_back(dir); }
    if(auto value=meta->get("additional_system_platform_ids")) {
      auto ids=value->getAsArray(); require(ids,"additional system platform IDs must be an array"); std::set<std::string> seen;
      for(auto &idValue:*ids) {
        auto id=idValue.getAsString(); require(id && !id->empty() && seen.insert(id->str()).second && *id!=p.id,"invalid additional system platform ID");
        auto found=entries.find(id->str()); require(found!=entries.end() && str(*found->second,"kind")=="clang-system-headers","missing additional system header platform","target_unavailable");
        auto root=real(fs::u8path(str(*found->second,"root"))); p.includes.push_back(root); p.additionalIncludes.push_back(root); p.additionalIds.push_back(id->str());
      }
    }
    r.platforms.push_back(std::move(p));
  }
  require(!r.platforms.empty(),"no approved Clang target platform","target_unavailable");
}
// 编译数据库参数是数据；只重建经过白名单校验的编译上下文。
static Config configuration(Request &r,const j::Object &o,size_t index) {
  for(auto &kv:o) require(kv.first=="directory" || kv.first=="file" || kv.first=="arguments" || kv.first=="output","unsupported compilation database field","unsupported_configuration");
  Config c; c.id="tu-"+std::to_string(index); c.directory=real(fs::u8path(str(o,"directory"))); require(under(c.directory,r.root),"CDB directory outside repository","unsupported_configuration");
  auto path=fs::u8path(str(o,"file")); c.file=real(path.is_absolute()?path:c.directory/path); require(under(c.file,r.root) && !excluded(c.file.lexically_relative(r.root)),"CDB source outside allowed repository","unsupported_configuration");
  auto args=o.getArray("arguments"); require(args && !args->empty(),"CDB requires arguments array, command strings are not executable input","unsupported_configuration");
  std::vector<std::string> input; for(auto &v:*args) { auto s=v.getAsString(); require(s.has_value() && s->find('\0')==llvm::StringRef::npos,"invalid CDB argument"); input.push_back(s->str()); }
  auto driver=fs::u8path(input.front()).filename().u8string(); require(driver=="clang" || driver=="clang++" || driver=="clang.exe" || driver=="clang++.exe" || driver=="cc" || driver=="c++" || driver=="gcc" || driver=="g++","compiler wrappers are unsupported","unsupported_configuration");
  std::string triple,standard; std::vector<fs::path> roots; bool sourceSeen=false;
  c.language=c.file.extension()==".c"?"c":"cpp";
  // 参数值必须存在，禁止忽略截断的命令。
  auto next=[&](size_t &i)->std::string { require(i+1<input.size(),"missing CDB argument value","unsupported_configuration"); return input[++i]; };
  // 仓内 include 目录也必须排除安装缓存和符号链接越界。
  auto include=[&](std::string value)->std::string {
    fs::path p=fs::u8path(value); auto resolved=real(p.is_absolute()?p:c.directory/p);
    bool allowed=under(resolved,r.root) && !excluded(resolved.lexically_relative(r.root));
    for(auto &platform:r.platforms) for(auto &dir:platform.includes) if(under(resolved,dir)) allowed=true;
    require(allowed && fs::is_directory(resolved),"unapproved include directory","unsupported_configuration"); return resolved.u8string();
  };
  for(size_t i=1;i<input.size();++i) {
    auto a=input[i];
    if(a=="-c" || a=="-fsyntax-only" || a=="-g") continue;
    if(a=="-O0" || a=="-O1" || a=="-O2" || a=="-O3" || a=="-Os" || a=="-Oz" || a=="-Og") { c.args.push_back(a); continue; }
    if(a=="-o") { next(i); continue; }
    if(a=="-x") { auto mode=next(i); require(mode=="c" || mode=="c++","unsupported language mode","unsupported_configuration"); c.language=mode=="c"?"c":"cpp"; continue; }
    if(a.rfind("-std=",0)==0) { standard=a.substr(5); continue; }
    if(a=="-target" || a=="--target") { triple=next(i); continue; }
    if(a.rfind("--target=",0)==0) { triple=a.substr(9); continue; }
    if(a=="-isysroot" || a=="--sysroot") { roots.push_back(real(fs::u8path(next(i)))); continue; }
    if(a.rfind("--sysroot=",0)==0) { roots.push_back(real(fs::u8path(a.substr(10)))); continue; }
    if(a=="-I" || a=="-isystem" || a=="-iquote") { c.args.push_back(a); c.args.push_back(include(next(i))); continue; }
    if(a.rfind("-I",0)==0 && a.size()>2) { c.args.push_back("-I"+include(a.substr(2))); continue; }
    if(a=="-D" || a=="-U") { c.args.push_back(a+next(i)); continue; }
    if((a.rfind("-D",0)==0 || a.rfind("-U",0)==0) && a.size()>2) { c.args.push_back(a); continue; }
    if(!a.empty() && a[0]!='-' && a[0]!='@') {
      fs::path f=fs::u8path(a); std::error_code ec; auto resolved=fs::canonical(f.is_absolute()?f:c.directory/f,ec);
      require(!ec && resolved==c.file && !sourceSeen,"unexpected compilation input","unsupported_configuration"); sourceSeen=true; continue;
    }
    throw Failure("unsupported_configuration","unapproved compiler argument: "+a);
  }
  require(sourceSeen,"CDB arguments omit source file","unsupported_configuration");
  require(standard==(c.language=="c"?"c17":"c++20"),"explicit C17/C++20 standard required","unsupported_configuration");
  std::vector<size_t> matches;
  for(size_t i=0;i<r.platforms.size();++i) {
    auto &p=r.platforms[i]; if(!triple.empty() && triple!=p.triple) continue;
    if(std::any_of(roots.begin(),roots.end(),[&](auto &root){return root!=p.root;})) continue; matches.push_back(i);
  }
  require(matches.size()==1,"target platform missing or ambiguous","target_unavailable"); c.platform=matches[0]; auto &p=r.platforms[c.platform];
  std::vector<std::string> fixed={"-x",c.language=="c"?"c":"c++","-std="+standard,"--target="+p.triple,"-fsyntax-only","-nostdinc","-nostdinc++","-fno-modules","-fno-implicit-modules","-fno-implicit-module-maps","-fno-delayed-template-parsing","-pedantic-errors","-resource-dir",p.resource.u8string(),"-isysroot",p.root.u8string()};
  // 标准搜索次序为 C++ 包装头、编译器内建头、目标 C 头；否则 include_next 会选错 stddef。
  auto isCxx=[](const fs::path &dir) { return std::any_of(dir.begin(),dir.end(),[](const fs::path &part){return part=="c++";}); };
  for(auto &dir:p.includes) if(isCxx(dir) || std::find(p.additionalIncludes.begin(),p.additionalIncludes.end(),dir)!=p.additionalIncludes.end()) { fixed.push_back("-isystem"); fixed.push_back(dir.u8string()); }
  fixed.push_back("-isystem"); fixed.push_back((p.resource/"include").u8string());
  for(auto &dir:p.includes) if(!isCxx(dir) && std::find(p.additionalIncludes.begin(),p.additionalIncludes.end(),dir)==p.additionalIncludes.end()) { fixed.push_back("-isystem"); fixed.push_back(dir.u8string()); }
  fixed.insert(fixed.end(),c.args.begin(),c.args.end()); c.args=std::move(fixed); return c;
}
// 校验请求、catalog 和静态 CDB；不运行项目发现脚本。
static void configure(Request &r,const j::Object &message) {
  keys(message,{"protocol","type","id","input","environment","limits"});
  require(str(message,"protocol")==Protocol && str(message,"type")=="analyze","invalid analyze envelope"); r.id=str(message,"id"); require(!r.id.empty(),"empty request id");
  auto in=message.getObject("input"), env=message.getObject("environment"), limits=message.getObject("limits"); require(in && env && limits,"invalid analyze objects");
  keys(*in,{"repository_root","files","include_tests","profile"}); keys(*env,{"scratch_root","platforms"}); keys(*limits,{"analysis_timeout_ms","max_result_bytes","max_diagnostic_bytes"});
  require(str(*in,"profile")=="clang-static-v1","unsupported profile"); require(in->getBoolean("include_tests").has_value(),"invalid include_tests");
  require(fs::u8path(str(*in,"repository_root")).is_absolute(),"repository root must be absolute"); r.root=real(fs::u8path(str(*in,"repository_root")));
  auto scratch=real(fs::u8path(str(*env,"scratch_root"))); require(scratch.is_absolute() && !under(scratch,r.root),"scratch must be outside repository");
  auto timeout=limits->getInteger("analysis_timeout_ms"), result=limits->getInteger("max_result_bytes"), diagnostic=limits->getInteger("max_diagnostic_bytes");
  require(timeout && *timeout>0 && result && *result>0 && diagnostic && *diagnostic>0,"limits must be positive integers");
  r.deadline=std::chrono::steady_clock::now()+std::chrono::milliseconds(*timeout); r.maxResult=*result; r.maxDiagnostic=*diagnostic;
  auto files=in->getArray("files"), catalog=env->getArray("platforms"); require(files && catalog,"invalid files/platforms array");
  for(auto &v:*files) { auto s=v.getAsString(); require(s.has_value(),"file must be string"); auto rel=relativePath(s->str()); auto full=real(r.root/rel); require(under(full,r.root) && !excluded(rel) && fs::is_regular_file(full),"source outside allowed repository"); require(r.selected.insert(rel.generic_u8string()).second,"duplicate source file"); }
  platforms(r,*catalog);
  auto path=r.root/"compile_commands.json"; require(fs::exists(path),"clang-static-v1 requires compile_commands.json","unsupported_configuration"); require(under(real(path),r.root),"CDB symlink escapes repository");
  auto cdb=parse(read(path)); auto entries=cdb.getAsArray(); require(entries && !entries->empty(),"CDB must be a nonempty array","unsupported_configuration");
  for(auto &v:*entries) { r.checkpoint(); auto o=v.getAsObject(); require(o,"CDB entry must be object"); auto c=configuration(r,*o,r.configs.size()); r.configs.push_back(std::move(c)); }
}
// 对身份字段进行无歧义转义，保持相对路径和源声明槽位可复查。
static std::string escape(const std::string &s) {
  static const char *hex="0123456789ABCDEF"; std::string out;
  for(unsigned char c:s) { if(isalnum(c) || c=='_' || c=='-' || c=='.' || c=='/') out+=c; else { out+='%'; out+=hex[c>>4]; out+=hex[c&15]; } } return out;
}
// 按 AST 基础类型范围拆分共用声明前缀，保持每个 declarator 的初始化内容独立。
static void splitDeclarators(Graph &graph) {
  std::map<std::tuple<std::string,std::string,unsigned>,std::vector<DeclInfo *>> groups;
  for(auto &entry:graph.decls) { auto &n=entry.second; if(n.typeEnd) groups[{n.path,n.parent,n.begin}].push_back(&n); }
  for(auto &group:groups) {
    auto &items=group.second; if(items.size()<2) continue;
    std::sort(items.begin(),items.end(),[](auto a,auto b){return a->nameOffset<b->nameOffset;});
    auto &first=*items.front(); auto &source=graph.files.at(first.path);
    require(first.typeEnd<=first.nameOffset,"unsupported shared declarator type range","object_extraction_failed");
    require(first.declaratorBegin>=first.typeEnd && first.declaratorBegin<=first.nameOffset,"invalid native declarator range","object_extraction_failed");
    std::string prefix=source.substr(first.begin,first.declaratorBegin-first.begin);
    unsigned previousEnd=first.end;
    for(size_t i=1;i<items.size();++i) {
      auto &n=*items[i]; require(previousEnd<=n.nameOffset,"overlapping declarator initializer","object_extraction_failed");
      auto tokens=rawTokens(source.substr(previousEnd,n.nameOffset-previousEnd));
      require(!tokens.empty() && tokens.front().is(tok::comma),"missing declarator separator","object_extraction_failed");
      unsigned begin=tokens.size()>1?previousEnd+tokens[1].getLocation().getRawEncoding():n.nameOffset;
      n.begin=begin; n.commonPrefix=prefix;
      n.signature=normalize(prefix+source.substr(begin,n.body-begin));
      previousEnd=n.end;
    }
  }
}
// 构造完成后的单图 DTO；任何未覆盖文件或不兼容归并都硬失败。
static j::Object finish(Graph &g) {
  splitDeclarators(g);
  auto &r=g.request; std::map<std::string,std::vector<DeclInfo *>> slots;
  for(auto &use:g.macroUses) {
    DeclInfo *owner=nullptr;
    for(auto &entry:g.decls) { auto &n=entry.second; if(n.path==use.path && n.kind!="MacroDefinition" && n.begin<=use.offset && use.offset<n.end && (!owner || n.end-n.begin<owner->end-owner->begin)) owner=&n; }
    if(owner && g.decls.count(use.target)) g.edges.insert({owner->key,use.target,{}, {},"code.references"});
  }
  size_t assigned=0;
  while(assigned<g.decls.size()) {
    slots.clear();
    for(auto &entry:g.decls) {
      auto &n=entry.second; if(!n.id.empty()) continue;
      if(!n.parent.empty() && g.decls.at(n.parent).id.empty()) continue;
      auto owner=n.parent.empty()?n.path:g.decls.at(n.parent).id;
      auto slot=owner+"#"+n.name+"("+n.params+")#"+n.role; slots[slot].push_back(&n);
    }
    require(!slots.empty(),"cyclic declaration containment","union_conflict");
    for(auto &slot:slots) { auto &items=slot.second; std::sort(items.begin(),items.end(),[](auto a,auto b){return a->begin<b->begin;}); for(size_t i=0;i<items.size();++i) { items[i]->id=items[i]->language+":"+escape(slot.first)+"#"+std::to_string(i); ++assigned; } }
  }
  std::map<std::string,std::vector<DeclInfo *>> children; std::map<std::string,std::vector<DeclInfo *>> symbols;
  for(auto &entry:g.decls) { auto &n=entry.second; if(!n.parent.empty()) children[n.parent].push_back(&n);  }
  for(auto &symbol:g.symbolKeys) for(auto &key:symbol.second) symbols[symbol.first].push_back(&g.decls.at(key));
  j::Array nodes,origins,roots,externals; std::map<std::string,j::Object> externalMap;
  for(auto &entry:g.decls) {
    r.checkpoint(); auto &n=entry.second; if(!n.selected) continue; j::Array contains,refs; std::set<std::string> refKeys;
    auto raw=g.files[n.path].substr(n.begin,n.end-n.begin); auto child=children[n.key]; std::sort(child.begin(),child.end(),[](auto a,auto b){return a->begin>b->begin;}); unsigned boundary=n.end;
    for(auto c:child) {
      require(c->path==n.path && c->begin>=n.begin && c->end<=boundary,"overlapping/nonlocal child declaration","object_extraction_failed");
      require(c->begin>n.begin || c->end<n.end,"ambiguous equal-span declarations","object_extraction_failed"); boundary=c->begin;
      raw.replace(c->begin-n.begin,c->end-c->begin," __narrant_child_"+escape(c->id)+" "); if(c->selected) contains.push_back(c->id);
    }
    n.code=normalize(n.commonPrefix+raw);
    if(n.parent.empty() || !g.decls.at(n.parent).selected) roots.push_back(n.id);
    for(auto &edge:g.edges) {
      if(edge.owner!=n.key) continue; j::Object target; DeclInfo *destination=nullptr;
      auto exact=g.decls.find(edge.target); if(exact!=g.decls.end()) destination=&exact->second;
      auto sym=symbols.find(edge.usr);
      if(!edge.usr.empty() && sym!=symbols.end()) {
        std::vector<DeclInfo *> definitions; for(auto d:sym->second) if(d->role=="definition") definitions.push_back(d);
        if(definitions.size()==1) destination=definitions.front();
      }
      if(destination && destination->selected) target=j::Object{{"kind","node"},{"id",destination->id}};
      else if(destination) { target=j::Object{{"kind","external"},{"symbol",destination->id}}; externalMap.try_emplace(destination->id,j::Object{{"symbol",destination->id},{"name",destination->name},{"scope","repository"}}); }
      else if(!edge.usr.empty() && edge.target.empty()) {
        auto symbol=n.language+":dependency:"+escape(edge.usr); target=j::Object{{"kind","external"},{"symbol",symbol}};
        externalMap.try_emplace(symbol,j::Object{{"symbol",symbol},{"name",edge.unknown.empty()?edge.usr:edge.unknown},{"scope","dependency"}});
      } else target=j::Object{{"kind","unresolved"},{"name",edge.unknown.empty()?"<unmapped-call>":edge.unknown}};
      j::Object ref{{"semantic",edge.semantic},{"target",std::move(target)}}; if(refKeys.insert(jsonText(j::Value(j::Object(ref)))).second) refs.push_back(std::move(ref));
    }
    // 容器签名只包含声明头，独立子对象正文不能污染父节点。
    auto signature=n.signature;
    nodes.push_back(j::Object{{"category",n.category},{"id",n.id},{"language",n.language},{"native_kind",n.kind},{"name",n.name},{"signature",signature},{"code",n.code},{"source",n.source},{"contains",std::move(contains)},{"references",std::move(refs)},{"extensions",j::Object{}}});
    origins.push_back(j::Object{{"node_id",n.id},{"selected_paths",j::Array{n.path}}});
  }
  for(auto &entry:externalMap) externals.push_back(std::move(entry.second));
  j::Array configs,files;
  for(auto &c:r.configs) { auto &p=r.platforms[c.platform]; j::Array ids{p.id,p.resourceId}; for(auto &id:p.additionalIds) ids.push_back(id); configs.push_back(j::Object{{"id",c.id},{"source_paths",j::Array{"compile_commands.json"}},{"platform_ids",std::move(ids)}}); }
  for(auto &file:r.selected) { auto &coverage=g.coverage[file]; require(!coverage.empty(),"selected file has no complete TU context: "+file,"object_extraction_failed"); j::Array ids; for(auto &id:coverage) ids.push_back(id); files.push_back(j::Object{{"path",file},{"disposition","analyzed"},{"configuration_ids",std::move(ids)}}); }
  return j::Object{{"protocol",Protocol},{"type","completed"},{"id",r.id},{"outcome","ok"},{"graph",j::Object{{"root_contains",std::move(roots)},{"nodes",std::move(nodes)},{"external_symbols",std::move(externals)}}},{"evidence",j::Object{{"configurations",std::move(configs)},{"files",std::move(files)},{"origins",std::move(origins)}}}};
}
// 顺序运行真实 LibTooling TU，禁止冷编译并发和任何子进程工具链查询。
static j::Object analyze(Request &r) {
  Graph graph(r);
  for(auto &c:r.configs) {
    r.checkpoint(); tooling::FixedCompilationDatabase db(c.directory.u8string(),c.args);
    llvm::IntrusiveRefCntPtr<ReadBoundary> filesystem(new ReadBoundary(r,r.platforms[c.platform]));
    tooling::ClangTool tool(db,{c.file.u8string()},std::make_shared<PCHContainerOperations>(),filesystem);
    tool.setPrintErrorMessage(false); Diagnostics diagnostics(r.maxDiagnostic); tool.setDiagnosticConsumer(&diagnostics);
    Factory factory(graph,c.id); int status=0;
    try { status=tool.run(&factory); } catch(const Failure &f) { if(f.code=="object_extraction_failed" && !diagnostics.firstError.empty()) throw Failure(f.code,diagnostics.firstError); throw; }
    require(status==0 && diagnostics.getNumErrors()==0,"Clang cannot extract complete objects: "+diagnostics.firstError,"object_extraction_failed");
  }
  return finish(graph);
}
// 错误终结不带部分图和证据。
static j::Object error(const std::string &id,const Failure &f) {
  return j::Object{{"protocol",Protocol},{"type","completed"},{"id",id},{"outcome","error"},{"error",j::Object{{"code",f.code},{"message",std::string(f.what())},{"paths",j::Array{}}}}};
}
// 跨平台非阻塞读取协议管道；-2 表示暂时无数据，0 表示 Host 已关闭。
static int readProtocol(char *buffer,size_t capacity) {
#ifdef _WIN32
  HANDLE pipe=GetStdHandle(STD_INPUT_HANDLE); DWORD available=0;
  if(!PeekNamedPipe(pipe,nullptr,0,nullptr,&available,nullptr)) return GetLastError()==ERROR_BROKEN_PIPE?0:-1;
  if(!available) { std::this_thread::sleep_for(std::chrono::milliseconds(10)); return -2; }
  DWORD readCount=0;
  if(!ReadFile(pipe,buffer,static_cast<DWORD>(std::min<size_t>(capacity,available)),&readCount,nullptr)) return GetLastError()==ERROR_BROKEN_PIPE?0:-1;
  return static_cast<int>(readCount);
#else
  pollfd descriptor{STDIN_FILENO,POLLIN|POLLHUP,0}; int ready=poll(&descriptor,1,10);
  if(ready<0) return -1;
  if(!ready || !(descriptor.revents&(POLLIN|POLLHUP))) return -2;
  return static_cast<int>(::read(STDIN_FILENO,buffer,capacity));
#endif
}
// 清除可能注入 include、驱动器或配置路径的宿主变量。
static void clearEnvironment(const char *name) {
#ifdef _WIN32
  _putenv_s(name,"");
#else
  unsetenv(name);
#endif
}
// 单次协议入口独立轮询取消；完成后立即退出，不等待 stdin EOF。
int main() {
  for(auto name:{"CPATH","C_INCLUDE_PATH","CPLUS_INCLUDE_PATH","OBJC_INCLUDE_PATH","SDKROOT","MACOSX_DEPLOYMENT_TARGET","CCC_OVERRIDE_OPTIONS","CLANG_CONFIG_FILE_SYSTEM_DIR","CLANG_CONFIG_FILE_USER_DIR","INCLUDE","CL","_CL_"}) clearEnvironment(name);
  std::cout<<jsonText(j::Object{{"protocol",Protocol},{"type","ready"},{"helper_id","narrant-clang"},{"helper_version","0.1.0"},{"projection_revision","clang-source-v1"},{"languages",j::Array{"c","cpp"}},{"profiles",j::Array{"clang-static-v1"}}})<<std::endl;
  Request request; std::future<j::Object> work; std::string buffer; bool started=false,closed=false; size_t inputBytes=0;
  try {
    while(true) {
      if(started && work.wait_for(std::chrono::milliseconds(0))==std::future_status::ready) {
        auto result=work.get(); auto output=jsonText(j::Value(std::move(result)));
        if(output.size()>request.maxResult) output=jsonText(error(request.id,Failure("resource_limit","result byte limit exceeded")));
        if(!closed) std::cout<<output<<std::endl; return 0;
      }
      char chunk[4096]; int count=readProtocol(chunk,sizeof(chunk));
      if(count==-1) { request.stopped=true; throw Failure("invalid_input","stdin pipe failed"); }
      if(count!=-2) {
        if(count<=0) { request.stopped=true; closed=true; if(!started) return 1; if(work.valid()) work.wait(); return 1; }
        inputBytes+=count; require(inputBytes<=4*1024*1024,"request byte limit exceeded","resource_limit"); buffer.append(chunk,count);
        size_t newline;
        while((newline=buffer.find('\n'))!=std::string::npos) {
          auto line=buffer.substr(0,newline); buffer.erase(0,newline+1); auto value=parse(line); auto object=value.getAsObject(); require(object,"message must be object");
          if(!started) {
            require(str(*object,"protocol")==Protocol && str(*object,"type")=="analyze","expected analyze"); request.id=str(*object,"id");
            try { configure(request,*object); }
            catch(const Failure &f) { std::cout<<jsonText(error(request.id,f))<<std::endl; return 0; }
            started=true; work=std::async(std::launch::async,[&request] {
              try { return analyze(request); }
              catch(const Failure &f) { if(f.code=="cancelled") return j::Object{{"protocol",Protocol},{"type","completed"},{"id",request.id},{"outcome","cancelled"}}; return error(request.id,f); }
              catch(const std::exception &e) { return error(request.id,Failure("internal_failure",e.what())); }
            });
          } else {
            keys(*object,{"protocol","type","id"}); require(str(*object,"protocol")==Protocol && str(*object,"type")=="cancel" && str(*object,"id")==request.id,"invalid cancel/second analyze"); request.stopped=true;
          }
        }
      }
    }
  } catch(const std::exception &e) { request.stopped=true; if(work.valid()) work.wait(); std::cerr<<"protocol failure: "<<e.what()<<'\n'; return 1; }
}
