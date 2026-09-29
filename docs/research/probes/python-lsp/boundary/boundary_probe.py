"""用固定纯源码与真实 ty LSP 进程检查分析环境边界，不执行任何 fixture。"""
import pathlib,json,subprocess,threading,queue,time,os,urllib.parse,datetime,hashlib
BASE=pathlib.Path(__file__).resolve().parent
BINARY=BASE.parent/'ty/ty-aarch64-apple-darwin/ty'
CONTROL=BASE/'control';CONTROL.mkdir(exist_ok=True)
EMPTY_CONFIG=CONTROL/'empty-ty.toml';EMPTY_CONFIG.write_text('')
EMPTY_PREFIX=CONTROL/'empty-python-prefix';(EMPTY_PREFIX/'lib/python3.14/site-packages').mkdir(parents=True,exist_ok=True)
CACHE=CONTROL/'cache';CONFIG_HOME=CONTROL/'config';CONFIG_HOME.mkdir(exist_ok=True)
FAKE_BIN=CONTROL/'bin';FAKE_BIN.mkdir(exist_ok=True)
FAKE=FAKE_BIN/'python';FAKE.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> '+str(CONTROL/'interpreter-invocations.log')+'\nexit 1\n');FAKE.chmod(0o755)
if not (FAKE_BIN/'python3').exists():(FAKE_BIN/'python3').symlink_to(FAKE)
EXTERNAL=BASE/'outside-source';EXTERNAL.mkdir(exist_ok=True)
(EXTERNAL/'extra_sentinel.py').write_text('EXTERNAL_CONFIG = "external-extra-path"\n')
(EXTERNAL/'linked_sentinel.py').write_text('EXTERNAL_LINK = "external-symlink"\n')
CLIENT='''from internal import repo_value
from pkga.api import pkg_value
from nestedpkg.api import nested_value
from extra_sentinel import EXTERNAL_CONFIG
from venv_sentinel import EXTERNAL_VENV
from linked_sentinel import EXTERNAL_LINK
import pathlib

root_result = repo_value()
src_result = pkg_value()
nested_result = nested_value()
extra_result = EXTERNAL_CONFIG
venv_result = EXTERNAL_VENV
link_result = EXTERNAL_LINK
stdlib_result = pathlib.Path("x")
builtin_result = len([])
'''
# 创建一个包含可区分声明、污染配置和伪环境的固定源码仓库。
def fixture(name,config_kind,nested=False,linked=False):
 root=BASE/'fixtures'/name;root.mkdir(parents=True,exist_ok=True)
 (root/'internal.py').write_text('def repo_value():\n    return "repository-root"\n')
 pkg=root/'src/pkga';pkg.mkdir(parents=True,exist_ok=True);(pkg/'__init__.py').write_text('');(pkg/'api.py').write_text('def pkg_value():\n    return "repository-src"\n')
 venv=root/'.venv';(venv/'lib/python3.14/site-packages').mkdir(parents=True,exist_ok=True)
 (venv/'lib/python3.14/site-packages/venv_sentinel.py').write_text('EXTERNAL_VENV = "user-environment-dependency"\n')
 (venv/'pyvenv.cfg').write_text('home = /nonexistent\ninclude-system-site-packages = false\nversion = 3.14.0\n')
 config='root = ['+json.dumps(str(root))+', '+json.dumps(str(root/'src'))+']\nextra-paths = ['+json.dumps(str(EXTERNAL))+']\npython = '+json.dumps(str(venv))+'\npython-version = "3.14"\npython-platform = "darwin"\n'
 project='[project]\nname = "boundary-fixture"\nversion = "0.0.0"\nrequires-python = ">=3.14"\n'
 (root/'pyproject.toml').write_text(project+ ('\n[tool.ty.environment]\n'+config if config_kind=='pyproject' else ''))
 if config_kind=='ty':(root/'ty.toml').write_text('[environment]\n'+config)
 roots=[str(root),str(root/'src')]
 client=root/'client.py'
 if nested:
  child=root/'services/app';child.mkdir(parents=True,exist_ok=True)
  childpkg=child/'src/nestedpkg';childpkg.mkdir(parents=True,exist_ok=True);(childpkg/'__init__.py').write_text('');(childpkg/'api.py').write_text('def nested_value():\n    return "repository-nested-src"\n')
  nested_config=config.replace('root = [','root = ['+json.dumps(str(child/'src'))+', ',1)
  (child/'pyproject.toml').write_text('[project]\nname = "nested-boundary-fixture"\nversion = "0.0.0"\nrequires-python = ">=3.14"\n[tool.ty.environment]\n'+nested_config)
  client=child/'client.py';roots.append(str(child/'src'))
 if linked:
  path=root/'linked_sentinel.py'
  if not path.exists():path.symlink_to(EXTERNAL/'linked_sentinel.py')
 client.write_text(CLIENT)
 return {'root':root,'client':client,'roots':roots}
# 根据会话目的生成配置；固定空文件模式屏蔽项目自动发现的污染配置。
def settings_for(fix,mode):
 s={'diagnosticMode':'off'}
 if mode in ['inline_empty','fixed']:
  s['configuration']={'environment':{'root':fix['roots'],'extra-paths':[],'python':str(EMPTY_PREFIX),'python-version':'3.14','python-platform':'darwin'}}
 if mode=='fixed':s['configurationFile']=str(EMPTY_CONFIG)
 return s
# 对真实返回的位置读取源码，记录词法路径和规范化路径以识别符号链接穿越。
def contexts(result,root):
 out=[]
 for x in result or []:
  uri=x.get('uri',x.get('targetUri'));rng=x.get('range',x.get('targetSelectionRange'))
  if not uri.startswith('file://'):
   out.append({'uri':uri,'range':rng,'kind':'non-file'});continue
  file=pathlib.Path(urllib.parse.unquote(urllib.parse.urlparse(uri).path));resolved=file.resolve();line=file.read_text().splitlines()[rng['start']['line']]
  start=rng['start']['character'];end=rng['end']['character'];selected=line.encode('utf-16-le')[2*start:2*end].decode('utf-16-le')
  kind='repo' if resolved.is_relative_to(root.resolve()) else 'stdlib' if resolved.is_relative_to(CACHE.resolve()) else 'outside'
  if '/.venv/' in str(resolved):kind='user-venv'
  out.append({'uri':uri,'path':str(file),'canonical_path':str(resolved),'kind':kind,'range':rng,'line_text':line,'selected':selected})
 return out
# 创建有界 LSP 会话，完成定义查询后 shutdown/exit 并确认进程回收。
def run_session(name,fix,mode):
 output=BASE/'sessions'/name;output.mkdir(parents=True,exist_ok=True)
 settings=settings_for(fix,mode);options={'untrustedWorkspace':True,'experimental':{'useUv':'off'},**settings}
 env={k:v for k,v in os.environ.items() if not k.startswith(('TY_','PYTHON','UV_','VIRTUAL_ENV','CONDA','PIP'))}
 env.update({'PATH':str(FAKE_BIN),'XDG_CONFIG_HOME':str(CONFIG_HOME),'XDG_CACHE_HOME':str(CACHE),'PYTHONNOUSERSITE':'1'})
 err=(output/'stderr.log').open('w');log=(output/'protocol.jsonl').open('w');messages=queue.Queue();seq=0
 process=subprocess.Popen([str(BINARY),'server'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,cwd=fix['root'],env=env)
 # 编码并记录请求，保持完整协议便于检查服务是否正确获取了设置。
 def send(message):
  log.write(json.dumps({'direction':'send','message':message},ensure_ascii=False)+'\n');log.flush();data=json.dumps(message,ensure_ascii=False).encode();process.stdin.write(b'Content-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data);process.stdin.flush()
 # 独立读取服务 stdout，避免等待响应时阻塞配置回调。
 def reader():
  try:
   while True:
    headers={}
    while True:
     line=process.stdout.readline()
     if not line:return
     if line==b'\r\n':break
     key,value=line.decode().split(':',1);headers[key.lower()]=value.strip()
    message=json.loads(process.stdout.read(int(headers['content-length'])));log.write(json.dumps({'direction':'receive','message':message},ensure_ascii=False)+'\n');log.flush();messages.put(message)
  except Exception as error:messages.put({'reader_error':repr(error)})
 threading.Thread(target=reader,daemon=True).start();notes=[];configuration_requests=[]
 # 在同一等待循环中回答服务的 workspace/configuration 等请求。
 def request(method,params,timeout=10):
  nonlocal seq
  seq+=1;ident=seq;send({'jsonrpc':'2.0','id':ident,'method':method,'params':params});deadline=time.monotonic()+timeout
  while time.monotonic()<deadline:
   try:message=messages.get(timeout=max(.01,deadline-time.monotonic()))
   except queue.Empty:break
   if message.get('id')==ident and 'method'not in message:return message
   if 'method'in message and'id'in message:
    if message['method']=='workspace/configuration':
     configuration_requests.append(message['params']);answer=[settings if item.get('section')=='ty' else None for item in message['params']['items']]
    else:answer=None
    send({'jsonrpc':'2.0','id':message['id'],'result':answer})
   else:notes.append(message)
  return {'error':'timeout','id':ident}
 result={'name':name,'mode':mode,'root':str(fix['root']),'client':str(fix['client']),'settings':settings,'initializationOptions':options,'env_overrides':{k:env[k] for k in ['PATH','XDG_CONFIG_HOME','XDG_CACHE_HOME','PYTHONNOUSERSITE']},'queries':[],'pid':process.pid}
 try:
  result['initialize']=request('initialize',{'processId':os.getpid(),'rootUri':fix['root'].as_uri(),'workspaceFolders':[{'uri':fix['root'].as_uri(),'name':name}],'initializationOptions':options,'capabilities':{'general':{'positionEncodings':['utf-16']},'workspace':{'configuration':True,'workspaceFolders':True},'textDocument':{'definition':{'linkSupport':True}}}})
  send({'jsonrpc':'2.0','method':'initialized','params':{}})
  send({'jsonrpc':'2.0','method':'textDocument/didOpen','params':{'textDocument':{'uri':fix['client'].as_uri(),'languageId':'python','version':1,'text':CLIENT}}})
  cases=[('root','root_result','repo_value'),('src','src_result','pkg_value'),('nested','nested_result','nested_value'),('extra','extra_result','EXTERNAL_CONFIG'),('venv','venv_result','EXTERNAL_VENV'),('symlink','link_result','EXTERNAL_LINK'),('stdlib','stdlib_result','Path'),('builtin','builtin_result','len')]
  for label,fragment,token in cases:
   line=next(i for i,s in enumerate(CLIENT.splitlines()) if fragment in s);column=CLIENT.splitlines()[line].index(token);position={'line':line,'character':column}
   response=request('textDocument/definition',{'textDocument':{'uri':fix['client'].as_uri()},'position':position})
   result['queries'].append({'case':label,'position':position,'response':response,'targets':contexts(response.get('result'),fix['root'])})
  result['shutdown']=request('shutdown',None,5);send({'jsonrpc':'2.0','method':'exit','params':None})
 except Exception as error:result['exception']=repr(error)
 finally:
  result['termination']='normal'
  try:process.wait(timeout=4)
  except subprocess.TimeoutExpired:
   process.terminate();result['termination']='terminate'
   try:process.wait(timeout=2)
   except subprocess.TimeoutExpired:process.kill();process.wait(timeout=2);result['termination']='kill'
  result.update({'exit_code':process.returncode,'configuration_requests':configuration_requests,'notes':notes})
  err.close();log.close();result['stderr']=(output/'stderr.log').read_text();(output/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
 return result
# 顺序执行独立会话，每个会话均先结束，再启动下一会话。
def main():
 fixtures={'ty':fixture('repo-ty','ty'),'pyproject':fixture('repo-pyproject','pyproject'),'nested':fixture('repo-nested','none',nested=True),'symlink':fixture('repo-symlink','ty',linked=True)}
 scenarios=[('01-default-ty','ty','default'),('02-default-pyproject','pyproject','default'),('03-inline-empty-arrays','ty','inline_empty'),('04-fixed-ty','ty','fixed'),('05-fixed-pyproject','pyproject','fixed'),('06-default-nested','nested','default'),('07-fixed-nested','nested','fixed'),('08-fixed-symlink','symlink','fixed')]
 results=[]
 for name,which,mode in scenarios:
  result=run_session(name,fixtures[which],mode);results.append(result);print(json.dumps({'name':name,'exit_code':result['exit_code'],'exception':result.get('exception'),'targets':{q['case']:[{'kind':t['kind'],'selected':t.get('selected'),'path':t.get('path'),'canonical':t.get('canonical_path')} for t in q['targets']] for q in result['queries']}},ensure_ascii=False),flush=True)
 (BASE/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
 processes=subprocess.run(['ps','-axo','pid=,ppid=,args='],capture_output=True,text=True,check=True).stdout.splitlines()
 lifecycle={'checked_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sessions':[{'name':r['name'],'pid':r['pid'],'exit_code':r['exit_code'],'termination':r['termination']} for r in results],'remaining_servers':[p for p in processes if str(BINARY)+' server' in p and 'python3 - <<' not in p],'interpreter_invocations':(CONTROL/'interpreter-invocations.log').read_text() if (CONTROL/'interpreter-invocations.log').exists() else ''}
 (BASE/'lifecycle.json').write_text(json.dumps(lifecycle,ensure_ascii=False,indent=2))
if __name__=='__main__':main()
