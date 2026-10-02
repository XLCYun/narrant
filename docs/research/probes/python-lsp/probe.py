import pathlib, json, subprocess, threading, queue, time, os, sys
BASE=pathlib.Path(__file__).resolve().parent
ROOT=BASE/'fixture'; PKG=ROOT/'src/pkg'; PKG.mkdir(parents=True,exist_ok=True)
files={
'__init__.py':'',
'api.py':'''def deco(fn):
    return fn

@deco
def decorated(value: int) -> int:
    return value

def first() -> str:
    return "first"

def second() -> str:
    return "second"

class Worker:
    def __init__(self):
        self.item = 1

    def run(self) -> str:
        return "run"

def 中文函数() -> int:
    return 1
''',
'stubbed.py':'''def stubbed(value):
    return value
''',
'stubbed.pyi':'''def stubbed(value: int) -> int: ...
''',
'client.py':'''from .api import decorated, first, second, Worker, 中文函数
from .stubbed import stubbed
import pathlib
import json

result = decorated(1)
direct_alias = first
a = direct_alias()
rebound = first
rebound = second
b = rebound()
worker = Worker()
c = worker.run()
d = worker.item
e = stubbed(1)
def choose(flag: bool):
    ambiguous = first if flag else second
    return ambiguous()
g = getattr(worker, "run")()
prefix = "🙂"; h = 中文函数()
i = len([])
j = pathlib.Path("x")
k = json.dumps({})
''',
'absolute_client.py':'''from pkg.api import first
result = first()
'''}
for name,source in files.items():(PKG/name).write_text(source)
(ROOT/'pyproject.toml').write_text('[project]\nname = "narrant-static-probe"\nversion = "0.0.0"\nrequires-python = ">=3.11"\n')
# 受控解释器仅记录服务传入的参数并失败退出，绝不执行分析项目。
fake=BASE/'fake-python'; fake.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> '+str(BASE/'interpreter-invocations.log')+'\nexit 1\n'); fake.chmod(0o755)
settings={'python':{'pythonPath':str(fake),'defaultInterpreterPath':str(fake),'analysis':{'autoSearchPaths':True,'diagnosticMode':'openFilesOnly','typeCheckingMode':'basic','logLevel':'Trace','useLibraryCodeForTypes':True}}}
cmd=['node',str(BASE/'pyright/package/langserver.index.js'),'--stdio']
log=(BASE/'protocol.jsonl').open('w'); stderr=(BASE/'stderr.log').open('w'); q=queue.Queue(); seq=0
# 所有进程均在 finally 中正常退出或有界强制回收。
p=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,env={**os.environ,'PYTHONNOUSERSITE':'1'})
# 按 LSP 帧格式发送 JSON 消息，并记录完整原始请求。
def write(msg):
    log.write(json.dumps({'direction':'send','message':msg},ensure_ascii=False)+'\n');log.flush()
    data=json.dumps(msg,ensure_ascii=False).encode();p.stdin.write(b'Content-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data);p.stdin.flush()
# 从服务 stdout 读取 LSP 响应，保留完整协议日志。
def reader():
    try:
        while True:
            headers={}
            while True:
                line=p.stdout.readline()
                if not line: return
                if line==b'\r\n':break
                k,v=line.decode().split(':',1);headers[k.lower()]=v.strip()
            message=json.loads(p.stdout.read(int(headers['content-length'])))
            log.write(json.dumps({'direction':'receive','message':message},ensure_ascii=False)+'\n');log.flush();q.put(message)
    except Exception as e:q.put({'reader_error':repr(e)})
threading.Thread(target=reader,daemon=True).start()
# 发送无需应答的 LSP 通知。
def notify(method,params):write({'jsonrpc':'2.0','method':method,'params':params})
# 为 Pyright 的配置请求读取对应命名空间。
def config(section):
    if not section:return settings
    cur=settings
    for part in section.split('.'):
        if not isinstance(cur,dict):return None
        cur=cur.get(part)
    return cur
notes=[]
# 处理服务主动配置请求，收集其他通知。
def consume(m):
    if 'method'in m and'id'in m:
        result=[config(x.get('section')) for x in m['params']['items']] if m['method']=='workspace/configuration' else None
        write({'jsonrpc':'2.0','id':m['id'],'result':result})
    else:notes.append(m)
# 发送单个请求，并在有界等待中处理服务回调。
def request(method,params,timeout=20):
    global seq
    seq+=1;i=seq;write({'jsonrpc':'2.0','id':i,'method':method,'params':params});end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:m=q.get(timeout=max(.01,end-time.monotonic()))
        except queue.Empty:break
        if m.get('id')==i and 'method'not in m:return m
        consume(m)
    return {'error':'timeout','request_id':i}
# 将固定用例的源代码列转换为 LSP UTF-16 字符偏移。
def pos_for(filename,line_fragment,token):
    lines=files[filename].splitlines();ln=next(i for i,l in enumerate(lines) if line_fragment in l);col=lines[ln].index(token)
    return {'line':ln,'character':len(lines[ln][:col].encode('utf-16-le'))//2}
# 读取真实声明位置，核对返回范围覆盖的源代码。
def target_context(result):
    if isinstance(result,dict): result=[result]
    out=[]
    for loc in result or []:
        uri=loc.get('uri',loc.get('targetUri'));r=loc.get('range',loc.get('targetSelectionRange'));path=pathlib.Path(__import__('urllib.parse',fromlist=['unquote']).unquote(uri.removeprefix('file://')))
        lines=path.read_text().splitlines();line=lines[r['start']['line']];start=r['start']['character'];end=r['end']['character'];encoded=line.encode('utf-16-le');selected=encoded[start*2:end*2].decode('utf-16-le') if r['start']['line']==r['end']['line'] else '<multiline>'
        out.append({'path':str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path),'range':r,'line_text':line,'selected':selected})
    return out
results={'service':'pyright','version':'1.1.414','command':cmd,'settings':settings,'fixture_root':str(ROOT),'observations':[]}
try:
    results['initialize']=request('initialize',{'processId':os.getpid(),'rootUri':ROOT.as_uri(),'workspaceFolders':[{'uri':ROOT.as_uri(),'name':'fixture'}],'capabilities':{'general':{'positionEncodings':['utf-16']},'workspace':{'configuration':True,'workspaceFolders':True},'textDocument':{'documentSymbol':{'hierarchicalDocumentSymbolSupport':True},'definition':{'linkSupport':True}}}})
    notify('initialized',{});notify('workspace/didChangeConfiguration',{'settings':settings})
    for name in ['api.py','client.py','absolute_client.py']:
        notify('textDocument/didOpen',{'textDocument':{'uri':(PKG/name).as_uri(),'languageId':'python','version':1,'text':files[name]}})
    for name in ['api.py','client.py']:
        results.setdefault('document_symbols',{})[name]=request('textDocument/documentSymbol',{'textDocument':{'uri':(PKG/name).as_uri()}})
    cases=[('relative_import','client.py','result = decorated','decorated'),('immutable_alias','client.py','a = direct_alias','direct_alias'),('runtime_rebinding','client.py','b = rebound','rebound'),('class_constructor','client.py','worker = Worker','Worker'),('instance_method','client.py','c = worker.run','run'),('instance_attribute','client.py','d = worker.item','item'),('stub_precedence','client.py','e = stubbed','stubbed'),('ambiguous_union','client.py','return ambiguous','ambiguous'),('dynamic_getattr','client.py','g = getattr','"run"'),('unicode_utf16','client.py','prefix =','中文函数'),('src_layout_absolute','absolute_client.py','result = first','first'),('builtin_len','client.py','i = len','len'),('stdlib_path','client.py','j = pathlib.Path','Path'),('stdlib_json','client.py','k = json.dumps','dumps')]
    for label,filename,fragment,token in cases:
        pos=pos_for(filename,fragment,token)
        if label=='dynamic_getattr':pos['character']+=1
        response=request('textDocument/definition',{'textDocument':{'uri':(PKG/filename).as_uri()},'position':pos})
        result=response.get('result');results['observations'].append({'case':label,'file':filename,'position':pos,'response':response,'targets':target_context(result)})
    results['shutdown']=request('shutdown',None,timeout=5);notify('exit',None)
finally:
    try:p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.terminate()
        try:p.wait(timeout=3)
        except subprocess.TimeoutExpired:p.kill();p.wait(timeout=3)
    results['process_exit_code']=p.returncode;results['process_id']=p.pid;results['notes']=notes
    results['interpreter_invocations']=(BASE/'interpreter-invocations.log').read_text() if (BASE/'interpreter-invocations.log').exists() else ''
    (BASE/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2));log.close();stderr.close()
print(json.dumps({'initialize':results.get('initialize'),'observations':results['observations'],'interpreter_invocations':results['interpreter_invocations'],'process_exit_code':results['process_exit_code']},ensure_ascii=False,indent=2))
