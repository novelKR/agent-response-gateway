import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { verifiedOutput } from './output.mjs';
const root=fileURLToPath(new URL('../../.local/management-web/dist/',import.meta.url));
const files=new Map([...verifiedOutput(root).files].map(([name,bytes])=>['/dashboard/'+name,bytes]));
const port=43141,host='127.0.0.1',authority=`${host}:${port}`;
const server=createServer((request,response)=>{
  if(request.headers.host!==authority || request.headers.origin && request.headers.origin!==`http://${authority}`){response.writeHead(403).end();return;}
  if(!['GET','HEAD'].includes(request.method)){response.writeHead(405).end();return;}
  const pathname=new URL(request.url,`http://${authority}`).pathname;
  const path=pathname==='/dashboard/'?'/dashboard/index.html':pathname;
  const content=files.get(path);
  if(!content){response.writeHead(404,{'Content-Type':'text/plain','Cache-Control':'no-store'}).end('Verified static preview only. The management API is supplied by an authorized host.');return;}
  const type=path.endsWith('.html')?'text/html; charset=utf-8':path.endsWith('.js')?'text/javascript; charset=utf-8':path.endsWith('.css')?'text/css; charset=utf-8':path.endsWith('.json')?'application/json':'text/plain; charset=utf-8';
  response.writeHead(200,{'Content-Type':type,'Cache-Control':'no-store','X-Content-Type-Options':'nosniff','Referrer-Policy':'no-referrer','Content-Security-Policy':"default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"});response.end(request.method==='HEAD'?undefined:content);
});
server.listen(port,host,()=>console.log(`Verified static preview: http://${authority}/dashboard/`));
