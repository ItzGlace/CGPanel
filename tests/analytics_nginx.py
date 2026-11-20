#!/usr/bin/env python3
"""Real Nginx tracker injection and collector proxy, using the emitted Rust directives."""
import http.server,json,os,pathlib,re,socket,subprocess,tempfile,threading,time,urllib.request
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1'
source=(pathlib.Path('/project/src/tls_agent.rs')).read_text()
strings=[json.loads('"'+s+'"') for s in re.findall(r'"((?:\\.|[^"\\])*)"',source)]
tracking=next(s for s in strings if s.startswith('proxy_set_header Accept-Encoding'))
tracking=tracking.replace('{id}','a'*32).replace('{key}','b'*32).replace('{pointer}','true')
telemetry=next(s for s in strings if s.startswith('location ^~ /__cgpanel/'))
class Origin(http.server.BaseHTTPRequestHandler):
 encoding=None
 def do_GET(self):
  Origin.encoding=self.headers.get('Accept-Encoding');body=b'<!doctype html><html><head><title>Test</title></head><body>Site</body></html>' if self.path!='/data' else b'{"answer":1}'
  self.send_response(200);self.send_header('Content-Type','application/json' if self.path=='/data' else 'text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*args):pass
class Collector(http.server.BaseHTTPRequestHandler):
 seen=None
 def do_POST(self):
  Collector.seen={'path':self.path,'origin':self.headers.get('Origin'),'client':self.headers.get('X-CGPanel-Client-IP'),'body':json.loads(self.rfile.read(int(self.headers['Content-Length'])))};self.send_response(204);self.end_headers()
 def log_message(self,*args):pass
origin=http.server.ThreadingHTTPServer(('127.0.0.1',0),Origin);collector=http.server.ThreadingHTTPServer(('127.0.0.1',2082),Collector)
for server in [origin,collector]:threading.Thread(target=server.serve_forever,daemon=True).start()
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
with tempfile.TemporaryDirectory() as temp:
 folder=pathlib.Path(temp);folder.chmod(0o755);config=folder/'nginx.conf'
 config.write_text(f'include /etc/nginx/modules-enabled/*.conf; pid {folder}/nginx.pid; error_log {folder}/error.log; events {{}} http {{server {{ listen 127.0.0.1:{port}; {telemetry} location / {{{tracking} proxy_pass http://127.0.0.1:{origin.server_port}; }} }} }}')
 subprocess.run(['nginx','-t','-c',str(config)],check=True,capture_output=True);process=subprocess.Popen(['nginx','-c',str(config),'-g','daemon off;'],stderr=subprocess.DEVNULL)
 try:
  time.sleep(.2);base='http://127.0.0.1:'+str(port)
  with urllib.request.urlopen(urllib.request.Request(base+'/',headers={'Accept-Encoding':'gzip'})) as response:html=response.read().decode()
  assert html.count('/__cgpanel/tracker.js')==2 and 'data-site="'+'a'*32+'"' in html,html
  assert Origin.encoding in (None,''),'Compressed upstream could hide HTML from injection'
  with urllib.request.urlopen(base+'/data') as response:assert json.load(response)=={'answer':1}
  request=urllib.request.Request(base+'/__cgpanel/collect/'+'a'*32,data=b'{"kind":"view"}',headers={'Content-Type':'text/plain','Origin':'https://site.example'})
  with urllib.request.urlopen(request) as response:assert response.status==204
  assert Collector.seen=={'path':'/telemetry/collect/'+'a'*32,'origin':'https://site.example','client':'127.0.0.1','body':{'kind':'view'}},Collector.seen
  print('PASS: native Nginx HTML injection, compressed-upstream bypass, untouched JSON, same-origin telemetry path and forwarded collector headers')
 finally:process.terminate();process.wait(timeout=5);origin.shutdown();collector.shutdown()
